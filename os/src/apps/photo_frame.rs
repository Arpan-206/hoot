//! Photo frame: shows the newest photo from a frame-server.
//!
//! This is the Sprig version of the ESP32 photo frame. The server renders
//! the photo, with any message, at 160x128 and serves it as raw RGB565 at
//! `<server>/frame/<name>.rgb565`. The frame polls with `If-Modified-Since`,
//! streams a new photo straight into a flash blob slot, then shows it from
//! there. The last photo therefore survives reboots and server outages.
//!
//! Heartbeat, warnings, server commands and firmware updates are not this
//! app's business: the OS agent (`agent.rs`) does them whatever is on
//! screen. A "refetch" or "clear-cache" command reaches this app through
//! the config it reads before every poll.
//!
//! Live photos: when the server holds motion frames for the current photo
//! (`X-Sprig-Live`), the frame plays them every so often while online,
//! one frame per request straight into RAM, then settles back on the
//! still from flash. Offline, the still is all there is.
//!
//! Behaviour follows the original: once a photo is on screen nothing draws
//! over it. Trouble shows on the left LED instead: two pulses when a working
//! network is lost, three when the server cannot be reached.

use hoot_gfx::{BYTES, CELL_HEIGHT, Framebuffer, Rgb565};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::dimmer::Dimmer;
use crate::drivers::input::Button;
use crate::hw::Pwm;
use crate::net::{FetchRequest, FetchResult, FixedStr, JobState, NetState, Sink};
use crate::storage::{Config, Storage, StorageError};
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Photo frame", group: Group::Frame, needs_network: true };

/// How often to ask the server for a new photo, and in battery saver mode.
const POLL_MS: u32 = 15_000;
const POLL_SAVER_MS: u32 = 60_000;
/// How soon to try again after a failure.
const RETRY_MS: u32 = 5_000;
/// Hold L this long to forget the cached timestamp and fetch again.
const FORCE_HOLD_MS: u32 = 2_000;
/// Blob kind tag for photos: "PHOT".
const KIND_PHOTO: u32 = 0x5048_4F54;
/// Photos alternate between two slots, so the old one stays valid until the
/// new one is complete.
const SLOT_A: u8 = 0;
const SLOT_B: u8 = 1;
/// How often the motion plays, how long to wait after a failed play, and
/// how long the last frame holds before the still returns.
const LIVE_EVERY_MS: u32 = 45_000;
const LIVE_RETRY_MS: u32 = 300_000;
const LIVE_HOLD_MS: u32 = 500;

/// Forget the cached photo: both slots and the stored timestamp. The next
/// poll fetches the photo afresh.
pub fn clear_cache(store: &mut Storage) -> Result<(), StorageError> {
    info!("photo: clearing cache");
    store.blob_erase(SLOT_A)?;
    store.blob_erase(SLOT_B)?;
    store.update_config(|c| {
        c.frame_last_modified.clear();
        c.frame_slot = SLOT_A;
    })
}

/// Non-blocking LED pulses on the left LED.
struct Blink {
    half_pulses: u8,
    on: bool,
    next_ms: u32,
}

impl Blink {
    const fn new() -> Self {
        Self { half_pulses: 0, on: false, next_ms: 0 }
    }

    fn start(&mut self, pulses: u8, now: u32) {
        if self.half_pulses == 0 {
            self.half_pulses = pulses * 2;
            self.next_ms = now;
        }
    }

    fn tick(&mut self, now: u32, led: &mut Dimmer<Pwm>) {
        if self.half_pulses == 0 || now.wrapping_sub(self.next_ms) >= 1 << 31 {
            return;
        }
        self.on = !self.on;
        self.half_pulses -= 1;
        led.set(if self.on && self.half_pulses > 0 { 160 } else { 0 });
        self.next_ms = now.wrapping_add(120);
    }
}

pub struct PhotoFrame {
    photo_on_screen: bool,
    fetching: bool,
    pending_slot: u8,
    next_poll_ms: u32,
    force: bool,
    fails: u8,
    last_error: StrBuf<24>,
    blink: Blink,
    hold_started: Option<u32>,
    /// Motion frames the server holds for the photo on screen.
    live_frames: u8,
    live_next_ms: u32,
    /// The frame being fetched while the motion plays.
    live_playing: Option<u8>,
    live_started_ms: u32,
    /// When to put the still back after the last frame.
    live_restore_ms: Option<u32>,
    /// K was pressed: play now, battery saver or not.
    live_manual: bool,
}

impl PhotoFrame {
    pub const fn new() -> Self {
        Self {
            photo_on_screen: false,
            fetching: false,
            pending_slot: SLOT_A,
            next_poll_ms: 0,
            force: false,
            fails: 0,
            last_error: StrBuf::new(),
            blink: Blink::new(),
            hold_started: None,
            live_frames: 0,
            live_next_ms: 0,
            live_playing: None,
            live_started_ms: 0,
            live_restore_ms: None,
            live_manual: false,
        }
    }

    fn live_url(cfg: &Config, n: u8) -> FixedStr<128> {
        let mut u = FixedStr::new();
        u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
        u.push_str("/live/");
        u.push_str(cfg.frame_name.as_str());
        let tail: StrBuf<16> = format(format_args!("/{n}.rgb565"));
        u.push_str(tail.as_str());
        u
    }

    fn start_live(&mut self, ctx: &mut Ctx, n: u8) {
        let request = FetchRequest::get(Self::live_url(ctx.store.config(), n), Sink::Frame);
        match ctx.net.fetch(request) {
            Ok(()) => self.live_playing = Some(n),
            Err(e) => {
                warn!("live: {}", e.label());
                self.live_stop(ctx, LIVE_RETRY_MS);
            }
        }
    }

    /// Back to the still, and plan the next play.
    fn live_stop(&mut self, ctx: &mut Ctx, next_in_ms: u32) {
        self.live_playing = None;
        self.live_restore_ms = None;
        self.live_next_ms = ctx.now_ms.wrapping_add(next_in_ms);
        let slot = ctx.store.config().frame_slot;
        Self::show_slot(ctx, slot);
    }

    fn live_finish(&mut self, ctx: &mut Ctx, r: FetchResult, n: u8) {
        let now = ctx.now_ms;
        if r.status != 200 || r.len as usize != BYTES {
            warn!("live: frame {} HTTP {} len {}", n, r.status, r.len);
            self.live_stop(ctx, LIVE_RETRY_MS);
            return;
        }
        let Ctx { net, fb, .. } = ctx;
        net.live_frame(|raw| fb.load_raw(raw));
        if n + 1 < self.live_frames {
            self.start_live(ctx, n + 1);
        } else {
            info!("live: {} frames in {} ms", self.live_frames, now.wrapping_sub(self.live_started_ms));
            self.live_playing = None;
            self.live_restore_ms = Some(now.wrapping_add(LIVE_HOLD_MS));
            self.live_next_ms = now.wrapping_add(LIVE_EVERY_MS);
        }
    }

    fn photo_url(cfg: &Config) -> FixedStr<128> {
        let mut u = FixedStr::new();
        u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
        u.push_str("/frame/");
        u.push_str(cfg.frame_name.as_str());
        u.push_str(".rgb565");
        u
    }

    /// Copy a complete photo from a blob slot to the screen.
    fn show_slot(ctx: &mut Ctx, slot: u8) -> bool {
        let Some(header) = ctx.store.blob_header(slot) else { return false };
        if header.kind != KIND_PHOTO || header.len as usize != BYTES {
            return false;
        }
        let Some(data) = ctx.store.blob_data(slot) else { return false };
        let Ok(raw) = <&[u8; BYTES]>::try_from(data) else { return false };
        ctx.fb.load_raw(raw);
        true
    }

    fn start_fetch(&mut self, ctx: &mut Ctx) {
        let cfg = ctx.store.config();
        let slot = if cfg.frame_slot == SLOT_A { SLOT_B } else { SLOT_A };
        let mut request = FetchRequest::get(Self::photo_url(cfg), Sink::Blob { slot, kind: KIND_PHOTO, seq: 0 });
        if !self.force {
            request.if_modified_since = cfg.frame_last_modified;
        }
        info!("photo: fetch {} into slot {}", request.url.as_str(), slot);
        match ctx.net.fetch(request) {
            Ok(()) => {
                self.fetching = true;
                self.pending_slot = slot;
            }
            Err(e) => self.fail(ctx.now_ms, e.label()),
        }
    }

    fn fail(&mut self, now: u32, what: &str) {
        warn!("photo: {what} (failure {})", self.fails + 1);
        self.fails = self.fails.saturating_add(1);
        self.last_error = format(format_args!("{what}"));
        self.next_poll_ms = now.wrapping_add(RETRY_MS);
        self.blink.start(3, now);
    }

    fn finish(&mut self, ctx: &mut Ctx, r: FetchResult) {
        let now = ctx.now_ms;
        info!("photo: HTTP {} len {} last-modified '{}' live {}", r.status, r.len, r.last_modified.as_str(), r.live);
        if r.status == 200 || r.status == 304 {
            if r.live > 0 && self.live_frames == 0 {
                self.live_next_ms = now.wrapping_add(3_000);
            }
            self.live_frames = r.live;
        }
        match r.status {
            200 if r.len as usize == BYTES => {
                let slot = self.pending_slot;
                if Self::show_slot(ctx, slot) {
                    let _ = ctx.store.update_config(|c| {
                        c.frame_slot = slot;
                        c.frame_last_modified = r.last_modified;
                    });
                    self.photo_on_screen = true;
                    self.fails = 0;
                    self.force = false;
                    self.last_error.clear();
                    self.next_poll_ms = now.wrapping_add(if ctx.saver { POLL_SAVER_MS } else { POLL_MS });
                } else {
                    self.fail(now, "bad photo");
                }
            }
            304 => {
                self.fails = 0;
                self.next_poll_ms = now.wrapping_add(if ctx.saver { POLL_SAVER_MS } else { POLL_MS });
            }
            200 => {
                let s: StrBuf<24> = format(format_args!("size {}", r.len));
                self.fail(now, s.as_str());
            }
            code => {
                let s: StrBuf<24> = format(format_args!("HTTP {code}"));
                self.fail(now, s.as_str());
            }
        }
    }

    fn draw_status(&self, ctx: &mut Ctx, state: NetState) {
        if state == NetState::Portal {
            crate::ui::setup::draw(ctx.fb);
            return;
        }
        let has_radio = ctx.net.has_radio();
        let server = ctx.store.config().frame_server;
        let name = ctx.store.config().frame_name;
        let fb = &mut *ctx.fb;
        theme::screen(fb, "Photo frame", "");
        let step = CELL_HEIGHT + 3;
        let mut y = theme::CONTENT_Y;
        row(fb, y, "Network", state.label(), theme::net_color(state));
        y += step;
        row(fb, y, "Server", server.as_str().trim_start_matches("http://"), theme::TEXT);
        y += step;
        row(fb, y, "Frame", name.as_str(), theme::TEXT);
        y += step;
        let status: &str = if !has_radio {
            "needs a Pico W"
        } else if !self.last_error.is_empty() {
            self.last_error.as_str()
        } else if self.fetching {
            "fetching..."
        } else {
            "waiting for photo"
        };
        let color = if !has_radio || !self.last_error.is_empty() { theme::WARN } else { theme::TEXT };
        row(fb, y, "Status", status, color);
        theme::footer(fb, "K play   hold L: refetch");
    }
}

fn row(fb: &mut Framebuffer, y: i32, label: &str, value: &str, color: Rgb565) {
    fb.draw_text(4, y, label, theme::MUTED, None);
    fb.draw_text(52, y, value, color, None);
}

impl App for PhotoFrame {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let slot = ctx.store.config().frame_slot;
        self.photo_on_screen = Self::show_slot(ctx, slot);
        info!("photo: enter, cached photo in slot {}: {}", slot, self.photo_on_screen);
        self.fetching = false;
        // No cached photo, for example after the flash layout changed:
        // fetch again even if the server says nothing is new.
        self.force = !self.photo_on_screen;
        self.fails = 0;
        self.last_error.clear();
        self.next_poll_ms = ctx.now_ms;
        self.hold_started = None;
        ctx.net.request_connect();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;

        // Hold L to forget the cached timestamp and fetch the photo again.
        if ctx.input.held(Button::L) {
            let started = *self.hold_started.get_or_insert(now);
            if now.wrapping_sub(started) >= FORCE_HOLD_MS && !self.force {
                self.force = true;
                self.next_poll_ms = now;
            }
        } else {
            self.hold_started = None;
        }
        // K plays the motion of a live photo now.
        if ctx.input.just_pressed(Button::K) && self.photo_on_screen && self.live_playing.is_none() {
            if self.live_frames > 0 && ctx.net.state().is_up() {
                self.live_manual = true;
                self.live_next_ms = now;
                audio::play(Sound::Tick);
            } else {
                info!("live: nothing to play (frames {}, online {})", self.live_frames, ctx.net.state().is_up());
            }
        }

        let state = ctx.net.state();
        if let Some(n) = self.live_playing {
            match ctx.net.take_job() {
                JobState::Done(r) => self.live_finish(ctx, r, n),
                JobState::Failed(e) => {
                    warn!("live: {}", e.label());
                    self.live_stop(ctx, LIVE_RETRY_MS);
                }
                _ => {}
            }
        } else if let Some(at) = self.live_restore_ms
            && now.wrapping_sub(at) < 1 << 31
        {
            self.live_restore_ms = None;
            let slot = ctx.store.config().frame_slot;
            Self::show_slot(ctx, slot);
        } else if self.photo_on_screen
            && !self.fetching
            && self.live_frames > 0
            && (!ctx.saver || self.live_manual)
            && state.is_up()
            && now.wrapping_sub(self.live_next_ms) < 1 << 31
        {
            self.live_manual = false;
            self.live_started_ms = now;
            self.start_live(ctx, 0);
        } else if self.fetching {
            match ctx.net.take_job() {
                JobState::Done(r) => {
                    self.fetching = false;
                    self.finish(ctx, r);
                }
                JobState::Failed(e) => {
                    self.fetching = false;
                    self.fail(now, e.label());
                }
                _ => {}
            }
        } else if now.wrapping_sub(self.next_poll_ms) < 1 << 31 && self.live_playing.is_none() {
            if state.is_up() {
                self.start_fetch(ctx);
            } else {
                self.next_poll_ms = now.wrapping_add(RETRY_MS);
                // Two pulses only when a working link went away, not while
                // the radio is still joining after power-on.
                let lost = matches!(state, NetState::Lost | NetState::JoinFailed);
                if self.photo_on_screen && lost {
                    self.blink.start(2, now);
                }
            }
        }

        self.blink.tick(now, &mut ctx.hw.led_left);
        if !self.photo_on_screen {
            self.draw_status(ctx, state);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.hw.led_left.set(0);
        self.blink = Blink::new();
        self.live_playing = None;
        self.live_restore_ms = None;
    }
}

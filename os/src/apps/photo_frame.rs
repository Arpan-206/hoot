//! Photo frame: shows the newest photo from a frame-server.
//!
//! This is the Sprig version of the ESP32 photo frame. The server renders
//! the photo, with any message, at 160x128 and serves it as raw RGB565 at
//! `<server>/frame/<name>.rgb565`. The frame polls with `If-Modified-Since`,
//! streams a new photo straight into a flash blob slot, then shows it from
//! there. The last photo therefore survives reboots and server outages.
//!
//! Behaviour follows the original: once a photo is on screen nothing draws
//! over it. Trouble shows on the left LED instead: two pulses when the
//! network is down, three when the server cannot be reached.

use sprig_gfx::{BYTES, CELL_HEIGHT, Framebuffer, Rgb565};

use crate::apps::{App, AppInfo, Ctx, Transition, back_pressed};
use crate::drivers::dimmer::Dimmer;
use crate::drivers::input::Button;
use crate::hw::Pwm;
use crate::net::{FetchRequest, FetchResult, FixedStr, JobState, NetState, Sink};
use crate::storage::{Config, Storage, StorageError};
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Photo frame", needs_network: true };

/// How often to ask the server for a new photo.
const POLL_MS: u32 = 15_000;
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

/// Forget the cached photo: both slots and the stored timestamp. The next
/// visit to the app fetches the photo afresh.
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
        }
    }

    fn url(cfg: &Config) -> FixedStr<128> {
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
        let request = FetchRequest {
            url: Self::url(cfg),
            if_modified_since: if self.force { FixedStr::new() } else { cfg.frame_last_modified },
            sink: Sink::Blob { slot, kind: KIND_PHOTO },
        };
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
        info!("photo: HTTP {} len {} last-modified '{}'", r.status, r.len, r.last_modified.as_str());
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
                    self.next_poll_ms = now.wrapping_add(POLL_MS);
                } else {
                    self.fail(now, "bad photo");
                }
            }
            304 => {
                self.fails = 0;
                self.next_poll_ms = now.wrapping_add(POLL_MS);
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
        theme::footer(fb, "J back   hold L: refetch");
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
        self.force = false;
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

        let state = ctx.net.state();
        if self.fetching {
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
        } else if now.wrapping_sub(self.next_poll_ms) < 1 << 31 {
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
    }
}

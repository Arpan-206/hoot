//! Slideshow: the last few photos sent to this frame, one after another.
//!
//! The server keeps an album of recent uploads at 160x128 and lists their
//! ids at `<server>/album/<name>`, newest first. The app keeps up to six
//! of them in flash blob slots 2 to 7, tagged with the id, and cycles
//! through them newest first. Photos survive reboots and outages; the list
//! is refreshed every few minutes and missing photos fetched one at a
//! time. A/D step by hand, W/S change the dwell time.

use sprig_gfx::{BYTES, CELL_HEIGHT, Framebuffer, Rgb565, WIDTH};
use sprig_proto::album;

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::net::{FetchRequest, FetchResult, FixedStr, JobState, Lane, NetState, Sink};
use crate::storage::Config;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Slideshow", group: Group::Frame, needs_network: true };

/// Blob kind tag for album photos: "ALBM".
const KIND_ALBUM: u32 = 0x414C_424D;
const FIRST_SLOT: u8 = 2;
const SLOTS: usize = 6;
const LIST_MAX: usize = 8;
const LIST_MS: u32 = 5 * 60_000;
const RETRY_MS: u32 = 15_000;
const DWELL_DEFAULT_S: u8 = 20;
const OVERLAY_MS: u32 = 1_500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    None,
    List,
    Photo { slot: u8, id: u32 },
}

pub struct Slideshow {
    /// Album ids newest first, from the server or from the cached slots.
    ids: [u32; LIST_MAX],
    count: usize,
    job: Job,
    next_list_ms: u32,
    next_switch_ms: u32,
    dwell_s: u8,
    /// Id of the photo on screen and its position in `ids`.
    showing: Option<(u32, usize)>,
    overlay_until: Option<u32>,
    status: StrBuf<24>,
    status_drawn: bool,
}

fn url(cfg: &Config, path: &str, tail: &str) -> FixedStr<128> {
    let mut u = FixedStr::new();
    u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
    u.push_str(path);
    u.push_str(cfg.frame_name.as_str());
    u.push_str(tail);
    u
}

fn due(now: u32, at: u32) -> bool {
    now.wrapping_sub(at) < 1 << 31
}

/// The album id held in `slot`, if it holds a complete album photo.
fn slot_id(ctx: &Ctx, slot: u8) -> Option<u32> {
    let h = ctx.store.blob_header(slot)?;
    (h.kind == KIND_ALBUM && h.len as usize == BYTES).then_some(h.seq)
}

fn find_slot(ctx: &Ctx, id: u32) -> Option<u8> {
    (FIRST_SLOT..FIRST_SLOT + SLOTS as u8).find(|&s| slot_id(ctx, s) == Some(id))
}

impl Slideshow {
    pub const fn new() -> Self {
        Self {
            ids: [0; LIST_MAX],
            count: 0,
            job: Job::None,
            next_list_ms: 0,
            next_switch_ms: 0,
            dwell_s: DWELL_DEFAULT_S,
            showing: None,
            overlay_until: None,
            status: StrBuf::new(),
            status_drawn: false,
        }
    }

    /// The ids worth keeping: the newest that fit in the slots.
    fn wanted(&self) -> &[u32] {
        &self.ids[..self.count.min(SLOTS)]
    }

    /// Start from what flash already holds, newest first.
    fn seed_from_cache(&mut self, ctx: &Ctx) {
        self.count = 0;
        for slot in FIRST_SLOT..FIRST_SLOT + SLOTS as u8 {
            if let Some(id) = slot_id(ctx, slot) {
                // Insert keeping the list sorted, newest first.
                let mut i = self.count;
                while i > 0 && self.ids[i - 1] < id {
                    self.ids[i] = self.ids[i - 1];
                    i -= 1;
                }
                self.ids[i] = id;
                self.count += 1;
            }
        }
    }

    /// The newest wanted photo not yet in flash, and a slot to put it in.
    fn next_missing(&self, ctx: &Ctx) -> Option<(u32, u8)> {
        let wanted = self.wanted();
        let id = *wanted.iter().find(|&&id| find_slot(ctx, id).is_none())?;
        let slot = (FIRST_SLOT..FIRST_SLOT + SLOTS as u8)
            .find(|&s| slot_id(ctx, s).is_none_or(|held| !wanted.contains(&held)))?;
        Some((id, slot))
    }

    fn show(&mut self, ctx: &mut Ctx, pos: usize) -> bool {
        let id = self.ids[pos];
        let Some(slot) = find_slot(ctx, id) else { return false };
        let Some(data) = ctx.store.blob_data(slot) else { return false };
        let Ok(raw) = <&[u8; BYTES]>::try_from(data) else { return false };
        ctx.fb.load_raw(raw);
        self.showing = Some((id, pos));
        self.next_switch_ms = ctx.now_ms.wrapping_add(self.dwell_s as u32 * 1000);
        true
    }

    /// Show the next cached photo in `direction` (1 or -1) from the current one.
    fn step(&mut self, ctx: &mut Ctx, direction: i32) {
        let n = self.wanted().len();
        if n == 0 {
            return;
        }
        let start = self.showing.map_or(0, |(_, pos)| pos);
        for k in 1..=n {
            let pos = (start as i32 + direction * k as i32).rem_euclid(n as i32) as usize;
            if self.show(ctx, pos) {
                return;
            }
        }
    }

    fn set_status(&mut self, text: &str) {
        self.status = format(format_args!("{text}"));
        self.status_drawn = false;
    }

    fn start_list(&mut self, ctx: &mut Ctx) {
        let request = FetchRequest::get(url(ctx.store.config(), "/album/", ""), Sink::Small);
        match ctx.net.fetch(request) {
            Ok(()) => self.job = Job::List,
            Err(e) => {
                self.set_status(e.label());
                self.next_list_ms = ctx.now_ms.wrapping_add(RETRY_MS);
            }
        }
    }

    fn start_photo(&mut self, ctx: &mut Ctx, id: u32, slot: u8) {
        let tail: StrBuf<20> = format(format_args!("/{id}.rgb565"));
        let request = FetchRequest::get(
            url(ctx.store.config(), "/album/", tail.as_str()),
            Sink::Blob { slot, kind: KIND_ALBUM, seq: id },
        );
        info!("album: fetch {} into slot {}", id, slot);
        match ctx.net.fetch(request) {
            Ok(()) => self.job = Job::Photo { slot, id },
            Err(e) => {
                self.set_status(e.label());
                self.next_list_ms = ctx.now_ms.wrapping_add(RETRY_MS);
            }
        }
    }

    fn finish_list(&mut self, ctx: &mut Ctx, r: FetchResult) {
        if r.status != 200 {
            let s: StrBuf<20> = format(format_args!("HTTP {}", r.status));
            self.set_status(s.as_str());
            self.next_list_ms = ctx.now_ms.wrapping_add(RETRY_MS);
            return;
        }
        let mut ids = [0u32; LIST_MAX];
        let count = ctx.net.small_body_on(Lane::App, |body| {
            album::parse_ids(core::str::from_utf8(body).unwrap_or(""), &mut ids)
        });
        info!("album: {} photos listed", count);
        self.ids = ids;
        self.count = count;
        self.status.clear();
        self.status_drawn = false;
        self.next_list_ms = ctx.now_ms.wrapping_add(LIST_MS);
        // Keep the position in step with the new order.
        if let Some((id, _)) = self.showing {
            self.showing = self.wanted().iter().position(|&x| x == id).map(|pos| (id, pos));
        }
    }

    fn finish_photo(&mut self, ctx: &mut Ctx, r: FetchResult, id: u32) {
        if r.status == 200 && r.len as usize == BYTES {
            info!("album: photo {} stored", id);
            if self.showing.is_none() {
                self.step(ctx, 0);
            }
        } else {
            warn!("album: photo {} HTTP {} len {}", id, r.status, r.len);
            // Drop it from the list until the next refresh.
            if let Some(pos) = self.ids[..self.count].iter().position(|&x| x == id) {
                self.ids.copy_within(pos + 1..self.count, pos);
                self.count -= 1;
            }
        }
    }

    fn draw_status(&mut self, ctx: &mut Ctx, state: NetState) {
        let cfg = ctx.store.config();
        let server = cfg.frame_server;
        let name = cfg.frame_name;
        let cached = self.wanted().iter().filter(|&&id| find_slot(ctx, id).is_some()).count();
        let fb = &mut *ctx.fb;
        theme::screen(fb, "Slideshow", "");
        let step = CELL_HEIGHT + 3;
        let mut y = theme::CONTENT_Y;
        row(fb, y, "Network", state.label(), theme::net_color(state));
        y += step;
        row(fb, y, "Server", server.as_str().trim_start_matches("http://"), theme::TEXT);
        y += step;
        row(fb, y, "Frame", name.as_str(), theme::TEXT);
        y += step;
        let photos: StrBuf<20> = format(format_args!("{} listed, {} here", self.count, cached));
        row(fb, y, "Album", photos.as_str(), theme::TEXT);
        y += step;
        let status: &str = if !self.status.is_empty() {
            self.status.as_str()
        } else if self.job != Job::None {
            "fetching..."
        } else {
            "waiting for photos"
        };
        row(fb, y, "Status", status, if self.status.is_empty() { theme::TEXT } else { theme::WARN });
        theme::footer(fb, "J back");
        self.status_drawn = true;
    }
}

fn row(fb: &mut Framebuffer, y: i32, label: &str, value: &str, color: Rgb565) {
    fb.draw_text(4, y, label, theme::MUTED, None);
    fb.draw_text(52, y, value, color, None);
}

impl App for Slideshow {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.job = Job::None;
        self.showing = None;
        self.overlay_until = None;
        self.status.clear();
        self.status_drawn = false;
        self.seed_from_cache(ctx);
        info!("album: {} cached photos", self.count);
        self.step(ctx, 0);
        self.next_list_ms = ctx.now_ms;
        ctx.net.request_connect();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let state = ctx.net.state();

        if ctx.input.just_pressed(Button::D) {
            self.step(ctx, 1);
        }
        if ctx.input.just_pressed(Button::A) {
            self.step(ctx, -1);
        }
        let mut dwell_changed = false;
        if ctx.input.repeat(Button::W) && self.dwell_s < 120 {
            self.dwell_s += 5;
            dwell_changed = true;
        }
        if ctx.input.repeat(Button::S) && self.dwell_s > 5 {
            self.dwell_s -= 5;
            dwell_changed = true;
        }
        if dwell_changed && self.showing.is_some() {
            let text: StrBuf<12> = format(format_args!("{} s", self.dwell_s));
            ctx.fb.fill_rect(WIDTH - 40, 4, 36, CELL_HEIGHT + 4, theme::BAR);
            ctx.fb.draw_text_right(WIDTH - 6, 6, text.as_str(), theme::TEXT, None);
            self.overlay_until = Some(now.wrapping_add(OVERLAY_MS));
            self.next_switch_ms = now.wrapping_add(self.dwell_s as u32 * 1000);
        }
        if let Some(until) = self.overlay_until
            && due(now, until)
        {
            self.overlay_until = None;
            if let Some((_, pos)) = self.showing {
                self.show(ctx, pos);
            }
        }

        // Network work, one request at a time.
        match self.job {
            Job::None => {
                if state.is_up() {
                    if due(now, self.next_list_ms) {
                        self.start_list(ctx);
                    } else if let Some((id, slot)) = self.next_missing(ctx) {
                        self.start_photo(ctx, id, slot);
                    }
                }
            }
            job => match ctx.net.take_job() {
                JobState::Done(r) => {
                    self.job = Job::None;
                    match job {
                        Job::List => self.finish_list(ctx, r),
                        Job::Photo { id, .. } => self.finish_photo(ctx, r, id),
                        Job::None => {}
                    }
                }
                JobState::Failed(e) => {
                    self.job = Job::None;
                    warn!("album: {}", e.label());
                    self.set_status(e.label());
                    self.next_list_ms = now.wrapping_add(RETRY_MS);
                }
                _ => {}
            },
        }

        if self.showing.is_some() {
            if due(now, self.next_switch_ms) && self.overlay_until.is_none() {
                self.step(ctx, 1);
            }
        } else if !self.status_drawn {
            self.draw_status(ctx, state);
        }
        Transition::Stay
    }
}

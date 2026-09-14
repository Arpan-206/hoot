//! Weather: the forecast for this frame's town. The server looks the town
//! up once and fetches the forecast every half hour; the device shows the
//! current sky, temperature, wind and humidity, and the next three days.

use hoot_gfx::{Framebuffer, Rgb565};
use hoot_proto::time::WEEKDAYS;
use hoot_proto::weather::{self, Day, Sky};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::net::{FetchRequest, FetchResult, FixedStr, JobState, Lane, NetState, Sink};
use crate::storage::Config;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Weather", group: Group::Frame, needs_network: true };

const REFRESH_MS: u32 = 30 * 60_000;
const RETRY_MS: u32 = 60_000;
const RAIN: Rgb565 = Rgb565::hex(0x5DA9E9);

/// What we keep of a forecast, without the borrowed text.
#[derive(Clone, Copy)]
struct Stored {
    place: FixedStr<20>,
    temp: i16,
    code: u8,
    wind: u8,
    humidity: u8,
    days: [Day; 3],
    day_count: usize,
}

pub struct Weather {
    data: Option<Stored>,
    fetching: bool,
    next_fetch_ms: u32,
    status: StrBuf<24>,
    dirty: bool,
}

fn url(cfg: &Config) -> FixedStr<128> {
    let mut u = FixedStr::new();
    u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
    u.push_str("/weather/");
    u.push_str(cfg.frame_name.as_str());
    u
}

impl Weather {
    pub const fn new() -> Self {
        Self { data: None, fetching: false, next_fetch_ms: 0, status: StrBuf::new(), dirty: true }
    }

    fn start(&mut self, ctx: &mut Ctx) {
        let request = FetchRequest::get(url(ctx.store.config()), Sink::Small);
        match ctx.net.fetch(request) {
            Ok(()) => {
                self.fetching = true;
                self.dirty = true;
            }
            Err(e) => self.fail(ctx.now_ms, e.label()),
        }
    }

    fn fail(&mut self, now: u32, what: &str) {
        warn!("weather: {what}");
        self.status = format(format_args!("{what}"));
        self.next_fetch_ms = now.wrapping_add(RETRY_MS);
        self.dirty = true;
    }

    fn finish(&mut self, ctx: &mut Ctx, r: FetchResult) {
        let now = ctx.now_ms;
        if r.status == 404 {
            self.fail(now, "no place set (web page)");
            self.next_fetch_ms = now.wrapping_add(REFRESH_MS);
            return;
        }
        if r.status != 200 {
            let s: StrBuf<24> = format(format_args!("HTTP {}", r.status));
            self.fail(now, s.as_str());
            return;
        }
        let parsed = ctx.net.small_body_on(Lane::App, |body| {
            weather::parse(core::str::from_utf8(body).unwrap_or("")).map(|f| Stored {
                place: FixedStr::truncated(f.place),
                temp: f.temp,
                code: f.code,
                wind: f.wind,
                humidity: f.humidity,
                days: f.days,
                day_count: f.day_count,
            })
        });
        match parsed {
            Some(s) => {
                info!("weather: {} {} C code {}", s.place.as_str(), s.temp, s.code);
                self.data = Some(s);
                self.status.clear();
                self.next_fetch_ms = now.wrapping_add(REFRESH_MS);
                self.dirty = true;
            }
            None => self.fail(now, "bad forecast"),
        }
    }
}

impl App for Weather {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.dirty = true;
        self.fetching = false;
        if self.data.is_none() {
            self.next_fetch_ms = ctx.now_ms;
        }
        ctx.net.request_connect();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let state = ctx.net.state();
        if ctx.input.just_pressed(Button::K) && !self.fetching {
            self.next_fetch_ms = now;
        }
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
        } else if now.wrapping_sub(self.next_fetch_ms) < 1 << 31 && state.is_up() {
            self.start(ctx);
        }
        if self.dirty {
            self.dirty = false;
            match self.data {
                Some(d) => draw(ctx.fb, &d, self.fetching),
                None => draw_status(ctx, state, self.fetching, self.status.as_str()),
            }
        }
        Transition::Stay
    }
}

// Icons, 16 by 16, rows as bits with the most significant bit on the left.
const SUN: [u16; 16] = [
    0x0180, 0x2184, 0x1008, 0x03C0, 0x07E0, 0x0FF0, 0x0FF0, 0xCFF3, 0x0FF0, 0x0FF0, 0x07E0, 0x03C0, 0x1008,
    0x2184, 0x0180, 0,
];
const SUN_SMALL: [u16; 16] = [0x0018, 0x0042, 0x0018, 0x00BD, 0x003C, 0x0018, 0x0042, 0x0018, 0, 0, 0, 0, 0, 0, 0, 0];
const CLOUD: [u16; 16] =
    [0, 0, 0, 0, 0x0780, 0x0FC0, 0x3FE0, 0x7FF8, 0xFFFC, 0xFFFE, 0xFFFE, 0x7FFC, 0, 0, 0, 0];
const DROPS: [u16; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1110, 0x2220, 0x4440, 0];
const DRIZZLE: [u16; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1010, 0, 0x0440, 0];
const FLAKES: [u16; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1110, 0, 0x0444, 0];
const BOLT: [u16; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0300, 0x0600, 0x0780, 0x0180, 0x0300];
const FOG: [u16; 16] = [0, 0, 0, 0, 0, 0x3FFC, 0, 0, 0x0FFF, 0, 0, 0x7FF8, 0, 0, 0x1FFC, 0];

fn layer(fb: &mut Framebuffer, x: i32, y: i32, rows: &[u16; 16], color: Rgb565, scale: i32) {
    let mut bytes = [0u8; 32];
    for (i, row) in rows.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&row.to_be_bytes());
    }
    fb.draw_bitmap_scaled(x, y, 16, 16, &bytes, color, scale);
}

/// The sky as a small picture.
pub fn draw_icon(fb: &mut Framebuffer, x: i32, y: i32, code: u8, scale: i32) {
    match weather::sky(code) {
        Sky::Clear => layer(fb, x, y, &SUN, theme::ACCENT, scale),
        Sky::PartlyCloudy => {
            layer(fb, x, y, &SUN_SMALL, theme::ACCENT, scale);
            layer(fb, x, y, &CLOUD, theme::TEXT, scale);
        }
        Sky::Cloudy => layer(fb, x, y, &CLOUD, theme::MUTED, scale),
        Sky::Fog => layer(fb, x, y, &FOG, theme::MUTED, scale),
        Sky::Drizzle => {
            layer(fb, x, y, &CLOUD, theme::MUTED, scale);
            layer(fb, x, y, &DRIZZLE, RAIN, scale);
        }
        Sky::Rain | Sky::Showers => {
            layer(fb, x, y, &CLOUD, theme::MUTED, scale);
            layer(fb, x, y, &DROPS, RAIN, scale);
        }
        Sky::Snow => {
            layer(fb, x, y, &CLOUD, theme::MUTED, scale);
            layer(fb, x, y, &FLAKES, theme::TEXT, scale);
        }
        Sky::Thunder => {
            layer(fb, x, y, &CLOUD, theme::MUTED, scale);
            layer(fb, x, y, &BOLT, theme::WARN, scale);
        }
    }
}

fn draw(fb: &mut Framebuffer, d: &Stored, fetching: bool) {
    theme::screen(fb, "Weather", d.place.as_str());
    draw_icon(fb, 6, 20, d.code, 2);
    let temp: StrBuf<8> = format(format_args!("{}", d.temp));
    let w = fb.draw_text_scaled(46, 22, temp.as_str(), theme::TEXT, None, 3);
    fb.draw_text(46 + w + 2, 22, "C", theme::MUTED, None);
    fb.draw_text(46, 46, weather::sky(d.code).label(), theme::ACCENT, None);
    let line: StrBuf<26> = format(format_args!("wind {} km/h  hum {}%", d.wind, d.humidity));
    fb.draw_text(4, 60, line.as_str(), theme::MUTED, None);

    for (i, day) in d.days[..d.day_count].iter().enumerate() {
        let x = 10 + i as i32 * 52;
        fb.draw_text(x + 8, 74, WEEKDAYS[day.weekday as usize % 7], theme::TEXT, None);
        draw_icon(fb, x + 6, 83, day.code, 1);
        let t: StrBuf<12> = format(format_args!("{}/{}", day.max, day.min));
        fb.draw_text_centered_in(x - 6, x + 42, 101, t.as_str(), theme::MUTED);
    }
    theme::footer(fb, if fetching { "updating..." } else { "K refresh   J back" });
}

fn draw_status(ctx: &mut Ctx, state: NetState, fetching: bool, status: &str) {
    let cfg = ctx.store.config();
    let server = cfg.frame_server;
    let name = cfg.frame_name;
    let fb = &mut *ctx.fb;
    theme::screen(fb, "Weather", "");
    let step = 11;
    let mut y = theme::CONTENT_Y;
    for (label, value, color) in [
        ("Network", state.label(), theme::net_color(state)),
        ("Server", server.as_str().trim_start_matches("http://"), theme::TEXT),
        ("Frame", name.as_str(), theme::TEXT),
    ] {
        fb.draw_text(4, y, label, theme::MUTED, None);
        fb.draw_text(52, y, value, color, None);
        y += step;
    }
    let text = if !status.is_empty() {
        status
    } else if fetching {
        "fetching..."
    } else {
        "waiting for the network"
    };
    fb.draw_text(4, y, "Status", theme::MUTED, None);
    fb.draw_text(52, y, text, if status.is_empty() { theme::TEXT } else { theme::WARN }, None);
    fb.draw_text(4, y + 22, "Set the town on the page.", theme::MUTED, None);
    theme::footer(fb, "K retry   J back");
}

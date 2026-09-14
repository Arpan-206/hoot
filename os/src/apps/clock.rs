//! Clock: big time, date and where the time came from. On a Pico W the
//! server sets it with every heartbeat. W/S move the hour and A/D the
//! minute by hand, which any board can use; L zeroes the seconds.

use hoot_gfx::Framebuffer;
use hoot_proto::time::Civil;

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::clock::{self, Source};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Clock", group: Group::Tools, needs_network: false };

/// Where a hand-set clock starts from: 2026-01-01 12:00.
const UNSET_START_SECS: u32 = 1_767_225_600 + 12 * 3600;

pub struct Clock {
    drawn: Option<(u32, Source)>,
}

impl Clock {
    pub const fn new() -> Self {
        Self { drawn: None }
    }
}

fn adjust(now_ms: u32, delta: i32, zero_seconds: bool) {
    let base = clock::now_secs(now_ms).unwrap_or(UNSET_START_SECS);
    let mut secs = (base as i64 + delta as i64).max(0) as u32;
    if zero_seconds {
        secs -= secs % 60;
    }
    clock::set(secs, now_ms, Source::Manual);
}

impl App for Clock {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let input = ctx.input;
        if input.repeat(Button::W) {
            adjust(now, 3600, false);
        }
        if input.repeat(Button::S) {
            adjust(now, -3600, false);
        }
        if input.repeat(Button::D) {
            adjust(now, 60, true);
        }
        if input.repeat(Button::A) {
            adjust(now, -60, true);
        }
        if input.just_pressed(Button::L) {
            adjust(now, 0, true);
        }
        let key = (clock::now_secs(now).unwrap_or(0), clock::source());
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            draw(ctx.fb, clock::now(now), key.1);
        }
        Transition::Stay
    }
}

fn draw(fb: &mut Framebuffer, time: Option<Civil>, source: Source) {
    theme::screen(fb, "Clock", if source == Source::Unset { "" } else { source.label() });
    match time {
        Some(c) => {
            let hm: StrBuf<8> = format(format_args!("{:02}:{:02}", c.hour, c.minute));
            fb.draw_text_centered(30, hm.as_str(), theme::TEXT, None, 3);
            let s: StrBuf<4> = format(format_args!("{:02}", c.second));
            fb.draw_text_centered(56, s.as_str(), theme::MUTED, None, 1);
            let date: StrBuf<20> =
                format(format_args!("{} {} {} {}", c.weekday_name(), c.day, c.month_name(), c.year));
            fb.draw_text_centered(74, date.as_str(), theme::ACCENT, None, 1);
        }
        None => {
            fb.draw_text_centered(30, "--:--", theme::MUTED, None, 3);
            fb.draw_text_centered(60, "clock not set", theme::WARN, None, 1);
            fb.draw_text_centered(74, "a Pico W gets it from the server", theme::MUTED, None, 1);
        }
    }
    theme::footer(fb, "W/S hour  A/D min  L zero");
}

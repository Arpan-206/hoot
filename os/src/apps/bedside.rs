//! Bedside: a dim clock for the night. The backlight drops low, the time
//! shows large in amber, with the date and the alarm if one is set. Any
//! key brings the light up for a few seconds; J leaves and restores it.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::time::Civil;

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::clock;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Bedside", group: Group::Tools, needs_network: false };

/// Backlight level at night, out of 255.
const DIM: u8 = 10;
/// How long a key press keeps the light up.
const BRIGHT_MS: u32 = 8_000;

pub struct Bedside {
    bright_until_ms: u32,
    drawn: Option<(u32, bool, u16, bool)>,
}

impl Bedside {
    pub const fn new() -> Self {
        Self { bright_until_ms: 0, drawn: None }
    }
}

impl App for Bedside {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.bright_until_ms = ctx.now_ms.wrapping_add(BRIGHT_MS);
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        if ctx.input.just_pressed_mask() != 0 {
            self.bright_until_ms = now.wrapping_add(BRIGHT_MS);
        }
        let bright = now.wrapping_sub(self.bright_until_ms) >= 1 << 31;
        // Set it every frame: the idle dimmer must not have the last word.
        ctx.hw.backlight.set(if bright { 255 } else { DIM });

        let minute = clock::now_secs(now).map_or(u32::MAX, |s| s / 60);
        let cfg = ctx.store.config();
        let key = (minute, cfg.alarm_on != 0, cfg.alarm_min, bright);
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            draw(ctx.fb, clock::now(now), cfg.alarm_on != 0, cfg.alarm_min, bright);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.hw.backlight.set(255);
    }
}

fn draw(fb: &mut Framebuffer, time: Option<Civil>, alarm_on: bool, alarm_min: u16, bright: bool) {
    fb.clear(theme::BG);
    let color = if bright { theme::TEXT } else { theme::ACCENT };
    match time {
        Some(c) => {
            let hm: StrBuf<8> = format(format_args!("{:02}:{:02}", c.hour, c.minute));
            fb.draw_text_centered(38, hm.as_str(), color, None, 4);
            let date: StrBuf<20> = format(format_args!("{} {} {}", c.weekday_name(), c.day, c.month_name()));
            fb.draw_text_centered(78, date.as_str(), theme::MUTED, None, 1);
        }
        None => {
            fb.draw_text_centered(38, "--:--", theme::MUTED, None, 4);
            fb.draw_text_centered(78, "clock not set", theme::MUTED, None, 1);
        }
    }
    if alarm_on {
        let a: StrBuf<16> = format(format_args!("alarm {:02}:{:02}", alarm_min / 60, alarm_min % 60));
        fb.draw_text_centered(94, a.as_str(), theme::MUTED, None, 1);
    }
    if bright {
        fb.draw_text(3, theme::FOOTER_Y, "J back", theme::MUTED, None);
        fb.draw_text_right(WIDTH - 3, theme::FOOTER_Y, "dims in a moment", theme::MUTED, None);
    }
}

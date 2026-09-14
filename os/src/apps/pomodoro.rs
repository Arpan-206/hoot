//! Pomodoro: work for 25 minutes, rest for 5, repeat.
//!
//! The end time is stored as an instant, so the clock keeps running while
//! another screen is up, and `background` makes the change from there too,
//! so the chime sounds even from the menu. LEDs show the phase: left glows
//! during work, right during a break. At a change both blink for a few
//! seconds. The speaker plays a rising chime when a work session is done
//! and a falling one when the break is over.

use hoot_gfx::{CELL_HEIGHT, Framebuffer, WIDTH};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Pomodoro", group: Group::Tools, needs_network: false };

const ALARM_MS: u32 = 4_000;
const LED_GLOW: u8 = 30;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Work,
    Break,
}

pub struct Pomodoro {
    phase: Phase,
    work_min: u8,
    break_min: u8,
    /// Remaining time while paused, or `None` while running.
    paused_ms: Option<u32>,
    /// When the running phase ends.
    ends_at_ms: u32,
    sessions: u8,
    alarm_until_ms: u32,
    last_drawn: (Phase, u32, bool, u8, u8),
}

impl Pomodoro {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Ready,
            work_min: 25,
            break_min: 5,
            paused_ms: None,
            ends_at_ms: 0,
            sessions: 0,
            alarm_until_ms: 0,
            last_drawn: (Phase::Ready, u32::MAX, false, 0, 0),
        }
    }

    fn remaining_ms(&self, now: u32) -> u32 {
        match (self.phase, self.paused_ms) {
            (Phase::Ready, _) => self.work_min as u32 * 60_000,
            (_, Some(left)) => left,
            (_, None) => {
                let left = self.ends_at_ms.wrapping_sub(now);
                if left >= 1 << 31 { 0 } else { left }
            }
        }
    }

    fn start(&mut self, phase: Phase, now: u32) {
        let minutes = if phase == Phase::Work { self.work_min } else { self.break_min };
        self.phase = phase;
        self.paused_ms = None;
        self.ends_at_ms = now.wrapping_add(minutes as u32 * 60_000);
    }

    fn advance(&mut self, now: u32) {
        let next = match self.phase {
            Phase::Work => {
                self.sessions = self.sessions.saturating_add(1);
                Phase::Break
            }
            _ => Phase::Work,
        };
        self.start(next, now);
        self.alarm_until_ms = now.wrapping_add(ALARM_MS);
        audio::play(if next == Phase::Break { Sound::Done } else { Sound::Rest });
    }
}

impl App for Pomodoro {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.last_drawn.1 = u32::MAX;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let input = ctx.input;
        let running = self.phase != Phase::Ready && self.paused_ms.is_none();

        if input.just_pressed(Button::L) {
            match (self.phase, self.paused_ms) {
                (Phase::Ready, _) => self.start(Phase::Work, now),
                (_, Some(left)) => {
                    self.paused_ms = None;
                    self.ends_at_ms = now.wrapping_add(left);
                }
                (_, None) => self.paused_ms = Some(self.remaining_ms(now)),
            }
            audio::play(Sound::Tick);
        }
        if input.just_pressed(Button::K) {
            if self.phase == Phase::Ready {
                self.sessions = 0;
            } else {
                self.phase = Phase::Ready;
                self.paused_ms = None;
                audio::play(Sound::Tick);
            }
        }
        if self.phase == Phase::Ready {
            if input.repeat(Button::W) {
                self.work_min = (self.work_min + 5).min(90);
            }
            if input.repeat(Button::S) {
                self.work_min = self.work_min.saturating_sub(5).max(5);
            }
            if input.repeat(Button::D) {
                self.break_min = (self.break_min + 1).min(30);
            }
            if input.repeat(Button::A) {
                self.break_min = self.break_min.saturating_sub(1).max(1);
            }
        }

        if running && self.remaining_ms(now) == 0 {
            self.advance(now);
        }
        let alarm = now.wrapping_sub(self.alarm_until_ms) >= 1 << 31;

        // LEDs: glow for the phase, blink at a change.
        let (left, right) = if alarm {
            let on = (now / 150).is_multiple_of(2);
            (if on { 200 } else { 0 }, if on { 200 } else { 0 })
        } else {
            match (self.phase, self.paused_ms.is_some()) {
                (Phase::Work, false) => (LED_GLOW, 0),
                (Phase::Break, false) => (0, LED_GLOW),
                _ => (0, 0),
            }
        };
        ctx.hw.led_left.set(left);
        ctx.hw.led_right.set(right);

        // Redraw only when something visible changed.
        let secs = self.remaining_ms(now).div_ceil(1000);
        let key = (self.phase, secs, self.paused_ms.is_some() || alarm, self.work_min, self.break_min);
        if key != self.last_drawn {
            self.last_drawn = key;
            draw(ctx.fb, self, secs, alarm);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.hw.led_left.set(0);
        ctx.hw.led_right.set(0);
    }

    fn background(&mut self, ctx: &mut Ctx) {
        let now_ms = ctx.now_ms;
        let running = self.phase != Phase::Ready && self.paused_ms.is_none();
        if running && self.remaining_ms(now_ms) == 0 {
            self.advance(now_ms);
        }
    }
}

fn draw(fb: &mut Framebuffer, p: &Pomodoro, secs: u32, alarm: bool) {
    let (label, color) = match p.phase {
        Phase::Ready => ("Ready", theme::MUTED),
        Phase::Work => ("Work", theme::ACCENT),
        Phase::Break => ("Break", theme::WARN),
    };
    theme::screen(fb, "Pomodoro", label);
    if alarm {
        fb.fill_rect(0, theme::TITLE_H + 1, WIDTH, theme::FOOTER_Y - theme::TITLE_H - 1, theme::ACCENT_DARK);
    }
    let time: StrBuf<8> = format(format_args!("{:02}:{:02}", secs / 60, secs % 60));
    let time_color = if p.paused_ms.is_some() { theme::MUTED } else { color };
    fb.draw_text_centered(34, time.as_str(), time_color, None, 3);

    // One dot per finished work session, up to eight.
    let dots = p.sessions.min(8) as i32;
    let x0 = (WIDTH - dots * 8) / 2;
    for i in 0..dots {
        fb.fill_rect(x0 + i * 8, 64, 5, 5, theme::ACCENT);
    }

    let plan: StrBuf<28> = format(format_args!("{} min work, {} min break", p.work_min, p.break_min));
    fb.draw_text_centered(78, plan.as_str(), theme::MUTED, None, 1);
    if p.phase == Phase::Ready {
        fb.draw_text_centered(78 + CELL_HEIGHT + 2, "W/S work  A/D break", theme::MUTED, None, 1);
    }
    let hint = match (p.phase, p.paused_ms.is_some()) {
        (Phase::Ready, _) => "L start   K clear   J back",
        (_, true) => "L resume   K stop   J back",
        (_, false) => "L pause   K stop   J back",
    };
    theme::footer(fb, hint);
}

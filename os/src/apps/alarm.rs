//! Alarm: one daily alarm. W/S and A/D set the time, L turns it on or
//! off, K picks the tone and plays it. It fires through `background`, so
//! the shell brings this screen up from wherever you are, with the tone
//! on repeat and both LEDs flashing. L stops it, K snoozes for five
//! minutes. It needs the clock: from the server on a Pico W, or set by
//! hand in the Clock app.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::time::{civil_from_secs, days_from_secs};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound, TUNES};
use crate::clock;
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Alarm", group: Group::Tools, needs_network: false };

const SNOOZE_SECS: u32 = 5 * 60;
/// Give up ringing after this long with nobody there.
const RING_MAX_MS: u32 = 120_000;
const TONE_GAP_MS: u32 = 400;
const MINUTES_PER_DAY: u16 = 24 * 60;

pub struct Alarm {
    ringing: bool,
    ring_started_ms: u32,
    next_tone_ms: u32,
    /// Day number of the last firing, so it fires once per day.
    fired_day: i64,
    /// Local seconds when a snoozed alarm rings again.
    snooze_until: Option<u32>,
    // Edited copies of the config, saved on exit.
    minute: u16,
    on: bool,
    tone: u8,
    drawn: Option<(u16, bool, u8, Option<u16>, bool)>,
}

impl Alarm {
    pub const fn new() -> Self {
        Self {
            ringing: false,
            ring_started_ms: 0,
            next_tone_ms: 0,
            fired_day: i64::MIN,
            snooze_until: None,
            minute: 0,
            on: false,
            tone: 0,
            drawn: None,
        }
    }

    pub fn is_ringing(&self) -> bool {
        self.ringing
    }

    fn tune(&self) -> (&'static str, Sound) {
        TUNES[self.tone as usize % TUNES.len()]
    }

    fn stop(&mut self, ctx: &mut Ctx) {
        self.ringing = false;
        ctx.hw.led_left.set(0);
        ctx.hw.led_right.set(0);
        self.drawn = None;
    }
}

fn hhmm(minute: u16) -> StrBuf<8> {
    format(format_args!("{:02}:{:02}", minute / 60, minute % 60))
}

impl App for Alarm {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let c = ctx.store.config();
        self.minute = c.alarm_min.min(MINUTES_PER_DAY - 1);
        self.on = c.alarm_on != 0;
        self.tone = c.alarm_tone;
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let now = ctx.now_ms;
        let input = ctx.input;

        if self.ringing {
            if input.just_pressed(Button::L) || input.just_pressed(Button::J) {
                info!("alarm: stopped");
                self.stop(ctx);
            } else if input.just_pressed(Button::K) {
                self.snooze_until = clock::now_secs(now).map(|s| s + SNOOZE_SECS);
                info!("alarm: snoozed");
                self.stop(ctx);
            } else if now.wrapping_sub(self.ring_started_ms) >= RING_MAX_MS {
                info!("alarm: gave up ringing");
                self.stop(ctx);
            }
        }
        if self.ringing {
            if now.wrapping_sub(self.next_tone_ms) < 1 << 31 {
                let (_, sound) = self.tune();
                audio::play(sound);
                self.next_tone_ms = now.wrapping_add(sound.len_ms() + TONE_GAP_MS);
            }
            let on = (now / 250).is_multiple_of(2);
            ctx.hw.led_left.set(if on { 255 } else { 0 });
            ctx.hw.led_right.set(if on { 0 } else { 255 });
            if self.drawn.is_none_or(|d| !d.4) {
                self.drawn = Some((self.minute, self.on, self.tone, None, true));
                draw_ringing(ctx.fb, self.minute);
            }
            return Transition::Stay;
        }

        if back_pressed(input) {
            return Transition::Exit;
        }
        if input.repeat(Button::W) {
            self.minute = (self.minute + 60) % MINUTES_PER_DAY;
        }
        if input.repeat(Button::S) {
            self.minute = (self.minute + MINUTES_PER_DAY - 60) % MINUTES_PER_DAY;
        }
        if input.repeat(Button::D) {
            self.minute = (self.minute + 1) % MINUTES_PER_DAY;
        }
        if input.repeat(Button::A) {
            self.minute = (self.minute + MINUTES_PER_DAY - 1) % MINUTES_PER_DAY;
        }
        if input.just_pressed(Button::L) {
            self.on = !self.on;
            self.snooze_until = None;
            audio::play(Sound::Tick);
        }
        if input.just_pressed(Button::K) {
            self.tone = ((self.tone as usize + 1) % TUNES.len()) as u8;
            audio::play(self.tune().1);
        }

        let clock_minute = clock::now(now).map(|c| c.minute_of_day());
        let key = (self.minute, self.on, self.tone, clock_minute, false);
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            draw(ctx.fb, self, clock_minute);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.hw.led_left.set(0);
        ctx.hw.led_right.set(0);
        let c = ctx.store.config();
        if c.alarm_min != self.minute || (c.alarm_on != 0) != self.on || c.alarm_tone != self.tone {
            let (minute, on, tone) = (self.minute, self.on, self.tone);
            let _ = ctx.store.update_config(|c| {
                c.alarm_min = minute;
                c.alarm_on = on as u8;
                c.alarm_tone = tone;
            });
            info!("alarm saved: {} on {} tone {}", minute, on, tone);
        }
    }

    fn background(&mut self, ctx: &mut Ctx) {
        if self.ringing || ctx.store.config().alarm_on == 0 {
            return;
        }
        let Some(secs) = clock::now_secs(ctx.now_ms) else { return };
        let day = days_from_secs(secs as i64);
        let due = match self.snooze_until {
            Some(t) => secs >= t,
            None => {
                civil_from_secs(secs as i64).minute_of_day() == ctx.store.config().alarm_min
                    && self.fired_day != day
            }
        };
        if due {
            info!("alarm: ringing");
            self.ringing = true;
            self.fired_day = day;
            self.snooze_until = None;
            self.ring_started_ms = ctx.now_ms;
            self.next_tone_ms = ctx.now_ms;
            self.drawn = None;
        }
    }
}

fn draw(fb: &mut Framebuffer, a: &Alarm, clock_minute: Option<u16>) {
    theme::screen(fb, "Alarm", if a.on { "on" } else { "off" });
    let color = if a.on { theme::ACCENT } else { theme::MUTED };
    fb.draw_text_centered(28, hhmm(a.minute).as_str(), color, None, 3);
    let tone: StrBuf<20> = format(format_args!("Tone: {}", a.tune().0));
    fb.draw_text_centered(58, tone.as_str(), theme::TEXT, None, 1);
    let line: StrBuf<28> = match (clock_minute, a.snooze_until) {
        (_, Some(_)) => format(format_args!("snoozed")),
        (Some(m), None) => format(format_args!("clock {}", hhmm(m).as_str())),
        (None, None) => format(format_args!("clock not set")),
    };
    let line_color = if clock_minute.is_none() { theme::WARN } else { theme::MUTED };
    fb.draw_text_centered(72, line.as_str(), line_color, None, 1);
    fb.draw_text_centered(90, "W/S hour  A/D min", theme::MUTED, None, 1);
    theme::footer(fb, "L on/off   K tone   J back");
}

fn draw_ringing(fb: &mut Framebuffer, minute: u16) {
    theme::screen(fb, "Alarm", "ringing");
    fb.fill_rect(0, theme::TITLE_H + 1, WIDTH, theme::FOOTER_Y - theme::TITLE_H - 1, theme::ACCENT_DARK);
    fb.draw_text_centered(34, hhmm(minute).as_str(), theme::TEXT, None, 3);
    fb.draw_text_centered(70, "wake up", theme::ACCENT, None, 2);
    theme::footer(fb, "L stop   K snooze 5 min");
}

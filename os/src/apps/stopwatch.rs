//! Stopwatch: start, stop, laps. Time comes from the frame clock, so it
//! keeps counting while another screen is up. Every press gives a tick.

use sprig_gfx::{CELL_HEIGHT, Framebuffer, WIDTH};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Stopwatch", group: Group::Tools, needs_network: false };

/// Laps kept. The oldest drops off when the list is full.
const MAX_LAPS: usize = 6;
/// Lap rows that fit under the big time.
const SHOWN_LAPS: usize = 4;

pub struct Stopwatch {
    /// Time counted before the current run.
    banked_ms: u32,
    /// When the current run began, or `None` while stopped.
    started_ms: Option<u32>,
    /// Elapsed total at the end of each lap, oldest first.
    laps: [u32; MAX_LAPS],
    lap_count: usize,
    /// Laps dropped from the front, so numbers stay right.
    lap_base: usize,
    last_drawn: (u32, bool, usize),
}

impl Stopwatch {
    pub const fn new() -> Self {
        Self {
            banked_ms: 0,
            started_ms: None,
            laps: [0; MAX_LAPS],
            lap_count: 0,
            lap_base: 0,
            last_drawn: (u32::MAX, false, 0),
        }
    }

    fn elapsed_ms(&self, now: u32) -> u32 {
        self.banked_ms.wrapping_add(self.started_ms.map_or(0, |t| now.wrapping_sub(t)))
    }

    fn lap(&mut self, now: u32) {
        if self.lap_count == MAX_LAPS {
            self.laps.copy_within(1.., 0);
            self.lap_count -= 1;
            self.lap_base += 1;
        }
        self.laps[self.lap_count] = self.elapsed_ms(now);
        self.lap_count += 1;
    }

    fn reset(&mut self) {
        self.banked_ms = 0;
        self.started_ms = None;
        self.lap_count = 0;
        self.lap_base = 0;
    }
}

impl App for Stopwatch {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.last_drawn.0 = u32::MAX;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        if ctx.input.just_pressed(Button::L) {
            match self.started_ms {
                None => self.started_ms = Some(now),
                Some(t) => {
                    self.banked_ms = self.banked_ms.wrapping_add(now.wrapping_sub(t));
                    self.started_ms = None;
                }
            }
            audio::play(Sound::Tick);
        }
        if ctx.input.just_pressed(Button::K) {
            if self.started_ms.is_some() {
                self.lap(now);
            } else {
                self.reset();
            }
            audio::play(Sound::Tick);
        }

        // Tenths are enough to read; redraw only when they change.
        let running = self.started_ms.is_some();
        let elapsed = self.elapsed_ms(now);
        let key = (elapsed / 100, running, self.lap_count);
        if key != self.last_drawn {
            self.last_drawn = key;
            draw(ctx.fb, self, elapsed, running);
        }
        Transition::Stay
    }
}

/// `MM:SS.t` under an hour, else `H:MM:SS`.
fn clock(ms: u32) -> StrBuf<12> {
    let secs = ms / 1000;
    if secs < 3600 {
        format(format_args!("{:02}:{:02}.{}", secs / 60, secs % 60, (ms / 100) % 10))
    } else {
        format(format_args!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60))
    }
}

fn draw(fb: &mut Framebuffer, s: &Stopwatch, elapsed: u32, running: bool) {
    theme::screen(fb, "Stopwatch", if running { "Running" } else { "Stopped" });
    let color = if running {
        theme::ACCENT
    } else if elapsed == 0 {
        theme::MUTED
    } else {
        theme::TEXT
    };
    fb.draw_text_centered(30, clock(elapsed).as_str(), color, None, 3);

    // The newest laps first: number, lap time, total at the lap.
    let mut y = 60;
    let first = s.lap_count.saturating_sub(SHOWN_LAPS);
    for i in (first..s.lap_count).rev() {
        let total = s.laps[i];
        let split = total.wrapping_sub(if i == 0 { 0 } else { s.laps[i - 1] });
        let number: StrBuf<4> = format(format_args!("{:>2}", s.lap_base + i + 1));
        fb.draw_text(8, y, number.as_str(), theme::MUTED, None);
        fb.draw_text(30, y, clock(split).as_str(), theme::TEXT, None);
        fb.draw_text_right(WIDTH - 8, y, clock(total).as_str(), theme::MUTED, None);
        y += CELL_HEIGHT + 2;
    }

    let hint = match (running, elapsed) {
        (true, _) => "L stop   K lap   J back",
        (false, 0) => "L start   J back",
        (false, _) => "L resume   K reset   J back",
    };
    theme::footer(fb, hint);
}

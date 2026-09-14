//! Test patterns for checking orientation, colour order and panel edges.

use hoot_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

const PATTERNS: u8 = 4;

pub const INFO: AppInfo = AppInfo { name: "Display test", group: Group::Developer, needs_network: false };

pub struct DisplayTest {
    pattern: u8,
}

impl DisplayTest {
    pub fn new() -> Self {
        Self { pattern: 0 }
    }
}

fn corner_labels(fb: &mut Framebuffer) {
    let bg = Some(Rgb565::BLACK);
    fb.draw_text(1, 1, "TL", Rgb565::WHITE, bg);
    fb.draw_text_right(WIDTH - 1, 1, "TR", Rgb565::WHITE, bg);
    fb.draw_text(1, HEIGHT - 9, "BL", Rgb565::WHITE, bg);
    fb.draw_text_right(WIDTH - 1, HEIGHT - 9, "BR", Rgb565::WHITE, bg);
}

/// Eight vertical bars in the classic order, plus corner labels.
fn bars(fb: &mut Framebuffer) {
    const COLORS: [Rgb565; 8] = [
        Rgb565::WHITE,
        Rgb565::YELLOW,
        Rgb565::CYAN,
        Rgb565::GREEN,
        Rgb565::MAGENTA,
        Rgb565::RED,
        Rgb565::BLUE,
        Rgb565::BLACK,
    ];
    let w = WIDTH / COLORS.len() as i32;
    for (i, c) in COLORS.iter().enumerate() {
        fb.fill_rect(i as i32 * w, 0, w, HEIGHT, *c);
    }
    corner_labels(fb);
    fb.draw_text(40, 60, "R", Rgb565::WHITE, Some(Rgb565::RED));
    fb.draw_text(60, 60, "G", Rgb565::BLACK, Some(Rgb565::GREEN));
    fb.draw_text(80, 60, "B", Rgb565::WHITE, Some(Rgb565::BLUE));
}

/// Red, green, blue and grey ramps from left to right.
fn gradients(fb: &mut Framebuffer) {
    let band = HEIGHT / 4;
    for x in 0..WIDTH {
        let v = (x * 255 / (WIDTH - 1)) as u8;
        fb.vline(x, 0, band, Rgb565::new(v, 0, 0));
        fb.vline(x, band, band, Rgb565::new(0, v, 0));
        fb.vline(x, band * 2, band, Rgb565::new(0, 0, v));
        fb.vline(x, band * 3, band, Rgb565::new(v, v, v));
    }
}

/// One-pixel border, a 16-pixel grid and a centre cross. Shows cropping or offset.
fn grid(fb: &mut Framebuffer) {
    fb.clear(Rgb565::BLACK);
    for x in (0..WIDTH).step_by(16) {
        fb.vline(x, 0, HEIGHT, theme::MUTED);
    }
    for y in (0..HEIGHT).step_by(16) {
        fb.hline(0, y, WIDTH, theme::MUTED);
    }
    fb.draw_rect(0, 0, WIDTH, HEIGHT, Rgb565::WHITE);
    fb.hline(WIDTH / 2 - 8, HEIGHT / 2, 17, theme::ACCENT);
    fb.vline(WIDTH / 2, HEIGHT / 2 - 8, 17, theme::ACCENT);
    corner_labels(fb);
}

impl App for DisplayTest {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.pattern = 0;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let next = ctx.input.just_pressed_mask() & !(1 << Button::J as u8);
        if next != 0 {
            self.pattern = (self.pattern + 1) % PATTERNS;
        }

        let fb = &mut *ctx.fb;
        match self.pattern {
            0 => bars(fb),
            1 => gradients(fb),
            2 => grid(fb),
            _ => fb.clear(Rgb565::WHITE),
        }
        if self.pattern != 3 {
            let label: StrBuf<24> =
                format(format_args!("{}/{} any key: next", self.pattern + 1, PATTERNS));
            fb.draw_text_centered(HEIGHT - 20, label.as_str(), Rgb565::WHITE, Some(Rgb565::BLACK), 1);
        }
        Transition::Stay
    }
}

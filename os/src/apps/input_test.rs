//! Shows all eight buttons in their physical layout and lights them when held.
//! Hold J for one second to leave, so J itself can be tested too.

use hoot_gfx::Framebuffer;

use crate::apps::{App, AppInfo, Group, Ctx, Transition};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

const KEY: i32 = 18;
const EXIT_HOLD_MS: u32 = 1000;

pub const INFO: AppInfo = AppInfo { name: "Input test", group: Group::Developer, needs_network: false };

pub struct InputTest {
    presses: u32,
    hold_started: Option<u32>,
}

impl InputTest {
    pub fn new() -> Self {
        Self { presses: 0, hold_started: None }
    }
}

/// Top-left corner of each key box. WASD sits left, IJKL sits right.
fn key_pos(b: Button) -> (i32, i32) {
    match b {
        Button::W => (36, 34),
        Button::A => (16, 54),
        Button::S => (36, 54),
        Button::D => (56, 54),
        Button::I => (106, 34),
        Button::J => (86, 54),
        Button::K => (106, 54),
        Button::L => (126, 54),
    }
}

fn draw_key(fb: &mut Framebuffer, b: Button, held: bool) {
    let (x, y) = key_pos(b);
    if held {
        fb.fill_rect(x, y, KEY, KEY, theme::ACCENT);
        fb.draw_text(x + 7, y + 6, b.label(), theme::BG, None);
    } else {
        fb.draw_rect(x, y, KEY, KEY, theme::MUTED);
        fb.draw_text(x + 7, y + 6, b.label(), theme::TEXT, None);
    }
}

impl App for InputTest {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.presses = 0;
        self.hold_started = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let input = ctx.input;
        self.presses += input.just_pressed_mask().count_ones();

        let mut hold_progress = 0u8;
        if input.held(Button::J) {
            let started = *self.hold_started.get_or_insert(ctx.now_ms);
            let held_for = ctx.now_ms.wrapping_sub(started);
            if held_for >= EXIT_HOLD_MS {
                return Transition::Exit;
            }
            hold_progress = (held_for * 255 / EXIT_HOLD_MS) as u8;
        } else {
            self.hold_started = None;
        }

        let count: StrBuf<16> = format(format_args!("presses: {}", self.presses));
        let fb = &mut *ctx.fb;
        theme::screen(fb, "Input test", "");
        for b in Button::ALL {
            draw_key(fb, b, input.held(b));
        }
        fb.draw_text(4, 88, count.as_str(), theme::TEXT, None);
        if hold_progress > 0 {
            theme::meter(fb, 4, 100, 152, 6, hold_progress, theme::WARN);
        }
        theme::footer(fb, "hold J to go back");
        Transition::Stay
    }
}

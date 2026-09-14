//! Adjust the two white LEDs and the backlight.

use sprig_gfx::Framebuffer;

use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "LEDs & backlight", group: Group::Developer, needs_network: false };

pub struct Leds;

const STEP: i16 = 8;
/// Never let the backlight go fully dark, or the menu becomes invisible.
const BACKLIGHT_MIN: u8 = 8;

fn row(fb: &mut Framebuffer, y: i32, label: &str, keys: &str, level: u8) {
    let value: StrBuf<4> = format(format_args!("{}", level));
    fb.draw_text(4, y, label, theme::TEXT, None);
    fb.draw_text_right(156, y, keys, theme::MUTED, None);
    theme::meter(fb, 4, y + 10, 120, 8, level, theme::ACCENT);
    fb.draw_text(130, y + 10, value.as_str(), theme::TEXT, None);
}

impl App for Leds {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }

        let input = ctx.input;
        let hw = &mut *ctx.hw;
        if input.repeat(Button::W) {
            hw.led_left.adjust(STEP, 0);
        }
        if input.repeat(Button::S) {
            hw.led_left.adjust(-STEP, 0);
        }
        if input.repeat(Button::I) {
            hw.led_right.adjust(STEP, 0);
        }
        if input.repeat(Button::K) {
            hw.led_right.adjust(-STEP, 0);
        }
        if input.repeat(Button::D) {
            hw.backlight.adjust(STEP, BACKLIGHT_MIN);
        }
        if input.repeat(Button::A) {
            hw.backlight.adjust(-STEP, BACKLIGHT_MIN);
        }

        let left = hw.led_left.level();
        let right = hw.led_right.level();
        let backlight = hw.backlight.level();

        let fb = &mut *ctx.fb;
        theme::screen(fb, "LEDs & backlight", "");
        row(fb, theme::CONTENT_Y, "Left LED", "W/S", left);
        row(fb, theme::CONTENT_Y + 26, "Right LED", "I/K", right);
        row(fb, theme::CONTENT_Y + 52, "Backlight", "A/D", backlight);
        theme::footer(fb, "J back");
        Transition::Stay
    }
}

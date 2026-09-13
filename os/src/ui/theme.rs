//! Colours and layout helpers shared by every screen.

use sprig_gfx::{CELL_HEIGHT, Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::net::NetState;

pub const BG: Rgb565 = Rgb565::hex(0x0E1216);
pub const BAR: Rgb565 = Rgb565::hex(0x1B2430);
pub const ACCENT: Rgb565 = Rgb565::hex(0x3DDC84);
pub const ACCENT_DARK: Rgb565 = Rgb565::hex(0x0B3D25);
pub const TEXT: Rgb565 = Rgb565::hex(0xE8EEF4);
pub const MUTED: Rgb565 = Rgb565::hex(0x7D8A99);
pub const WARN: Rgb565 = Rgb565::hex(0xF2C14E);

pub const TITLE_H: i32 = 12;
/// First free row below the title bar.
pub const CONTENT_Y: i32 = TITLE_H + 6;
/// Row for the one-line hint at the bottom.
pub const FOOTER_Y: i32 = HEIGHT - CELL_HEIGHT - 2;

/// Clear the frame and draw the title bar.
pub fn screen(fb: &mut Framebuffer, title: &str, right: &str) {
    fb.clear(BG);
    fb.fill_rect(0, 0, WIDTH, TITLE_H, BAR);
    fb.hline(0, TITLE_H, WIDTH, ACCENT_DARK);
    fb.draw_text(3, 2, title, ACCENT, None);
    if !right.is_empty() {
        fb.draw_text_right(WIDTH - 3, 2, right, MUTED, None);
    }
}

/// One-line hint at the bottom of the screen.
pub fn footer(fb: &mut Framebuffer, hint: &str) {
    fb.draw_text(3, FOOTER_Y, hint, MUTED, None);
}

/// A horizontal bar showing `value` out of 255.
pub fn meter(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, value: u8, color: Rgb565) {
    fb.draw_rect(x, y, w, h, MUTED);
    let inner = (w - 2) * value as i32 / 255;
    fb.fill_rect(x + 1, y + 1, inner, h - 2, color);
}

/// Colour for a network state: green when up, amber when in trouble.
pub fn net_color(state: NetState) -> Rgb565 {
    match state {
        NetState::NoRadio => MUTED,
        NetState::Up(_) => ACCENT,
        NetState::JoinFailed | NetState::Lost => WARN,
        _ => TEXT,
    }
}

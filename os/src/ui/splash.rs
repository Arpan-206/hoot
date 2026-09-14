//! Boot screen: the owl on its perch, the name and the version.

use hoot_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::VERSION;
use crate::ui::theme;

/// The owl, 16 by 16, as layers drawn in order: body, eyes, pupils, beak.
/// Rows are bits, most significant on the left.
const OWL: i32 = 16;
const BODY: [u16; OWL as usize] = [
    0b0100_0000_0000_0010,
    0b0110_0000_0000_0110,
    0b0111_1111_1111_1110,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0011_1111_1111_1100,
    0b0001_1000_0001_1000,
    0b0011_1000_0001_1100,
];
const EYES: [u16; OWL as usize] = [
    0, 0, 0, 0,
    0b0001_1100_0011_1000,
    0b0011_1110_0111_1100,
    0b0011_1110_0111_1100,
    0b0011_1110_0111_1100,
    0b0001_1100_0011_1000,
    0, 0, 0, 0, 0, 0, 0,
];
const PUPILS: [u16; OWL as usize] = [
    0, 0, 0, 0, 0, 0,
    0b0000_1100_0001_1000,
    0b0000_1100_0001_1000,
    0, 0, 0, 0, 0, 0, 0, 0,
];
const BEAK: [u16; OWL as usize] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0,
    0b0000_0001_1000_0000,
    0b0000_0001_1000_0000,
    0, 0, 0, 0, 0,
];

fn layer(fb: &mut Framebuffer, x: i32, y: i32, rows: &[u16; OWL as usize], color: Rgb565, scale: i32) {
    let mut bytes = [0u8; OWL as usize * 2];
    for (i, row) in rows.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&row.to_be_bytes());
    }
    fb.draw_bitmap_scaled(x, y, OWL, OWL, &bytes, color, scale);
}

/// The Hoot owl with its top-left corner at `x`, `y`.
pub fn draw_owl(fb: &mut Framebuffer, x: i32, y: i32, scale: i32) {
    layer(fb, x, y, &BODY, theme::ACCENT, scale);
    layer(fb, x, y, &EYES, theme::TEXT, scale);
    layer(fb, x, y, &PUPILS, theme::BG, scale);
    layer(fb, x, y, &BEAK, theme::WARN, scale);
}

pub fn draw(fb: &mut Framebuffer) {
    fb.clear(theme::BG);
    let scale = 3;
    let size = OWL * scale;
    let x = (WIDTH - size) / 2;
    let y = 12;
    draw_owl(fb, x, y, scale);
    // The perch.
    fb.fill_rect(x - 14, y + size, size + 28, 2, theme::MUTED);
    fb.draw_text_centered(y + size + 10, "Hoot", theme::TEXT, None, 2);
    fb.draw_text_centered(y + size + 28, VERSION, theme::MUTED, None, 1);
    fb.draw_text_centered(HEIGHT - 12, "booting...", theme::MUTED, None, 1);
}

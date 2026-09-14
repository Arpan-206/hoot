//! The owl sprite shared by the splash and by Hoot itself: a barn owl,
//! 16 by 16, drawn as 1-bit layers at any integer scale.

use hoot_gfx::{Framebuffer, Rgb565};

use crate::ui::theme;

pub const SIZE: i32 = 16;

const FEATHER: Rgb565 = Rgb565::hex(0xC98B45);
const WING: Rgb565 = Rgb565::hex(0x8C5A2E);
const BEAK: Rgb565 = Rgb565::hex(0x7A4A2A);

/// Rows are bits, most significant on the left.
const BODY: [u16; SIZE as usize] = [
    0b0000_0000_0000_0000,
    0b0000_1111_1111_0000,
    0b0011_1111_1111_1100,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b1111_1111_1111_1111,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0011_1111_1111_1100,
    0b0001_1111_1111_1000,
    0b0000_0110_0110_0000,
    0b0000_1110_0111_0000,
];
/// The heart-shaped face.
const FACE: [u16; SIZE as usize] = [
    0b0000_0000_0000_0000,
    0b0000_0000_0000_0000,
    0b0000_1110_0111_0000,
    0b0001_1111_1111_1000,
    0b0001_1111_1111_1000,
    0b0001_1111_1111_1000,
    0b0000_1111_1111_0000,
    0b0000_0111_1110_0000,
    0b0000_0011_1100_0000,
    0b0000_0001_1000_0000,
    0, 0, 0, 0, 0, 0,
];
const EYES: [u16; SIZE as usize] = [
    0, 0, 0, 0,
    0b0000_1100_0011_0000,
    0b0000_1100_0011_0000,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
/// Eyes shut: one line where the lower row of each eye was.
const EYES_SHUT: [u16; SIZE as usize] = [
    0, 0, 0, 0, 0,
    0b0000_1100_0011_0000,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const BEAK_ROWS: [u16; SIZE as usize] = [
    0, 0, 0, 0, 0, 0,
    0b0000_0001_1000_0000,
    0b0000_0001_1000_0000,
    0, 0, 0, 0, 0, 0, 0, 0,
];
/// Round spectacles for the wise owl.
const GLASSES: [u16; SIZE as usize] = [
    0, 0, 0,
    0b0001_1110_0111_1000,
    0b0001_0011_1100_1000,
    0b0001_0010_0100_1000,
    0b0001_1110_0111_1000,
    0, 0, 0, 0, 0, 0, 0, 0, 0,
];
/// Wing edges, a shade darker than the body.
const WINGS: [u16; SIZE as usize] = [
    0, 0, 0, 0, 0,
    0b1100_0000_0000_0011,
    0b1100_0000_0000_0011,
    0b1110_0000_0000_0111,
    0b1110_0000_0000_0111,
    0b0110_0000_0000_0110,
    0b0110_0000_0000_0110,
    0b0011_0000_0000_1100,
    0b0001_0000_0000_1000,
    0, 0, 0,
];

fn layer(fb: &mut Framebuffer, x: i32, y: i32, rows: &[u16; SIZE as usize], color: Rgb565, scale: i32) {
    let mut bytes = [0u8; SIZE as usize * 2];
    for (i, row) in rows.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&row.to_be_bytes());
    }
    fb.draw_bitmap_scaled(x, y, SIZE, SIZE, &bytes, color, scale);
}

/// The whole owl with its top-left corner at `x`, `y`.
pub fn draw(fb: &mut Framebuffer, x: i32, y: i32, scale: i32, eyes_shut: bool, glasses: bool) {
    layer(fb, x, y, &BODY, FEATHER, scale);
    layer(fb, x, y, &WINGS, WING, scale);
    layer(fb, x, y, &FACE, theme::TEXT, scale);
    draw_eyes(fb, x, y, scale, eyes_shut);
    layer(fb, x, y, &BEAK_ROWS, BEAK, scale);
    if glasses {
        layer(fb, x, y, &GLASSES, BEAK, scale);
    }
}

/// Only the eyes, for blinking over an owl already drawn.
pub fn draw_eyes(fb: &mut Framebuffer, x: i32, y: i32, scale: i32, shut: bool) {
    if shut {
        layer(fb, x, y, &EYES, theme::TEXT, scale);
        layer(fb, x, y, &EYES_SHUT, theme::BG, scale);
    } else {
        layer(fb, x, y, &EYES, theme::BG, scale);
    }
}

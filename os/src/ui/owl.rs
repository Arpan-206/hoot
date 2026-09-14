//! The owl sprite shared by the splash and by Hoot itself: a barn owl,
//! 16 by 16, drawn as 1-bit layers at any integer scale. Outfits are
//! 20 rows tall: four rows of hat room above the head.

use hoot_gfx::{Framebuffer, Rgb565};
use hoot_proto::pet::Outfit;

use crate::ui::theme;

pub const SIZE: i32 = 16;
/// Rows above the sprite that a hat may use.
pub const HAT_ROOM: i32 = 4;

const FEATHER: Rgb565 = Rgb565::hex(0xC98B45);
const WING: Rgb565 = Rgb565::hex(0x8C5A2E);
const BEAK: Rgb565 = Rgb565::hex(0x7A4A2A);
const SCARF: Rgb565 = Rgb565::hex(0xC0392B);
const BOW: Rgb565 = Rgb565::hex(0xE85D9A);
const HEADPHONES: Rgb565 = Rgb565::hex(0x2E3A59);
const SHELL: Rgb565 = Rgb565::hex(0xEADCC0);
const SPOT: Rgb565 = Rgb565::hex(0xC9B58F);

/// How the owl looks this frame.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    pub eyes_shut: bool,
    pub glasses: bool,
    /// -1 looks left, 1 looks right.
    pub glance: i8,
    pub flap: bool,
    pub outfit: Outfit,
}

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
/// Wings lifted, tips out past the body.
const WINGS_UP: [u16; SIZE as usize] = [
    0, 0, 0,
    0b1100_0000_0000_0011,
    0b1110_0000_0000_0111,
    0b1110_0000_0000_0111,
    0b0110_0000_0000_0110,
    0b0110_0000_0000_0110,
    0b0011_0000_0000_1100,
    0b0001_0000_0000_1000,
    0, 0, 0, 0, 0, 0,
];

// Outfits: 20 rows, the first four above the head.
const ROWS: usize = (SIZE + HAT_ROOM) as usize;
const SCARF_ROWS: [u16; ROWS] = [
    0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0b0111_1111_1111_1110,
    0b0000_0000_0000_1110,
    0b0000_0000_0000_0110,
    0, 0, 0,
];
const BOW_ROWS: [u16; ROWS] = [
    0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0b0000_0110_0110_0000,
    0b0000_0001_1000_0000,
    0, 0, 0, 0,
];
const PARTY_HAT_ROWS: [u16; ROWS] = [
    0b0000_0001_1000_0000,
    0b0000_0001_1000_0000,
    0b0000_0011_1100_0000,
    0b0000_0111_1110_0000,
    0b0000_1111_1111_0000,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const HEADPHONES_ROWS: [u16; ROWS] = [
    0, 0, 0, 0,
    0b0001_1111_1111_1000,
    0b0010_0000_0000_0100,
    0b0100_0000_0000_0010,
    0b1100_0000_0000_0011,
    0b1100_0000_0000_0011,
    0b1100_0000_0000_0011,
    0b1100_0000_0000_0011,
    0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const CROWN_ROWS: [u16; ROWS] = [
    0, 0,
    0b0000_1001_1001_0000,
    0b0000_1111_1111_0000,
    0b0000_1111_1111_0000,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// The egg Hoot hatches from, and two crack stages.
const EGG: [u16; SIZE as usize] = [
    0b0000_0011_1100_0000,
    0b0000_1111_1111_0000,
    0b0001_1111_1111_1000,
    0b0011_1111_1111_1100,
    0b0011_1111_1111_1100,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0111_1111_1111_1110,
    0b0011_1111_1111_1100,
    0b0011_1111_1111_1100,
    0b0001_1111_1111_1000,
    0b0000_1111_1111_0000,
    0b0000_0011_1100_0000,
    0,
];
const EGG_SPOTS: [u16; SIZE as usize] = [
    0, 0, 0,
    0b0000_0100_0000_0000,
    0, 0,
    0b0000_0000_0010_0000,
    0, 0,
    0b0000_1000_0000_0000,
    0, 0,
    0b0000_0000_0100_0000,
    0, 0, 0,
];
const CRACK_1: [u16; SIZE as usize] = [
    0, 0, 0, 0, 0, 0,
    0b0000_0001_0000_0000,
    0b0000_0010_1000_0000,
    0b0000_0001_0000_0000,
    0, 0, 0, 0, 0, 0, 0,
];
const CRACK_2: [u16; SIZE as usize] = [
    0, 0, 0, 0, 0,
    0b0000_0010_0000_0000,
    0b0000_0001_0000_0000,
    0b0000_0010_1100_0000,
    0b0000_0100_0010_0000,
    0b0000_0010_0100_0000,
    0b0000_0001_0000_0000,
    0, 0, 0, 0, 0,
];

fn layer<const N: usize>(fb: &mut Framebuffer, x: i32, y: i32, rows: &[u16; N], color: Rgb565, scale: i32) {
    let mut bytes = [0u8; ROWS * 2];
    for (i, row) in rows.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&row.to_be_bytes());
    }
    fb.draw_bitmap_scaled(x, y, SIZE, N as i32, &bytes[..N * 2], color, scale);
}

/// The whole owl with its top-left corner at `x`, `y`. Hats extend above.
pub fn draw(fb: &mut Framebuffer, x: i32, y: i32, scale: i32, look: Look) {
    layer(fb, x, y, &BODY, FEATHER, scale);
    layer(fb, x, y, if look.flap { &WINGS_UP } else { &WINGS }, WING, scale);
    layer(fb, x, y, &FACE, theme::TEXT, scale);
    if look.eyes_shut {
        draw_eyes(fb, x, y, scale, true);
    } else {
        layer(fb, x + look.glance as i32 * scale, y, &EYES, theme::BG, scale);
    }
    layer(fb, x, y, &BEAK_ROWS, BEAK, scale);
    if look.glasses {
        layer(fb, x, y, &GLASSES, BEAK, scale);
    }
    let top = y - HAT_ROOM * scale;
    match look.outfit {
        Outfit::None => {}
        Outfit::Scarf => layer(fb, x, top, &SCARF_ROWS, SCARF, scale),
        Outfit::Bow => layer(fb, x, top, &BOW_ROWS, BOW, scale),
        Outfit::PartyHat => layer(fb, x, top, &PARTY_HAT_ROWS, theme::ACCENT, scale),
        Outfit::Headphones => layer(fb, x, top, &HEADPHONES_ROWS, HEADPHONES, scale),
        Outfit::Crown => layer(fb, x, top, &CROWN_ROWS, theme::ACCENT, scale),
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

/// The egg. `crack` 0 is whole, 1 and 2 are the crack stages.
pub fn draw_egg(fb: &mut Framebuffer, x: i32, y: i32, scale: i32, crack: u8) {
    layer(fb, x, y, &EGG, SHELL, scale);
    layer(fb, x, y, &EGG_SPOTS, SPOT, scale);
    match crack {
        0 => {}
        1 => layer(fb, x, y, &CRACK_1, theme::BG, scale),
        _ => layer(fb, x, y, &CRACK_2, theme::BG, scale),
    }
}

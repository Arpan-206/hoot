//! Boot screen: a barn owl on a branch under a crescent moon, the name
//! and the version.

use hoot_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::VERSION;
use crate::ui::theme;

/// Owl colours. The face and eyes use the theme's cream and background.
const FEATHER: Rgb565 = Rgb565::hex(0xC98B45);
const WING: Rgb565 = Rgb565::hex(0x8C5A2E);
const BEAK: Rgb565 = Rgb565::hex(0x7A4A2A);
const BRANCH: Rgb565 = Rgb565::hex(0x5C4632);

/// The owl, 16 by 16, as layers drawn in order. Rows are bits, most
/// significant on the left.
const OWL: i32 = 16;
const BODY: [u16; OWL as usize] = [
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
const FACE: [u16; OWL as usize] = [
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
const EYES: [u16; OWL as usize] = [
    0, 0, 0, 0,
    0b0000_1100_0011_0000,
    0b0000_1100_0011_0000,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const BEAK_ROWS: [u16; OWL as usize] = [
    0, 0, 0, 0, 0, 0,
    0b0000_0001_1000_0000,
    0b0000_0001_1000_0000,
    0, 0, 0, 0, 0, 0, 0, 0,
];
/// Wing edges, a shade darker than the body.
const WINGS: [u16; OWL as usize] = [
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

fn layer(fb: &mut Framebuffer, x: i32, y: i32, rows: &[u16; OWL as usize], color: Rgb565, scale: i32) {
    let mut bytes = [0u8; OWL as usize * 2];
    for (i, row) in rows.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&row.to_be_bytes());
    }
    fb.draw_bitmap_scaled(x, y, OWL, OWL, &bytes, color, scale);
}

/// The Hoot owl with its top-left corner at `x`, `y`.
pub fn draw_owl(fb: &mut Framebuffer, x: i32, y: i32, scale: i32) {
    layer(fb, x, y, &BODY, FEATHER, scale);
    layer(fb, x, y, &WINGS, WING, scale);
    layer(fb, x, y, &FACE, theme::TEXT, scale);
    layer(fb, x, y, &EYES, theme::BG, scale);
    layer(fb, x, y, &BEAK_ROWS, BEAK, scale);
}

/// A crescent: a disc with a second disc taken out of its upper right.
fn moon(fb: &mut Framebuffer, cx: i32, cy: i32, r: i32) {
    let (bx, by, br) = (cx + r / 2, cy - r / 3, r - 1);
    for y in -r..=r {
        for x in -r..=r {
            let inside = x * x + y * y <= r * r;
            let bite = (x - (bx - cx)) * (x - (bx - cx)) + (y - (by - cy)) * (y - (by - cy)) <= br * br;
            if inside && !bite {
                fb.set(cx + x, cy + y, theme::ACCENT);
            }
        }
    }
}

pub fn draw(fb: &mut Framebuffer) {
    fb.clear(theme::BG);
    // A few stars and the moon.
    for &(x, y) in &[(14, 10), (38, 22), (129, 30), (147, 12), (22, 58), (140, 66)] {
        fb.set(x, y, theme::MUTED);
    }
    fb.set(147, 13, theme::MUTED);
    fb.set(146, 12, theme::MUTED);
    moon(fb, 132, 16, 8);

    let scale = 3;
    let size = OWL * scale;
    let x = (WIDTH - size) / 2;
    let y = 12;
    draw_owl(fb, x, y, scale);
    // The branch, with a twig.
    let branch_y = y + size - 2;
    fb.fill_rect(x - 18, branch_y, size + 36, 3, BRANCH);
    fb.fill_rect(x - 18, branch_y - 3, 4, 3, BRANCH);
    fb.fill_rect(x + size + 10, branch_y - 4, 3, 4, BRANCH);

    fb.draw_text_centered(y + size + 10, "Hoot", theme::TEXT, None, 2);
    fb.draw_text_centered(y + size + 28, VERSION, theme::MUTED, None, 1);
    fb.draw_text_centered(HEIGHT - 12, "booting...", theme::MUTED, None, 1);
}

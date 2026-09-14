//! Boot screen: the owl on a branch under a crescent moon, the name and
//! the version.

use hoot_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::VERSION;
use crate::ui::owl;
use crate::ui::theme;

const BRANCH: Rgb565 = Rgb565::hex(0x5C4632);

/// Where the splash puts the owl: top-left corner and scale.
const fn owl_origin() -> (i32, i32, i32) {
    let scale = 3;
    ((WIDTH - owl::SIZE * scale) / 2, 12, scale)
}

/// Shut or open the owl's eyes on the splash. Cheap: only the eyes redraw.
pub fn blink(fb: &mut Framebuffer, shut: bool) {
    let (x, y, scale) = owl_origin();
    owl::draw_eyes(fb, x, y, scale, shut);
}

/// A crescent: a disc with a second disc taken out of its upper right.
pub fn moon(fb: &mut Framebuffer, cx: i32, cy: i32, r: i32) {
    let (bx, by, br) = (r / 2, -r / 3, r - 1);
    for y in -r..=r {
        for x in -r..=r {
            let inside = x * x + y * y <= r * r;
            let bite = (x - bx) * (x - bx) + (y - by) * (y - by) <= br * br;
            if inside && !bite {
                fb.set(cx + x, cy + y, theme::ACCENT);
            }
        }
    }
}

pub fn draw(fb: &mut Framebuffer) {
    fb.clear(theme::BG);
    for &(x, y) in &[(14, 10), (38, 22), (129, 30), (147, 12), (22, 58), (140, 66), (147, 13), (146, 12)] {
        fb.set(x, y, theme::MUTED);
    }
    moon(fb, 132, 16, 8);

    let (x, y, scale) = owl_origin();
    let size = owl::SIZE * scale;
    owl::draw(fb, x, y, scale, false, false);
    let branch_y = y + size - 2;
    fb.fill_rect(x - 18, branch_y, size + 36, 3, BRANCH);
    fb.fill_rect(x - 18, branch_y - 3, 4, 3, BRANCH);
    fb.fill_rect(x + size + 10, branch_y - 4, 3, 4, BRANCH);

    fb.draw_text_centered(y + size + 10, "Hoot", theme::TEXT, None, 2);
    fb.draw_text_centered(y + size + 28, VERSION, theme::MUTED, None, 1);
    fb.draw_text_centered(HEIGHT - 12, "booting...", theme::MUTED, None, 1);
}

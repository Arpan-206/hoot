//! Boot screen.

use sprig_gfx::{Framebuffer, HEIGHT};

use crate::VERSION;
use crate::ui::theme;

pub fn draw(fb: &mut Framebuffer) {
    fb.clear(theme::BG);
    fb.draw_text_centered(40, "Sprig OS", theme::ACCENT, None, 2);
    fb.draw_text_centered(62, VERSION, theme::MUTED, None, 1);
    fb.draw_text_centered(HEIGHT - 22, "booting...", theme::MUTED, None, 1);
}

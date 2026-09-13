//! QR codes drawn straight into the framebuffer, without a heap.

use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
use sprig_gfx::{Framebuffer, Rgb565};

/// Largest code we draw. Version 5 is 37 modules a side.
const MAX_VERSION: Version = Version::new(5);
const BUF: usize = MAX_VERSION.buffer_len();

/// Draw `text` as a QR code with its top-left corner at (x, y), each module
/// `scale` pixels, inside a white quiet zone of `border` modules. Returns
/// the side length in pixels, or `None` if the text does not fit.
pub fn draw(fb: &mut Framebuffer, x: i32, y: i32, scale: i32, border: i32, text: &str) -> Option<i32> {
    let mut temp = [0u8; BUF];
    let mut out = [0u8; BUF];
    let qr = QrCode::encode_text(text, &mut temp, &mut out, QrCodeEcc::Low, Version::MIN, MAX_VERSION, None, false).ok()?;
    let n = qr.size();
    let side = (n + 2 * border) * scale;
    fb.fill_rect(x, y, side, side, Rgb565::WHITE);
    for my in 0..n {
        for mx in 0..n {
            if qr.get_module(mx, my) {
                fb.fill_rect(x + (mx + border) * scale, y + (my + border) * scale, scale, scale, Rgb565::BLACK);
            }
        }
    }
    Some(side)
}

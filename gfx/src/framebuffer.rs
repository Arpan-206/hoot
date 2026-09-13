use crate::color::Rgb565;
use crate::font::{self, CELL_HEIGHT, CELL_WIDTH, GLYPH_HEIGHT};

/// Display width in pixels.
pub const WIDTH: i32 = 160;
/// Display height in pixels.
pub const HEIGHT: i32 = 128;
/// Size of one frame in bytes (two bytes per pixel).
pub const BYTES: usize = (WIDTH * HEIGHT * 2) as usize;

/// A full 160x128 RGB565 frame.
///
/// Pixels are stored big-endian, which is the byte order the ST7735 expects.
/// A frame can therefore be sent over SPI with no conversion step.
#[repr(C, align(4))]
pub struct Framebuffer {
    data: [u8; BYTES],
    /// Set by every drawing call. The OS clears it when it sends a frame.
    dirty: bool,
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    /// An all-black, clean frame. `const` and all zeros, so a `static`
    /// framebuffer lands in `.bss` and costs no flash and no boot-time copy.
    /// Call `mark_dirty` if the first frame must be sent unconditionally.
    pub const fn new() -> Self {
        Self { data: [0; BYTES], dirty: false }
    }

    /// Raw bytes, ready to stream to the display.
    pub fn as_bytes(&self) -> &[u8; BYTES] {
        &self.data
    }

    /// Replace the whole frame with raw big-endian RGB565 bytes.
    pub fn load_raw(&mut self, bytes: &[u8; BYTES]) {
        self.data.copy_from_slice(bytes);
        self.dirty = true;
    }

    /// True if anything was drawn since the last `take_dirty`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Read and clear the dirty flag. The OS calls this once per frame.
    pub fn take_dirty(&mut self) -> bool {
        core::mem::replace(&mut self.dirty, false)
    }

    /// Force the next frame to be sent even if nothing was drawn.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Fill the whole frame with one colour.
    pub fn clear(&mut self, c: Rgb565) {
        self.dirty = true;
        let [hi, lo] = c.to_be_bytes();
        if hi == lo {
            self.data.fill(hi);
            return;
        }
        for px in self.data.chunks_exact_mut(2) {
            px[0] = hi;
            px[1] = lo;
        }
    }

    /// Set one pixel. Coordinates outside the frame are ignored.
    #[inline(always)]
    pub fn set(&mut self, x: i32, y: i32, c: Rgb565) {
        if (0..WIDTH).contains(&x) && (0..HEIGHT).contains(&y) {
            self.dirty = true;
            let i = ((y * WIDTH + x) * 2) as usize;
            let [hi, lo] = c.to_be_bytes();
            self.data[i] = hi;
            self.data[i + 1] = lo;
        }
    }

    /// Read one pixel, or `None` when the coordinates are outside the frame.
    pub fn get(&self, x: i32, y: i32) -> Option<Rgb565> {
        if (0..WIDTH).contains(&x) && (0..HEIGHT).contains(&y) {
            let i = ((y * WIDTH + x) * 2) as usize;
            Some(Rgb565(u16::from_be_bytes([self.data[i], self.data[i + 1]])))
        } else {
            None
        }
    }

    /// Fill a rectangle. The rectangle is clipped to the frame.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb565) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = x.saturating_add(w).min(WIDTH);
        let y1 = y.saturating_add(h).min(HEIGHT);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        self.dirty = true;
        let [hi, lo] = c.to_be_bytes();
        for row in y0..y1 {
            let start = ((row * WIDTH + x0) * 2) as usize;
            let end = ((row * WIDTH + x1) * 2) as usize;
            for px in self.data[start..end].chunks_exact_mut(2) {
                px[0] = hi;
                px[1] = lo;
            }
        }
    }

    /// Draw a one-pixel rectangle outline.
    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb565) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.hline(x, y, w, c);
        self.hline(x, y + h - 1, w, c);
        self.vline(x, y, h, c);
        self.vline(x + w - 1, y, h, c);
    }

    /// Draw a horizontal line of `w` pixels starting at (x, y).
    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: Rgb565) {
        self.fill_rect(x, y, w, 1, c);
    }

    /// Draw a vertical line of `h` pixels starting at (x, y).
    pub fn vline(&mut self, x: i32, y: i32, h: i32, c: Rgb565) {
        self.fill_rect(x, y, 1, h, c);
    }

    /// Draw one character at integer `scale`. Returns the horizontal advance.
    ///
    /// `bg` paints the whole 6x8 cell first; `None` leaves the background as is.
    pub fn draw_char(
        &mut self,
        x: i32,
        y: i32,
        ch: char,
        fg: Rgb565,
        bg: Option<Rgb565>,
        scale: i32,
    ) -> i32 {
        let scale = scale.max(1);
        if let Some(bg) = bg {
            self.fill_rect(x, y, CELL_WIDTH * scale, CELL_HEIGHT * scale, bg);
        }
        let glyph = font::glyph(ch);
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..GLYPH_HEIGHT {
                if bits & (1 << row) != 0 {
                    let px = x + col as i32 * scale;
                    let py = y + row * scale;
                    if scale == 1 {
                        self.set(px, py, fg);
                    } else {
                        self.fill_rect(px, py, scale, scale, fg);
                    }
                }
            }
        }
        CELL_WIDTH * scale
    }

    /// Draw a string at scale 1. Returns the width drawn in pixels.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, fg: Rgb565, bg: Option<Rgb565>) -> i32 {
        self.draw_text_scaled(x, y, text, fg, bg, 1)
    }

    /// Draw a string at an integer scale. Returns the width drawn in pixels.
    pub fn draw_text_scaled(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        fg: Rgb565,
        bg: Option<Rgb565>,
        scale: i32,
    ) -> i32 {
        let mut cx = x;
        for ch in text.chars() {
            cx += self.draw_char(cx, y, ch, fg, bg, scale);
        }
        cx - x
    }

    /// Draw a string centred horizontally. Returns the x position used.
    pub fn draw_text_centered(
        &mut self,
        y: i32,
        text: &str,
        fg: Rgb565,
        bg: Option<Rgb565>,
        scale: i32,
    ) -> i32 {
        let x = (WIDTH - text_width(text, scale)) / 2;
        self.draw_text_scaled(x, y, text, fg, bg, scale);
        x
    }

    /// Draw a string so that it ends at `right`. Returns the x position used.
    pub fn draw_text_right(
        &mut self,
        right: i32,
        y: i32,
        text: &str,
        fg: Rgb565,
        bg: Option<Rgb565>,
    ) -> i32 {
        let x = right - text_width(text, 1);
        self.draw_text(x, y, text, fg, bg);
        x
    }
}

/// Width in pixels that `text` occupies at `scale`.
pub fn text_width(text: &str, scale: i32) -> i32 {
    text.chars().count() as i32 * CELL_WIDTH * scale.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fb() -> Box<Framebuffer> {
        Box::new(Framebuffer::new())
    }

    #[test]
    fn dirty_flag_tracks_drawing() {
        let mut fb = fb();
        assert!(!fb.is_dirty(), "a new frame starts clean and all-zero");
        fb.mark_dirty();
        assert!(fb.take_dirty());
        assert!(!fb.take_dirty());
        fb.set(-1, -1, Rgb565::WHITE);
        assert!(!fb.is_dirty(), "clipped pixels do not dirty the frame");
        fb.set(1, 1, Rgb565::WHITE);
        assert!(fb.take_dirty());
        fb.fill_rect(200, 200, 5, 5, Rgb565::WHITE);
        assert!(!fb.is_dirty(), "fully clipped rects do not dirty the frame");
        fb.draw_text(0, 0, "x", Rgb565::WHITE, None);
        assert!(fb.take_dirty());
        let raw = [0x12u8; BYTES];
        fb.load_raw(&raw);
        assert!(fb.take_dirty());
        assert_eq!(fb.as_bytes()[7], 0x12);
    }

    #[test]
    fn set_and_get_round_trip() {
        let mut fb = fb();
        fb.set(3, 4, Rgb565::CYAN);
        assert_eq!(fb.get(3, 4), Some(Rgb565::CYAN));
        assert_eq!(fb.get(4, 3), Some(Rgb565::BLACK));
    }

    #[test]
    fn out_of_range_pixels_are_ignored() {
        let mut fb = fb();
        fb.set(-1, 0, Rgb565::WHITE);
        fb.set(WIDTH, 0, Rgb565::WHITE);
        fb.set(0, HEIGHT, Rgb565::WHITE);
        assert!(fb.as_bytes().iter().all(|&b| b == 0));
        assert_eq!(fb.get(WIDTH, 0), None);
    }

    #[test]
    fn bytes_are_big_endian() {
        let mut fb = fb();
        fb.clear(Rgb565::RED);
        assert_eq!(&fb.as_bytes()[..4], &[0xF8, 0x00, 0xF8, 0x00]);
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut fb = fb();
        fb.fill_rect(-10, -10, 20, 20, Rgb565::WHITE);
        assert_eq!(fb.get(0, 0), Some(Rgb565::WHITE));
        assert_eq!(fb.get(9, 9), Some(Rgb565::WHITE));
        assert_eq!(fb.get(10, 9), Some(Rgb565::BLACK));
        assert_eq!(fb.get(9, 10), Some(Rgb565::BLACK));
        fb.fill_rect(150, 120, 100, 100, Rgb565::GREEN);
        assert_eq!(fb.get(WIDTH - 1, HEIGHT - 1), Some(Rgb565::GREEN));
        assert_eq!(fb.get(149, 119), Some(Rgb565::BLACK));
    }

    #[test]
    fn empty_and_negative_rects_draw_nothing() {
        let mut fb = fb();
        fb.fill_rect(10, 10, 0, 5, Rgb565::WHITE);
        fb.fill_rect(10, 10, -5, 5, Rgb565::WHITE);
        fb.draw_rect(10, 10, 0, 0, Rgb565::WHITE);
        assert!(fb.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn draw_rect_only_touches_the_outline() {
        let mut fb = fb();
        fb.draw_rect(10, 10, 5, 4, Rgb565::WHITE);
        assert_eq!(fb.get(10, 10), Some(Rgb565::WHITE));
        assert_eq!(fb.get(14, 13), Some(Rgb565::WHITE));
        assert_eq!(fb.get(11, 11), Some(Rgb565::BLACK));
        assert_eq!(fb.get(15, 10), Some(Rgb565::BLACK));
    }

    #[test]
    fn text_advances_one_cell_per_character() {
        let mut fb = fb();
        assert_eq!(fb.draw_text(0, 0, "Sprig", Rgb565::WHITE, None), 5 * CELL_WIDTH);
        assert_eq!(text_width("Sprig", 2), 5 * CELL_WIDTH * 2);
        assert_eq!(fb.draw_char(0, 0, 'A', Rgb565::WHITE, None, 3), CELL_WIDTH * 3);
    }

    #[test]
    fn scaled_text_is_a_pixel_doubled_copy() {
        let mut small = fb();
        let mut big = fb();
        small.draw_text(0, 0, "Wg", Rgb565::WHITE, None);
        big.draw_text_scaled(0, 0, "Wg", Rgb565::WHITE, None, 2);
        for y in 0..(CELL_HEIGHT * 2) {
            for x in 0..(CELL_WIDTH * 4) {
                assert_eq!(big.get(x, y), small.get(x / 2, y / 2), "at ({x},{y})");
            }
        }
    }

    #[test]
    fn background_fills_the_full_cell() {
        let mut fb = fb();
        fb.draw_char(0, 0, ' ', Rgb565::WHITE, Some(Rgb565::BLUE), 1);
        assert_eq!(fb.get(CELL_WIDTH - 1, CELL_HEIGHT - 1), Some(Rgb565::BLUE));
        assert_eq!(fb.get(CELL_WIDTH, 0), Some(Rgb565::BLACK));
    }

    /// Prints the whole font as ASCII art. Run with
    /// `cargo host-test -- font_sheet --nocapture` and check it by eye.
    #[test]
    fn font_sheet() {
        let mut fb = fb();
        let rows = [
            "Sprig OS 0.1.0 !\"#$%&'()*+",
            ",-./0123456789:;<=>?@",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "[\\]^_`{|}~",
            "abcdefghijklmnopqrstuvwxyz",
        ];
        for (i, row) in rows.iter().enumerate() {
            fb.draw_text(0, i as i32 * CELL_HEIGHT, row, Rgb565::WHITE, None);
        }
        let mut out = String::new();
        for y in 0..(rows.len() as i32 * CELL_HEIGHT) {
            for x in 0..WIDTH {
                out.push(if fb.get(x, y) == Some(Rgb565::WHITE) { '#' } else { '.' });
            }
            out.push('\n');
        }
        println!("\n{out}");
    }
}

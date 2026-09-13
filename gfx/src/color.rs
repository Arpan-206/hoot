/// A 16-bit colour: 5 bits red, 6 bits green, 5 bits blue (RGB565).
///
/// Red occupies the high bits. This is the layout the ST7735 expects when
/// its MADCTL "BGR" bit is clear.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
pub struct Rgb565(pub u16);

const fn scale(c: u8, level: u8) -> u8 {
    ((c as u16 * level as u16) / 255) as u8
}

impl Rgb565 {
    pub const BLACK: Self = Self(0x0000);
    pub const WHITE: Self = Self(0xFFFF);
    pub const RED: Self = Self(0xF800);
    pub const GREEN: Self = Self(0x07E0);
    pub const BLUE: Self = Self(0x001F);
    pub const YELLOW: Self = Self(0xFFE0);
    pub const CYAN: Self = Self(0x07FF);
    pub const MAGENTA: Self = Self(0xF81F);

    /// Build a colour from 8-bit channels. Low bits are dropped.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self((((r as u16) & 0xF8) << 8) | (((g as u16) & 0xFC) << 3) | ((b as u16) >> 3))
    }

    /// Build a colour from a `0xRRGGBB` value.
    pub const fn hex(rgb: u32) -> Self {
        Self::new((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// Bytes in the order the display wants them (high byte first).
    pub const fn to_be_bytes(self) -> [u8; 2] {
        self.0.to_be_bytes()
    }

    /// Red channel expanded back to 8 bits.
    pub const fn r(self) -> u8 {
        let v = ((self.0 >> 11) & 0x1F) as u8;
        (v << 3) | (v >> 2)
    }

    /// Green channel expanded back to 8 bits.
    pub const fn g(self) -> u8 {
        let v = ((self.0 >> 5) & 0x3F) as u8;
        (v << 2) | (v >> 4)
    }

    /// Blue channel expanded back to 8 bits.
    pub const fn b(self) -> u8 {
        let v = (self.0 & 0x1F) as u8;
        (v << 3) | (v >> 2)
    }

    /// Darken the colour. `level` 255 keeps it unchanged, 0 gives black.
    pub const fn dimmed(self, level: u8) -> Self {
        Self::new(
            scale(self.r(), level),
            scale(self.g(), level),
            scale(self.b(), level),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_channels_in_rgb565_order() {
        assert_eq!(Rgb565::new(255, 0, 0), Rgb565::RED);
        assert_eq!(Rgb565::new(0, 255, 0), Rgb565::GREEN);
        assert_eq!(Rgb565::new(0, 0, 255), Rgb565::BLUE);
        assert_eq!(Rgb565::hex(0xFFFFFF), Rgb565::WHITE);
    }

    #[test]
    fn expands_channels_back_to_full_range() {
        let c = Rgb565::new(255, 255, 255);
        assert_eq!((c.r(), c.g(), c.b()), (255, 255, 255));
        let c = Rgb565::new(0, 0, 0);
        assert_eq!((c.r(), c.g(), c.b()), (0, 0, 0));
    }

    #[test]
    fn big_endian_bytes_put_red_first() {
        assert_eq!(Rgb565::RED.to_be_bytes(), [0xF8, 0x00]);
        assert_eq!(Rgb565::BLUE.to_be_bytes(), [0x00, 0x1F]);
    }

    #[test]
    fn dimming_to_zero_is_black() {
        assert_eq!(Rgb565::WHITE.dimmed(0), Rgb565::BLACK);
        assert_eq!(Rgb565::WHITE.dimmed(255), Rgb565::WHITE);
    }
}

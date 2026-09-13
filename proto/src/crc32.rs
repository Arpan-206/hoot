//! CRC-32 (IEEE 802.3, the zlib/PNG variant), computed bit by bit.
//! Slow but tiny: no 1 KiB table in flash. Fast enough for 40 KiB photos.

const POLY: u32 = 0xEDB8_8320;

/// Streaming CRC-32 state.
#[derive(Clone, Copy, Debug)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut c = self.0;
        for &b in bytes {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            }
        }
        self.0 = c;
    }

    pub const fn finish(self) -> u32 {
        !self.0
    }
}

/// CRC-32 of one buffer.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(bytes);
    c.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data = b"hello, sprig";
        let mut c = Crc32::new();
        c.update(&data[..5]);
        c.update(&data[5..]);
        assert_eq!(c.finish(), crc32(data));
    }
}

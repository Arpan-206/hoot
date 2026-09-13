//! Fixed-capacity string formatting without a heap.

use core::fmt;

/// A string buffer of at most `N` bytes. Extra output is dropped.
pub struct StrBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StrBuf<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> fmt::Write for StrBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let mut n = bytes.len().min(N - self.len);
        while n > 0 && !s.is_char_boundary(n) {
            n -= 1;
        }
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        if n == bytes.len() { Ok(()) } else { Err(fmt::Error) }
    }
}

/// Format into a new buffer: `let s: StrBuf<16> = format(format_args!("{n}%"));`
pub fn format<const N: usize>(args: fmt::Arguments) -> StrBuf<N> {
    let mut s = StrBuf::new();
    let _ = fmt::Write::write_fmt(&mut s, args);
    s
}

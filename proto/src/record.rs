//! The OS config record: a fixed layout with a sequence number and a CRC.
//!
//! Two flash sectors hold two copies, written alternately. On boot the copy
//! with the newest sequence number and a valid CRC wins. No heap, no serde.

use crate::crc32::crc32;

/// A string with a fixed capacity, stored inline. Never allocates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixedStr<const N: usize> {
    buf: [u8; N],
    len: u8,
}

impl<const N: usize> Default for FixedStr<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> FixedStr<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    /// Build from `s`, truncated to the capacity at a character boundary.
    pub fn truncated(s: &str) -> Self {
        let mut f = Self::new();
        f.set(s);
        f
    }

    /// Replace the content. Returns `false` if `s` had to be truncated.
    pub fn set(&mut self, s: &str) -> bool {
        self.len = 0;
        self.push_str(s)
    }

    /// Append. Returns `false` if `s` had to be truncated.
    pub fn push_str(&mut self, s: &str) -> bool {
        let room = N - self.len as usize;
        let mut n = s.len().min(room);
        while n > 0 && !s.is_char_boundary(n) {
            n -= 1;
        }
        let start = self.len as usize;
        self.buf[start..start + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n as u8;
        n == s.len()
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

/// Everything the OS persists. Add fields at the end and bump `VERSION`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Config {
    pub wifi_ssid: FixedStr<32>,
    pub wifi_password: FixedStr<64>,
    /// Base URL of the photo frame server, no trailing slash.
    pub frame_server: FixedStr<96>,
    /// Which frame this device is, as the server knows it.
    pub frame_name: FixedStr<24>,
    /// `Last-Modified` of the photo in the current slot.
    pub frame_last_modified: FixedStr<40>,
    /// Blob slot that holds the current photo.
    pub frame_slot: u8,
    /// How often the frame asks the server for a new photo.
    pub poll_secs: u16,
}

const MAGIC: &[u8; 4] = b"SPCF";
const VERSION: u16 = 1;
/// Encoded size in bytes: header, payload, CRC.
pub const RECORD_LEN: usize = 12 + (33 + 65 + 97 + 25 + 41) + 1 + 2 + 4;

struct Cursor<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn put(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.pos.checked_add(bytes.len())?;
        if end > self.buf.len() {
            return None;
        }
        self.buf[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
        Some(())
    }

    fn put_str<const N: usize>(&mut self, s: &FixedStr<N>) -> Option<()> {
        self.put(&[s.len])?;
        self.put(&s.buf)
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn str<const N: usize>(&mut self) -> Option<FixedStr<N>> {
        let len = self.u8()?;
        let bytes = self.take(N)?;
        if len as usize > N || core::str::from_utf8(&bytes[..len as usize]).is_err() {
            return None;
        }
        let mut f = FixedStr::<N>::new();
        f.buf.copy_from_slice(bytes);
        f.len = len;
        Some(f)
    }
}

/// Serialise `cfg` with sequence number `seq`. Returns the length written.
pub fn encode(cfg: &Config, seq: u32, out: &mut [u8]) -> Option<usize> {
    let mut c = Cursor { buf: out, pos: 0 };
    c.put(MAGIC)?;
    c.put(&VERSION.to_le_bytes())?;
    c.put(&(RECORD_LEN as u16).to_le_bytes())?;
    c.put(&seq.to_le_bytes())?;
    c.put_str(&cfg.wifi_ssid)?;
    c.put_str(&cfg.wifi_password)?;
    c.put_str(&cfg.frame_server)?;
    c.put_str(&cfg.frame_name)?;
    c.put_str(&cfg.frame_last_modified)?;
    c.put(&[cfg.frame_slot])?;
    c.put(&cfg.poll_secs.to_le_bytes())?;
    let body_len = c.pos;
    let crc = crc32(&c.buf[..body_len]);
    c.put(&crc.to_le_bytes())?;
    Some(c.pos)
}

/// Parse a record. Returns the config and its sequence number, or `None`
/// if the magic, version, length or CRC do not check out.
pub fn decode(buf: &[u8]) -> Option<(Config, u32)> {
    let mut r = Reader { buf, pos: 0 };
    if r.take(4)? != MAGIC || r.u16()? != VERSION {
        return None;
    }
    let len = r.u16()? as usize;
    if len != RECORD_LEN || buf.len() < len {
        return None;
    }
    let stored_crc = u32::from_le_bytes(buf[len - 4..len].try_into().ok()?);
    if crc32(&buf[..len - 4]) != stored_crc {
        return None;
    }
    let seq = r.u32()?;
    let cfg = Config {
        wifi_ssid: r.str()?,
        wifi_password: r.str()?,
        frame_server: r.str()?,
        frame_name: r.str()?,
        frame_last_modified: r.str()?,
        frame_slot: r.u8()?,
        poll_secs: r.u16()?,
    };
    Some((cfg, seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut c = Config::default();
        c.wifi_ssid.set("HomeNet");
        c.wifi_password.set("correct horse battery");
        c.frame_server.set("http://192.168.1.9:8000");
        c.frame_name.set("arpan");
        c.frame_last_modified.set("Sat, 13 Sep 2026 10:00:00 GMT");
        c.frame_slot = 1;
        c.poll_secs = 15;
        c
    }

    #[test]
    fn round_trip() {
        let mut buf = [0u8; 512];
        let n = encode(&sample(), 42, &mut buf).unwrap();
        assert_eq!(n, RECORD_LEN);
        let (back, seq) = decode(&buf[..n]).unwrap();
        assert_eq!(back, sample());
        assert_eq!(seq, 42);
    }

    #[test]
    fn detects_corruption() {
        let mut buf = [0u8; 512];
        let n = encode(&sample(), 1, &mut buf).unwrap();
        buf[20] ^= 0x01;
        assert!(decode(&buf[..n]).is_none());
        assert!(decode(&[0xFF; RECORD_LEN]).is_none());
        assert!(decode(&buf[..n - 1]).is_none());
    }

    #[test]
    fn too_small_output_fails_cleanly() {
        let mut buf = [0u8; 16];
        assert_eq!(encode(&sample(), 1, &mut buf), None);
    }

    #[test]
    fn fixed_str_truncates_at_char_boundary() {
        let mut s = FixedStr::<5>::new();
        assert!(!s.set("héllo"));
        assert_eq!(s.as_str(), "héll");
        assert!(s.set("ab"));
        assert!(s.push_str("cd"));
        assert!(!s.push_str("ef"));
        assert_eq!(s.as_str(), "abcde");
    }
}

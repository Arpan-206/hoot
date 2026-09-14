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
    /// Battery saver: 0 = auto (from the USB sense, plain Pico only),
    /// 1 = always on, 2 = always off.
    pub power_mode: u8,
    /// Speaker volume, 0 (off) to 10.
    pub sound: u8,
    /// Daily alarm: minutes after midnight, on/off, and the tone index.
    pub alarm_min: u16,
    pub alarm_on: u8,
    pub alarm_tone: u8,
}

pub const POWER_AUTO: u8 = 0;
pub const POWER_SAVER: u8 = 1;
pub const POWER_NORMAL: u8 = 2;
pub const SOUND_OFF: u8 = 0;
pub const SOUND_MAX: u8 = 10;
pub const SOUND_DEFAULT: u8 = 8;
/// 07:00.
pub const ALARM_DEFAULT_MIN: u16 = 7 * 60;

const MAGIC: &[u8; 4] = b"SPCF";
const VERSION: u16 = 1;
/// Encoded size in bytes: header, payload, CRC. Fields added later sit at
/// the end of the payload; older, shorter records still decode and the
/// missing fields take their defaults. Never reorder or remove a field.
pub const RECORD_LEN: usize = 12 + (33 + 65 + 97 + 25 + 41) + 1 + 2 + 1 + 1 + 2 + 1 + 1 + 4;
/// Bytes after `poll_secs`, added by later firmware one at a time.
#[cfg(test)]
const TRAILING_LEN: usize = 1 + 1 + 2 + 1 + 1;
const HEADER_LEN: usize = 12;
const CRC_LEN: usize = 4;

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
    c.put(&[cfg.power_mode])?;
    c.put(&[cfg.sound])?;
    c.put(&cfg.alarm_min.to_le_bytes())?;
    c.put(&[cfg.alarm_on])?;
    c.put(&[cfg.alarm_tone])?;
    let body_len = c.pos;
    let crc = crc32(&c.buf[..body_len]);
    c.put(&crc.to_le_bytes())?;
    Some(c.pos)
}

/// Parse a record. Returns the config and its sequence number, or `None`
/// if the magic, version, length or CRC do not check out. Records written
/// by older firmware, which are shorter, decode with defaults for the
/// fields they lack.
pub fn decode(buf: &[u8]) -> Option<(Config, u32)> {
    let mut r = Reader { buf, pos: 0 };
    if r.take(4)? != MAGIC || r.u16()? != VERSION {
        return None;
    }
    let len = r.u16()? as usize;
    if !(HEADER_LEN + CRC_LEN..=RECORD_LEN).contains(&len) || buf.len() < len {
        return None;
    }
    let stored_crc = u32::from_le_bytes(buf[len - CRC_LEN..len].try_into().ok()?);
    if crc32(&buf[..len - CRC_LEN]) != stored_crc {
        return None;
    }
    let seq = r.u32()?;
    // Read fields from the payload only; anything past its end is absent.
    let mut r = Reader { buf: &buf[..len - CRC_LEN], pos: HEADER_LEN };
    let cfg = Config {
        wifi_ssid: r.str().unwrap_or_default(),
        wifi_password: r.str().unwrap_or_default(),
        frame_server: r.str().unwrap_or_default(),
        frame_name: r.str().unwrap_or_default(),
        frame_last_modified: r.str().unwrap_or_default(),
        frame_slot: r.u8().unwrap_or(0),
        poll_secs: r.u16().unwrap_or(15),
        power_mode: r.u8().unwrap_or(POWER_AUTO),
        sound: r.u8().unwrap_or(SOUND_DEFAULT),
        alarm_min: r.u16().unwrap_or(ALARM_DEFAULT_MIN),
        alarm_on: r.u8().unwrap_or(0),
        alarm_tone: r.u8().unwrap_or(0),
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
        c.power_mode = POWER_SAVER;
        c.sound = 7;
        c.alarm_min = 6 * 60 + 30;
        c.alarm_on = 1;
        c.alarm_tone = 2;
        c
    }

    /// A record as the first firmware wrote it: same layout without the
    /// trailing bytes later versions added, with its own length and CRC.
    fn older_record(cfg: &Config, seq: u32) -> Vec<u8> {
        let mut buf = [0u8; 512];
        let n = encode(cfg, seq, &mut buf).unwrap();
        let mut old = buf[..n - CRC_LEN - TRAILING_LEN].to_vec(); // drop the newer fields and the CRC
        let len = (old.len() + CRC_LEN) as u16;
        old[6..8].copy_from_slice(&len.to_le_bytes());
        let crc = crc32(&old);
        old.extend_from_slice(&crc.to_le_bytes());
        old
    }

    #[test]
    fn decodes_records_from_older_firmware() {
        let old = older_record(&sample(), 7);
        assert_eq!(old.len(), RECORD_LEN - TRAILING_LEN);
        let (cfg, seq) = decode(&old).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(cfg.wifi_ssid, sample().wifi_ssid);
        assert_eq!(cfg.frame_slot, 1);
        assert_eq!(cfg.poll_secs, 15);
        assert_eq!(cfg.power_mode, POWER_AUTO, "missing field takes its default");
        assert_eq!(cfg.sound, SOUND_DEFAULT, "missing field takes its default");
        assert_eq!(cfg.alarm_min, ALARM_DEFAULT_MIN);
        assert_eq!((cfg.alarm_on, cfg.alarm_tone), (0, 0));
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

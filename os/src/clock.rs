//! Wall-clock time. The RP2040 has no clock chip, so the OS keeps the
//! offset between its millisecond uptime and local seconds since 1970. A
//! Pico W learns it from the server with every heartbeat (`X-Sprig-Time`
//! plus the frame's zone offset). Any board can have it set by hand in the
//! Clock app. It is lost at power-off.

use portable_atomic::{AtomicU8, AtomicU32, Ordering};
use hoot_proto::time::{Civil, civil_from_secs};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Source {
    Unset = 0,
    Manual = 1,
    Server = 2,
}

impl Source {
    pub const fn label(self) -> &'static str {
        match self {
            Source::Unset => "not set",
            Source::Manual => "set by hand",
            Source::Server => "from server",
        }
    }
}

/// Local seconds since 1970 at uptime zero.
static BASE: AtomicU32 = AtomicU32::new(0);
static SOURCE: AtomicU8 = AtomicU8::new(Source::Unset as u8);

pub fn set(local_secs: u32, now_ms: u32, source: Source) {
    BASE.store(local_secs.wrapping_sub(now_ms / 1000), Ordering::Relaxed);
    SOURCE.store(source as u8, Ordering::Relaxed);
}

pub fn source() -> Source {
    match SOURCE.load(Ordering::Relaxed) {
        1 => Source::Manual,
        2 => Source::Server,
        _ => Source::Unset,
    }
}

/// Local seconds since 1970, if the clock is set.
pub fn now_secs(now_ms: u32) -> Option<u32> {
    (source() != Source::Unset).then(|| BASE.load(Ordering::Relaxed).wrapping_add(now_ms / 1000))
}

pub fn now(now_ms: u32) -> Option<Civil> {
    now_secs(now_ms).map(|s| civil_from_secs(s as i64))
}

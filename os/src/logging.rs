//! Log macros that prefix every line with the uptime in milliseconds, so
//! timing problems show up in the USB log. Use these instead of `log::*`.
//!
//! Warnings are also kept in a small ring in RAM. The photo frame posts
//! new ones to the server, so problems can be read from far away.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

/// Milliseconds since boot, for log prefixes.
pub fn ms() -> u64 {
    embassy_time::Instant::now().as_millis()
}

macro_rules! info {
    ($($arg:tt)*) => {
        ::log::info!("[{:>7}] {}", $crate::logging::ms(), format_args!($($arg)*))
    };
}

macro_rules! warn {
    ($($arg:tt)*) => {{
        ::log::warn!("[{:>7}] {}", $crate::logging::ms(), format_args!($($arg)*));
        $crate::logging::remember(format_args!($($arg)*));
    }};
}

const LINES: usize = 8;
const LINE_LEN: usize = 96;

struct Ring {
    lines: [[u8; LINE_LEN]; LINES],
    lens: [u8; LINES],
    /// Where the next line goes.
    next: usize,
    /// Lines written and not yet drained, at most `LINES`.
    unsent: usize,
}

static RING: Mutex<CriticalSectionRawMutex, RefCell<Ring>> = Mutex::new(RefCell::new(Ring {
    lines: [[0; LINE_LEN]; LINES],
    lens: [0; LINES],
    next: 0,
    unsent: 0,
}));

/// Keep a warning for the remote log. Called by the `warn!` macro.
pub fn remember(args: core::fmt::Arguments) {
    let line: crate::ui::text::StrBuf<LINE_LEN> =
        crate::ui::text::format(format_args!("[{:>7}] {}", ms(), args));
    RING.lock(|cell| {
        let mut r = cell.borrow_mut();
        let i = r.next;
        let bytes = line.as_str().as_bytes();
        r.lines[i][..bytes.len()].copy_from_slice(bytes);
        r.lens[i] = bytes.len() as u8;
        r.next = (i + 1) % LINES;
        r.unsent = (r.unsent + 1).min(LINES);
    });
}

/// Copy the newest warning into `out`. Returns the length, 0 if none.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn latest(out: &mut [u8]) -> usize {
    RING.lock(|cell| {
        let r = cell.borrow();
        if r.lens.iter().all(|&l| l == 0) {
            return 0;
        }
        let i = (r.next + LINES - 1) % LINES;
        let len = (r.lens[i] as usize).min(out.len());
        out[..len].copy_from_slice(&r.lines[i][..len]);
        len
    })
}

/// Number of warnings not yet drained.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn unsent() -> usize {
    RING.lock(|cell| cell.borrow().unsent)
}

/// Copy the undrained warnings into `out`, one per line, oldest first, and
/// mark them drained. Returns the number of bytes written.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn drain_into(out: &mut [u8]) -> usize {
    RING.lock(|cell| {
        let mut r = cell.borrow_mut();
        let mut n = 0;
        let count = r.unsent;
        for k in 0..count {
            let i = (r.next + LINES - count + k) % LINES;
            let len = r.lens[i] as usize;
            if n + len + 1 > out.len() {
                break;
            }
            out[n..n + len].copy_from_slice(&r.lines[i][..len]);
            n += len;
            out[n] = b'\n';
            n += 1;
        }
        r.unsent = 0;
        n
    })
}

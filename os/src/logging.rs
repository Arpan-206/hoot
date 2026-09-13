//! Log macros that prefix every line with the uptime in milliseconds, so
//! timing problems show up in the USB log. Use these instead of `log::*`.

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
    ($($arg:tt)*) => {
        ::log::warn!("[{:>7}] {}", $crate::logging::ms(), format_args!($($arg)*))
    };
}

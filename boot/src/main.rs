//! Hoot boot loader.
//!
//! Runs first, before the OS. If the OS wrote a new image into the update
//! partition and marked it, this swaps it into the active partition and
//! starts it. If that new image never reports a good boot, the next reset
//! swaps the old one back. Flashed once over USB; it never updates itself.

#![no_std]
#![no_main]

use core::cell::RefCell;

use cortex_m_rt::{entry, exception};
use embassy_boot_rp::{BootLoader, BootLoaderConfig, WatchdogFlash};
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::Duration;

const FLASH_SIZE: usize = 2 * 1024 * 1024;
/// The OS must feed the watchdog within this time after boot, or the board
/// resets. Its own 3 s watchdog takes over once it is running.
const WATCHDOG: Duration = Duration::from_secs(8);

#[entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());
    let flash = WatchdogFlash::<FLASH_SIZE>::start(p.FLASH, p.WATCHDOG, WATCHDOG);
    let flash = Mutex::new(RefCell::new(flash));
    let config = BootLoaderConfig::from_linkerfile_blocking(&flash, &flash, &flash);
    let active_offset = config.active.offset();
    let bl: BootLoader = BootLoader::prepare(config);
    unsafe { bl.load(embassy_rp::flash::FLASH_BASE as u32 + active_offset) }
}

#[unsafe(no_mangle)]
#[cfg_attr(target_os = "none", unsafe(link_section = ".HardFault.user"))]
unsafe extern "C" fn HardFault() {
    cortex_m::peripheral::SCB::sys_reset();
}

#[exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    cortex_m::peripheral::SCB::sys_reset();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::peripheral::SCB::sys_reset();
}

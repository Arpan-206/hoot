//! Flash-backed storage.
//!
//! Two kinds of data live in flash beside the firmware:
//!
//! - one small config record (Wi-Fi, frame server, and so on), kept in two
//!   sectors written alternately so a power cut never loses both copies;
//! - blob slots of 64 KiB for large records such as a cached photo. Each
//!   slot starts with a header sector that is written last, so a slot is
//!   either complete or invalid.
//!
//! Reads use the memory-mapped flash (XIP) directly: they need no driver
//! and no copying. Only erase and program go through the flash driver,
//! behind a mutex, because they stall the core and must not overlap.

// Only the network apps persist anything so far.
#![cfg_attr(not(feature = "wifi"), allow(dead_code))]

mod blob;
mod config;

pub use blob::BlobHeader;
#[cfg(feature = "wifi")]
pub use blob::{BLOB_DATA_MAX, BlobWriter};
pub use hoot_proto::record::Config;

use core::cell::RefCell;

use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use crate::board::FLASH_SIZE;

pub type FlashDev = Flash<'static, FLASH, Blocking, FLASH_SIZE>;
pub type FlashMutex = Mutex<CriticalSectionRawMutex, RefCell<FlashDev>>;

/// Base address of the memory-mapped flash.
const XIP_BASE: usize = 0x1000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    Flash,
    Encode,
    Range,
}

/// Borrow `len` bytes of flash at `offset` straight from the XIP window.
///
/// Panics if the range is outside the flash. Callers keep the slice only
/// while nothing rewrites that region; the OS never rewrites a live slot.
pub fn xip(offset: u32, len: usize) -> &'static [u8] {
    let end = offset as usize + len;
    assert!(end <= FLASH_SIZE, "flash read out of range");
    // SAFETY: the RP2040 maps the whole flash read-only at XIP_BASE, and the
    // range was checked against the flash size above.
    unsafe { core::slice::from_raw_parts((XIP_BASE + offset as usize) as *const u8, len) }
}

/// The app-facing storage API. Owned by the shell task.
pub struct Storage {
    flash: &'static FlashMutex,
    config: Config,
    seq: u32,
}

impl Storage {
    /// Load the config from flash, or start from `defaults` if none is valid.
    pub fn new(flash: &'static FlashMutex, defaults: Config) -> Self {
        let (config, seq) = config::load().unwrap_or((defaults, 0));
        Self { flash, config, seq }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Change the config and persist it. Unchanged configs are not rewritten.
    pub fn update_config(&mut self, f: impl FnOnce(&mut Config)) -> Result<(), StorageError> {
        let mut next = self.config.clone();
        f(&mut next);
        if next == self.config {
            return Ok(());
        }
        let seq = self.seq.wrapping_add(1);
        config::store(self.flash, &next, seq)?;
        self.config = next;
        self.seq = seq;
        Ok(())
    }

    /// Header of a blob slot, if the slot holds a complete record.
    pub fn blob_header(&self, slot: u8) -> Option<BlobHeader> {
        blob::read_header(slot)
    }

    /// The data of a complete blob slot, borrowed from flash.
    pub fn blob_data(&self, slot: u8) -> Option<&'static [u8]> {
        blob::read_data(slot)
    }

    /// Make a blob slot read as empty.
    pub fn blob_erase(&self, slot: u8) -> Result<(), StorageError> {
        blob::erase_header(self.flash, slot)
    }
}

//! Over-the-air firmware updates.
//!
//! The boot loader (`boot/`) keeps two firmware partitions. The OS streams
//! a new image into the update partition, marks it, and reboots. The boot
//! loader swaps it in. If the new image never calls `confirm_boot`, the
//! next reset swaps the old image back. See `board::flash_map`.

// Only the network build downloads images; every build confirms its boot.
#![cfg_attr(not(feature = "wifi"), allow(dead_code))]

use embassy_boot_rp::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_embedded_hal::flash::partition::BlockingPartition;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use crate::board::FLASH_SECTOR;
use crate::board::flash_map::{BOOT_STATE_START, DFU_SIZE, DFU_START};
use crate::storage::{FlashDev, FlashMutex};

pub type Part<'a> = BlockingPartition<'a, CriticalSectionRawMutex, FlashDev>;
pub type Updater<'a> = BlockingFirmwareUpdater<'a, Part<'a>, Part<'a>>;

/// Scratch the updater needs for the state partition.
pub type Scratch = AlignedBuffer<1>;

pub fn scratch() -> Scratch {
    AlignedBuffer([0u8; 1])
}

/// Build an updater over the shared flash. `scratch` must outlive it.
pub fn updater<'a>(flash: &'a FlashMutex, scratch: &'a mut Scratch) -> Updater<'a> {
    let config = FirmwareUpdaterConfig {
        dfu: BlockingPartition::new(flash, DFU_START, DFU_SIZE),
        state: BlockingPartition::new(flash, BOOT_STATE_START, FLASH_SECTOR),
    };
    BlockingFirmwareUpdater::new(config, &mut scratch.0)
}

/// Tell the boot loader this image works, so it stops the rollback timer.
/// Returns true if a pending swap was confirmed.
pub fn confirm_boot(flash: &FlashMutex) -> bool {
    let mut scratch = AlignedBuffer([0u8; 1]);
    let mut up = updater(flash, &mut scratch);
    match up.get_state() {
        Ok(State::Swap) => {
            let ok = up.mark_booted().is_ok();
            info!("ota: new firmware confirmed: {}", ok);
            ok
        }
        Ok(State::Revert) => {
            warn!("ota: boot loader reverted a bad update");
            let _ = up.mark_booted();
            false
        }
        _ => false,
    }
}

#[repr(C, align(4))]
struct Sector([u8; FLASH_SECTOR as usize]);

/// Streams an image into the update partition, one sector at a time.
pub struct ImageWriter<'u, 'a> {
    updater: &'u mut Updater<'a>,
    offset: usize,
    buf: Sector,
    fill: usize,
}

impl<'u, 'a> ImageWriter<'u, 'a> {
    pub fn new(updater: &'u mut Updater<'a>) -> Self {
        Self { updater, offset: 0, buf: Sector([0xFF; FLASH_SECTOR as usize]), fill: 0 }
    }

    pub fn write(&mut self, mut data: &[u8]) -> Result<(), ()> {
        while !data.is_empty() {
            let room = self.buf.0.len() - self.fill;
            let n = room.min(data.len());
            self.buf.0[self.fill..self.fill + n].copy_from_slice(&data[..n]);
            self.fill += n;
            data = &data[n..];
            if self.fill == self.buf.0.len() {
                self.flush()?;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ()> {
        if self.fill == 0 {
            return Ok(());
        }
        if self.offset + self.fill > DFU_SIZE as usize {
            return Err(());
        }
        // Whole pages only: pad the tail with erased bytes.
        let len = self.fill.div_ceil(256) * 256;
        self.updater
            .write_firmware(self.offset, &self.buf.0[..len])
            .map_err(|_| ())?;
        self.offset += self.fill;
        self.fill = 0;
        self.buf.0.fill(0xFF);
        Ok(())
    }

    /// Flush the tail and mark the update for the boot loader.
    pub fn finish(mut self) -> Result<usize, ()> {
        self.flush()?;
        self.updater.mark_updated().map_err(|_| ())?;
        Ok(self.offset)
    }
}

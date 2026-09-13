//! Two-sector config store. See the module docs in `storage`.

use sprig_proto::record::{self, Config, RECORD_LEN};

use super::{FlashMutex, StorageError, xip};
use crate::board::FLASH_SECTOR;
use crate::board::flash_map::{CONFIG_SECTORS, CONFIG_START};

/// Records are padded to whole flash pages.
const PADDED_LEN: usize = RECORD_LEN.div_ceil(256) * 256;

#[repr(C, align(4))]
struct Page([u8; PADDED_LEN]);

fn sector_offset(i: u32) -> u32 {
    CONFIG_START + i * FLASH_SECTOR
}

/// Newest valid record across the config sectors.
pub fn load() -> Option<(Config, u32)> {
    let mut best: Option<(Config, u32)> = None;
    for i in 0..CONFIG_SECTORS {
        let bytes = xip(sector_offset(i), RECORD_LEN);
        if let Some((cfg, seq)) = record::decode(bytes) {
            let newer = match &best {
                None => true,
                // Wrapping compare so the counter may roll over some day.
                Some((_, best_seq)) => seq.wrapping_sub(*best_seq) < u32::MAX / 2,
            };
            if newer {
                best = Some((cfg, seq));
            }
        }
    }
    best
}

/// Write `cfg` as sequence `seq` into the sector chosen by `seq`.
///
/// Alternating sectors means the previous record survives until the new
/// one is fully written and verified by its CRC on the next load.
pub fn store(flash: &FlashMutex, cfg: &Config, seq: u32) -> Result<(), StorageError> {
    let mut page = Page([0xFF; PADDED_LEN]);
    record::encode(cfg, seq, &mut page.0).ok_or(StorageError::Encode)?;
    let offset = sector_offset(seq % CONFIG_SECTORS);
    flash.lock(|f| {
        let mut f = f.borrow_mut();
        f.blocking_erase(offset, offset + FLASH_SECTOR)
            .map_err(|_| StorageError::Flash)?;
        f.blocking_write(offset, &page.0).map_err(|_| StorageError::Flash)
    })
}

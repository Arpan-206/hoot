//! Blob slots: 64 KiB records with a header sector written last.

use hoot_proto::crc32::Crc32;

use super::{FlashMutex, StorageError, xip};
use crate::board::FLASH_SECTOR;
use crate::board::flash_map::{BLOB_SLOT_SIZE, BLOB_SLOTS, BLOBS_START};

/// The header occupies the first sector of a slot; data follows.
const HEADER_SECTOR: u32 = FLASH_SECTOR;
/// Largest blob that fits in one slot.
pub const BLOB_DATA_MAX: u32 = BLOB_SLOT_SIZE - HEADER_SECTOR;

const MAGIC: u32 = 0x5350_5242; // "SPRB"
const VERSION: u32 = 1;
const HEADER_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobHeader {
    /// What the blob is, chosen by the app. For example `b"PHOT"`.
    pub kind: u32,
    pub len: u32,
    pub crc32: u32,
    /// App-chosen counter, for example the time the blob was stored.
    pub seq: u32,
}

impl BlobHeader {
        fn to_bytes(self) -> [u8; HEADER_LEN] {
        let mut b = [0xFFu8; HEADER_LEN];
        for (i, word) in [MAGIC, VERSION, self.kind, self.len, self.crc32, self.seq]
            .iter()
            .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        b
    }

    fn from_bytes(b: &[u8]) -> Option<Self> {
        let word = |i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        if b.len() < HEADER_LEN || word(0) != MAGIC || word(1) != VERSION {
            return None;
        }
        let h = Self { kind: word(2), len: word(3), crc32: word(4), seq: word(5) };
        (h.len <= BLOB_DATA_MAX).then_some(h)
    }
}

fn slot_base(slot: u8) -> u32 {
    assert!(slot < BLOB_SLOTS, "blob slot out of range");
    BLOBS_START + slot as u32 * BLOB_SLOT_SIZE
}

/// Header of `slot`, if it holds a complete record.
pub fn read_header(slot: u8) -> Option<BlobHeader> {
    BlobHeader::from_bytes(xip(slot_base(slot), HEADER_LEN))
}

/// Data of `slot`, if it holds a complete record.
pub fn read_data(slot: u8) -> Option<&'static [u8]> {
    let h = read_header(slot)?;
    Some(xip(slot_base(slot) + HEADER_SECTOR, h.len as usize))
}

/// Invalidate `slot` by erasing its header sector. The data stays but is
/// never read again, and the next writer overwrites it.
pub fn erase_header(flash: &FlashMutex, slot: u8) -> Result<(), StorageError> {
    let base = slot_base(slot);
    flash.lock(|f| {
        f.borrow_mut()
            .blocking_erase(base, base + HEADER_SECTOR)
            .map_err(|_| StorageError::Flash)
    })
}

#[repr(C, align(4))]
struct Sector([u8; FLASH_SECTOR as usize]);

/// Streams a blob into a slot, one sector at a time. RAM cost: one sector.
///
/// The header sector is erased first, so a slot with a half-written blob
/// reads as empty. `finish` writes the header and makes the slot valid.
pub struct BlobWriter {
    flash: &'static FlashMutex,
    base: u32,
    kind: u32,
    seq: u32,
    written: u32,
    buf: Sector,
    fill: usize,
    crc: Crc32,
}

impl BlobWriter {
    pub fn begin(flash: &'static FlashMutex, slot: u8, kind: u32, seq: u32) -> Result<Self, StorageError> {
        let base = slot_base(slot);
        flash.lock(|f| {
            f.borrow_mut()
                .blocking_erase(base, base + HEADER_SECTOR)
                .map_err(|_| StorageError::Flash)
        })?;
        Ok(Self {
            flash,
            base,
            kind,
            seq,
            written: 0,
            buf: Sector([0xFF; FLASH_SECTOR as usize]),
            fill: 0,
            crc: Crc32::new(),
        })
    }

    pub fn write(&mut self, mut data: &[u8]) -> Result<(), StorageError> {
        while !data.is_empty() {
            let room = self.buf.0.len() - self.fill;
            let n = room.min(data.len());
            self.buf.0[self.fill..self.fill + n].copy_from_slice(&data[..n]);
            self.fill += n;
            data = &data[n..];
            if self.fill == self.buf.0.len() {
                self.flush_sector()?;
            }
        }
        Ok(())
    }

    fn flush_sector(&mut self) -> Result<(), StorageError> {
        if self.fill == 0 {
            return Ok(());
        }
        let offset = self.base + HEADER_SECTOR + self.written;
        if offset + FLASH_SECTOR > self.base + BLOB_SLOT_SIZE {
            return Err(StorageError::Range);
        }
        self.crc.update(&self.buf.0[..self.fill]);
        self.flash.lock(|f| {
            let mut f = f.borrow_mut();
            f.blocking_erase(offset, offset + FLASH_SECTOR)
                .map_err(|_| StorageError::Flash)?;
            f.blocking_write(offset, &self.buf.0).map_err(|_| StorageError::Flash)
        })?;
        self.written += self.fill as u32;
        self.fill = 0;
        self.buf.0.fill(0xFF);
        Ok(())
    }

    /// Flush the last sector and write the header. The slot is valid after this.
    pub fn finish(mut self) -> Result<BlobHeader, StorageError> {
        self.flush_sector()?;
        let header = BlobHeader {
            kind: self.kind,
            len: self.written,
            crc32: self.crc.finish(),
            seq: self.seq,
        };
        let mut page = Sector([0xFF; FLASH_SECTOR as usize]);
        page.0[..HEADER_LEN].copy_from_slice(&header.to_bytes());
        let base = self.base;
        self.flash.lock(|f| {
            f.borrow_mut()
                .blocking_write(base, &page.0[..256])
                .map_err(|_| StorageError::Flash)
        })?;
        Ok(header)
    }
}

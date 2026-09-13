/* Sprig flash map, boot loader view. Must match os/memory.x and os/src/board.rs.
   0x000000 boot2 + boot loader   24 KiB
   0x006000 boot loader state      4 KiB
   0x007000 active firmware      640 KiB
   0x0A7000 update (DFU)         644 KiB
   0x148000 radio firmware       288 KiB
   0x190000 blob slots           384 KiB
   0x1F0000 config                 8 KiB */
MEMORY
{
  BOOT2            : ORIGIN = 0x10000000, LENGTH = 0x100
  FLASH            : ORIGIN = 0x10000100, LENGTH = 24K - 0x100
  BOOTLOADER_STATE : ORIGIN = 0x10006000, LENGTH = 4K
  ACTIVE           : ORIGIN = 0x10007000, LENGTH = 640K
  DFU              : ORIGIN = 0x100A7000, LENGTH = 644K
  RAM              : ORIGIN = 0x20000000, LENGTH = 264K
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE) - ORIGIN(BOOT2);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE) - ORIGIN(BOOT2);
__bootloader_active_start = ORIGIN(ACTIVE) - ORIGIN(BOOT2);
__bootloader_active_end = ORIGIN(ACTIVE) + LENGTH(ACTIVE) - ORIGIN(BOOT2);
__bootloader_dfu_start = ORIGIN(DFU) - ORIGIN(BOOT2);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU) - ORIGIN(BOOT2);

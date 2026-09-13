//! Board definition for the Hack Club Sprig.
//!
//! The Sprig is a carrier board for a Raspberry Pi Pico (RP2040). The pin
//! numbers below are RP2040 GPIO numbers. They were taken from the stock
//! Hack Club firmware (`firmware/sprig_hal` and `firmware/spade`).

#![allow(dead_code)]

/// Crystal frequency on the Pico module.
pub const XTAL_FREQ_HZ: u32 = 12_000_000;

/// External flash on the Pico module.
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;
/// Smallest erasable unit of the flash.
pub const FLASH_SECTOR: u32 = 4096;

/// How the 2 MiB flash is divided. Offsets are from the start of flash.
/// The boot loader (`boot/`) and `memory.x` must agree with this table.
pub mod flash_map {
    /// Boot2 plus the boot loader.
    pub const BOOTLOADER_END: u32 = 0x0_6000;
    /// Boot loader state: one 4 KiB sector.
    pub const BOOT_STATE_START: u32 = 0x0_6000;
    /// Active firmware, where the OS runs from: 640 KiB.
    pub const ACTIVE_START: u32 = 0x0_7000;
    pub const ACTIVE_SIZE: u32 = 0xA_0000;
    /// Update (DFU) partition: active size plus one sector for the swap.
    pub const DFU_START: u32 = 0x0A_7000;
    pub const DFU_SIZE: u32 = 0xA_1000;
    /// Radio firmware for the Pico W: stored once, outside both firmware
    /// partitions, so an update never re-sends the 231 KiB blob.
    pub const RADIO_START: u32 = 0x14_8000;
    pub const RADIO_SIZE: u32 = 0x4_8000;
    /// Blob store: large records such as cached photos. 6 slots of 64 KiB.
    pub const BLOBS_START: u32 = 0x19_0000;
    pub const BLOB_SLOT_SIZE: u32 = 0x1_0000;
    pub const BLOB_SLOTS: u8 = 6;
    /// Config store: two 4 KiB sectors written alternately.
    pub const CONFIG_START: u32 = 0x1F_0000;
    pub const CONFIG_SECTORS: u32 = 2;
    // 0x1F_2000 to 0x20_0000 (56 KiB) stays free.
}

/// SPI clock for the display. 125 MHz / 4 gives exactly this value.
/// Drop to 20_000_000 if the picture shows noise.
pub const TFT_SPI_HZ: u32 = 31_250_000;

/// MADCTL bits (memory data access control) of the ST7735.
pub const MADCTL_MY: u8 = 0x80; // mirror rows
pub const MADCTL_MX: u8 = 0x40; // mirror columns
pub const MADCTL_MV: u8 = 0x20; // swap rows and columns (landscape)
pub const MADCTL_ML: u8 = 0x10; // vertical refresh direction
pub const MADCTL_BGR: u8 = 0x08; // panel colour order

/// Landscape, same way up as the stock firmware, with RGB565 sent red-first.
/// Verified on hardware: right way up, colours correct.
pub const TFT_MADCTL: u8 = MADCTL_MX | MADCTL_MV | MADCTL_ML;

/// GPIO assignments.
pub mod pins {
    // ST7735 display on SPI0.
    pub const TFT_SCK: u8 = 18;
    pub const TFT_MOSI: u8 = 19;
    pub const TFT_MISO: u8 = 16; // wired but unused
    pub const TFT_CS: u8 = 20;
    pub const TFT_DC: u8 = 22;
    pub const TFT_RST: u8 = 26;
    pub const TFT_BACKLIGHT: u8 = 17; // PWM slice 0, channel B

    // Buttons. Active low with internal pull-ups.
    pub const BTN_W: u8 = 5;
    pub const BTN_A: u8 = 6;
    pub const BTN_S: u8 = 7;
    pub const BTN_D: u8 = 8;
    pub const BTN_I: u8 = 12;
    pub const BTN_J: u8 = 13;
    pub const BTN_K: u8 = 14;
    pub const BTN_L: u8 = 15;

    // White LEDs.
    pub const LED_LEFT: u8 = 28; // PWM slice 6, channel A
    pub const LED_RIGHT: u8 = 4; // PWM slice 2, channel A

    // I2S audio to the MAX98357A amplifier. Not driven yet.
    pub const I2S_DIN: u8 = 9;
    pub const I2S_BCLK: u8 = 10;
    pub const I2S_LRCLK: u8 = 11;

    // Power.
    pub const VBUS_DETECT: u8 = 24; // high when USB is connected (plain Pico only)
    pub const VSYS_SENSE: u8 = 29; // ADC channel 3, reads VSYS / 3

    // Pins that differ between a plain Pico and a Pico W.
    // See `drivers::module` for how the board is detected.
    pub const SMPS_POWER_SAVE: u8 = 23; // plain Pico: regulator mode
    pub const PICO_LED: u8 = 25; // plain Pico: on-board LED
    pub const WL_ON: u8 = 23; // Pico W: wireless chip power
    pub const WL_D: u8 = 24; // Pico W: wireless SPI data
    pub const WL_CS: u8 = 25; // Pico W: wireless SPI chip select
    pub const WL_CLK: u8 = 29; // Pico W: wireless SPI clock, shared with ADC3
}

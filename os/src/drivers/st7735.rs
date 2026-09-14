//! Driver for the ST7735 160x128 TFT on the Sprig.
//!
//! The panel is used in landscape. A whole frame is written in one SPI burst
//! from a big-endian RGB565 buffer, so no per-pixel conversion is needed.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;
use hoot_gfx::{BYTES, HEIGHT, WIDTH};

use crate::board::TFT_MADCTL;

#[allow(dead_code)]
mod cmd {
    pub const SWRESET: u8 = 0x01;
    pub const SLPIN: u8 = 0x10;
    pub const SLPOUT: u8 = 0x11;
    pub const NORON: u8 = 0x13;
    pub const INVOFF: u8 = 0x20;
    pub const INVON: u8 = 0x21;
    pub const DISPOFF: u8 = 0x28;
    pub const DISPON: u8 = 0x29;
    pub const CASET: u8 = 0x2A;
    pub const RASET: u8 = 0x2B;
    pub const RAMWR: u8 = 0x2C;
    pub const MADCTL: u8 = 0x36;
    pub const COLMOD: u8 = 0x3A;
    pub const FRMCTR1: u8 = 0xB1;
    pub const FRMCTR2: u8 = 0xB2;
    pub const FRMCTR3: u8 = 0xB3;
    pub const INVCTR: u8 = 0xB4;
    pub const PWCTR1: u8 = 0xC0;
    pub const PWCTR2: u8 = 0xC1;
    pub const PWCTR3: u8 = 0xC2;
    pub const PWCTR4: u8 = 0xC3;
    pub const PWCTR5: u8 = 0xC4;
    pub const VMCTR1: u8 = 0xC5;
    pub const GMCTRP1: u8 = 0xE0;
    pub const GMCTRN1: u8 = 0xE1;
}

pub struct St7735<SPI, CS, DC, RST> {
    spi: SPI,
    cs: CS,
    dc: DC,
    rst: RST,
}

impl<SPI, CS, DC, RST> St7735<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    pub fn new(spi: SPI, cs: CS, dc: DC, rst: RST) -> Self {
        Self { spi, cs, dc, rst }
    }

    /// Reset the panel and load the register set used by the stock firmware.
    pub fn init<D: DelayNs>(&mut self, delay: &mut D) {
        self.hardware_reset(delay);

        self.command(cmd::SWRESET, &[]);
        delay.delay_ms(150);
        self.command(cmd::SLPOUT, &[]);
        delay.delay_ms(255);

        // Frame rate: normal, idle, partial modes.
        self.command(cmd::FRMCTR1, &[0x01, 0x2C, 0x2D]);
        self.command(cmd::FRMCTR2, &[0x01, 0x2C, 0x2D]);
        self.command(cmd::FRMCTR3, &[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D]);
        // Display inversion control: no inversion.
        self.command(cmd::INVCTR, &[0x07]);
        // Power control.
        self.command(cmd::PWCTR1, &[0xA2, 0x02, 0x84]);
        self.command(cmd::PWCTR2, &[0xC5]);
        self.command(cmd::PWCTR3, &[0x0A, 0x00]);
        self.command(cmd::PWCTR4, &[0x8A, 0x2A]);
        self.command(cmd::PWCTR5, &[0x8A, 0xEE]);
        self.command(cmd::VMCTR1, &[0x0E]);
        self.command(cmd::INVOFF, &[]);
        // Orientation and 16-bit colour.
        self.command(cmd::MADCTL, &[TFT_MADCTL]);
        self.command(cmd::COLMOD, &[0x05]);
        // Gamma curves.
        self.command(
            cmd::GMCTRP1,
            &[
                0x02, 0x1C, 0x07, 0x12, 0x37, 0x32, 0x29, 0x2D, 0x29, 0x25, 0x2B, 0x39, 0x00,
                0x01, 0x03, 0x10,
            ],
        );
        self.command(
            cmd::GMCTRN1,
            &[
                0x03, 0x1D, 0x07, 0x06, 0x2E, 0x2C, 0x29, 0x2D, 0x2E, 0x2E, 0x37, 0x3F, 0x00,
                0x00, 0x02, 0x10,
            ],
        );
        self.command(cmd::NORON, &[]);
        delay.delay_ms(10);
        self.command(cmd::DISPON, &[]);
        delay.delay_ms(100);
    }

    fn hardware_reset<D: DelayNs>(&mut self, delay: &mut D) {
        let _ = self.rst.set_high();
        delay.delay_ms(10);
        let _ = self.rst.set_low();
        delay.delay_ms(10);
        let _ = self.rst.set_high();
        delay.delay_ms(120);
    }

    /// Send one command byte, then `data` with the D/C line high.
    fn command(&mut self, command: u8, data: &[u8]) {
        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        let _ = self.spi.write(&[command]);
        // Wait for the byte to leave the FIFO before D/C changes.
        let _ = self.spi.flush();
        if !data.is_empty() {
            let _ = self.dc.set_high();
            let _ = self.spi.write(data);
            let _ = self.spi.flush();
        }
        let _ = self.cs.set_high();
    }

    /// Select the rectangle that the next `RAMWR` fills. Bounds are inclusive.
    pub fn set_window(&mut self, x0: u8, y0: u8, x1: u8, y1: u8) {
        self.command(cmd::CASET, &[0x00, x0, 0x00, x1]);
        self.command(cmd::RASET, &[0x00, y0, 0x00, y1]);
    }

    /// Send a complete frame of big-endian RGB565 pixels, blocking.
    /// The panic handler uses this path: it needs no interrupts.
    pub fn write_frame(&mut self, frame: &[u8; BYTES]) {
        self.set_window(0, 0, (WIDTH - 1) as u8, (HEIGHT - 1) as u8);
        self.command(cmd::RAMWR, frame);
    }

    /// Invert all colours on the panel.
    #[allow(dead_code)]
    pub fn set_inverted(&mut self, on: bool) {
        self.command(if on { cmd::INVON } else { cmd::INVOFF }, &[]);
    }
}

impl<SPI, CS, DC, RST> St7735<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8> + embedded_hal_async::spi::SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    /// Send a complete frame with DMA. The CPU is free while it transfers,
    /// so other tasks run during the 10 ms the frame takes.
    pub async fn write_frame_async(&mut self, frame: &[u8; BYTES]) {
        self.set_window(0, 0, (WIDTH - 1) as u8, (HEIGHT - 1) as u8);
        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        let _ = SpiBus::write(&mut self.spi, &[cmd::RAMWR]);
        let _ = SpiBus::flush(&mut self.spi);
        let _ = self.dc.set_high();
        let _ = embedded_hal_async::spi::SpiBus::write(&mut self.spi, frame).await;
        let _ = embedded_hal_async::spi::SpiBus::flush(&mut self.spi).await;
        let _ = self.cs.set_high();
    }
}

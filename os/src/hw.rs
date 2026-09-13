//! Concrete hardware handles shared by the shell and the apps, plus the
//! interrupt bindings that several drivers share.

use embassy_rp::bind_interrupts;
use embassy_rp::dma;
use embassy_rp::peripherals::{DMA_CH0, USB};
#[cfg(feature = "wifi")]
use embassy_rp::peripherals::{DMA_CH1, PIO0};
#[cfg(feature = "wifi")]
use embassy_rp::pio;
use embassy_rp::usb;

use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Async, Spi};

use crate::drivers::dimmer::Dimmer;
use crate::drivers::module::Module;
use crate::drivers::power::Power;
use crate::drivers::st7735::St7735;

/// One PWM slice driving a single pin. The whole slice handle is kept, not a
/// split channel: dropping the slice handle disables the slice.
pub type Pwm = embassy_rp::pwm::Pwm<'static>;

/// The display: ST7735 on SPI0 with DMA, plus its three control lines.
pub type Display = St7735<Spi<'static, SPI0, Async>, Output<'static>, Output<'static>, Output<'static>>;

// DMA channel 0 moves frames to the display. Channel 1 and PIO0 serve the
// radio on a Pico W. One binding covers all of them.
// USB carries the debug log as a serial port.
#[cfg(feature = "wifi")]
bind_interrupts!(pub struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});
#[cfg(not(feature = "wifi"))]
bind_interrupts!(pub struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});

/// Everything an app may touch besides the framebuffer, buttons, network
/// and storage.
pub struct Hardware {
    /// Plain Pico or Pico W, detected at boot.
    pub module: Module,
    pub backlight: Dimmer<Pwm>,
    pub led_left: Dimmer<Pwm>,
    pub led_right: Dimmer<Pwm>,
    pub power: Power,
    /// False once a DMA frame transfer failed and the OS fell back to
    /// blocking transfers.
    pub display_dma: bool,
}

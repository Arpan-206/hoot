//! Detect which Pico module the Sprig carries: a plain Pico or a Pico W.
//!
//! The two boards wire three pins differently:
//!
//! | GPIO | Pico              | Pico W                              |
//! | ---- | ----------------- | ----------------------------------- |
//! | 23   | SMPS power-save   | WL_ON: wireless chip power           |
//! | 24   | VBUS sense        | WL_D: wireless SPI data              |
//! | 25   | on-board LED      | WL_CS: wireless SPI chip select      |
//! | 29   | ADC3 = VSYS / 3   | ADC3 = VSYS / 3 and wireless SPI CLK |
//!
//! On a Pico W the wireless chip holds GPIO29 low while WL_CS is low, so
//! ADC3 reads close to zero. On a plain Pico ADC3 always shows VSYS / 3,
//! which is never below about 600 counts while the board is running.
//! Raspberry Pi documents this check in "Connecting to the Internet with
//! Pico W", section 2.4. The arduino-pico core uses it too.

use cortex_m::asm::delay;
use embassy_rp::Peri;
use embassy_rp::adc::{Adc, Blocking, Channel};
use embassy_rp::gpio::{Flex, Pull};
use embassy_rp::peripherals::{PIN_25, PIN_29};

use crate::drivers::power::sample_vsys;

/// ADC3 counts below this while WL_CS is low mean a Pico W.
const PICO_W_ADC_MAX: u16 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Module {
    Pico,
    PicoW,
}

impl Module {
    pub const fn name(self) -> &'static str {
        match self {
            Module::Pico => "Pico",
            Module::PicoW => "Pico W",
        }
    }
}

/// Probe the board once at boot. Borrows the two pins and gives them back
/// in their reset state, so the caller can hand them to the radio or the
/// power monitor afterwards.
pub fn detect(
    adc: &mut Adc<'_, Blocking>,
    gpio29: Peri<'_, PIN_29>,
    gpio25: Peri<'_, PIN_25>,
) -> Module {
    let mut vsys = Channel::new_pin(gpio29, Pull::None);
    let mut cs = Flex::new(gpio25);
    cs.set_pull(Pull::Down);
    cs.set_as_input();
    delay(2_000); // let the pad settle, about 16 us

    // A plain Pico drives an LED from this pin, so it can never read high.
    // Something pulling it up means a Pico W with WL_CS held inactive.
    let cs_reads_high = cs.is_high();
    let adc_with_cs_low = sample_vsys(adc, &mut vsys);

    if cs_reads_high || adc_with_cs_low < PICO_W_ADC_MAX {
        Module::PicoW
    } else {
        Module::Pico
    }
}

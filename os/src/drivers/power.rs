//! USB detection and battery voltage.
//!
//! GP29 reads VSYS through a 3:1 divider on the Pico module, on ADC channel 3.
//! GP24 is high when USB power is present. Both pins belong to the radio on
//! a Pico W, so there the reading is simply unknown.

use embassy_rp::adc::{Adc, Blocking, Channel};
use embassy_rp::gpio::Input;

pub type VsysChannel = Channel<'static>;
pub type VbusPin = Input<'static>;

/// Two AAA cells: about 3.1 V when fresh, about 2.0 V when empty.
const BATTERY_FULL_MV: u32 = 3100;
const BATTERY_EMPTY_MV: u32 = 2000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerStatus {
    pub usb: bool,
    pub vsys_mv: u16,
    /// False when the board cannot measure (Pico W). Other fields are then zero.
    pub known: bool,
}

impl PowerStatus {
    /// Rough remaining charge, 0 to 100. Meaningless while on USB.
    pub fn battery_percent(&self) -> u8 {
        let mv = (self.vsys_mv as u32).clamp(BATTERY_EMPTY_MV, BATTERY_FULL_MV);
        ((mv - BATTERY_EMPTY_MV) * 100 / (BATTERY_FULL_MV - BATTERY_EMPTY_MV)) as u8
    }
}

/// Read ADC3 in raw counts (0 to 4095). Discards one sample after the input
/// mux switches, then averages four.
pub fn sample_vsys(adc: &mut Adc<'_, Blocking>, vsys: &mut Channel<'_>) -> u16 {
    let _ = adc.blocking_read(vsys);
    let mut sum: u32 = 0;
    for _ in 0..4 {
        sum += adc.blocking_read(vsys).unwrap_or(0) as u32;
    }
    (sum / 4) as u16
}

pub struct Power {
    adc: Adc<'static, Blocking>,
    vsys: Option<VsysChannel>,
    vbus: Option<VbusPin>,
}

impl Power {
    /// Pass `None` for both on a Pico W, where the radio owns the pins.
    pub fn new(adc: Adc<'static, Blocking>, vsys: Option<VsysChannel>, vbus: Option<VbusPin>) -> Self {
        Self { adc, vsys, vbus }
    }

    /// Take a fresh reading. Costs a few microseconds.
    pub fn read(&mut self) -> PowerStatus {
        let Some(vsys) = self.vsys.as_mut() else {
            return PowerStatus::default();
        };
        let raw = sample_vsys(&mut self.adc, vsys) as u32;
        // 12-bit result, 3.3 V reference, divided by 3 on the board.
        let vsys_mv = (raw * 3 * 3300 / 4095) as u16;
        let usb = self.vbus.as_ref().is_some_and(|pin| pin.is_high());
        PowerStatus { usb, vsys_mv, known: true }
    }
}

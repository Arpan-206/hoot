//! The eight Sprig buttons: debouncing, edge detection and key repeat.
//!
//! Call [`Input::poll`] once per frame. Everything else is a pure read.

use embassy_rp::gpio::Input as GpioInput;

pub type ButtonPin = GpioInput<'static>;

/// Ignore state changes for this long after a change (switch bounce).
const DEBOUNCE_MS: u32 = 20;
/// Key repeat starts after this long ...
const REPEAT_DELAY_MS: u32 = 400;
/// ... and fires this often while the button stays down.
const REPEAT_RATE_MS: u32 = 90;

/// One of the eight buttons. The value is the bit index in the state masks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Button {
    W = 0,
    A = 1,
    S = 2,
    D = 3,
    I = 4,
    J = 5,
    K = 6,
    L = 7,
}

impl Button {
    pub const ALL: [Button; 8] = [
        Button::W,
        Button::A,
        Button::S,
        Button::D,
        Button::I,
        Button::J,
        Button::K,
        Button::L,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Button::W => "W",
            Button::A => "A",
            Button::S => "S",
            Button::D => "D",
            Button::I => "I",
            Button::J => "J",
            Button::K => "K",
            Button::L => "L",
        }
    }

    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

pub struct Input {
    pins: [ButtonPin; 8],
    /// Debounced state, one bit per button, 1 = down.
    stable: u8,
    /// `stable` from the previous poll.
    prev: u8,
    /// Buttons that produced a repeat event this poll (includes the first press).
    repeat: u8,
    last_change_ms: [u32; 8],
    held_since_ms: [u32; 8],
    last_repeat_ms: [u32; 8],
}

impl Input {
    /// `pins` must be in [`Button::ALL`] order, configured as pull-up inputs.
    pub fn new(pins: [ButtonPin; 8]) -> Self {
        Self {
            pins,
            stable: 0,
            prev: 0,
            repeat: 0,
            last_change_ms: [0; 8],
            held_since_ms: [0; 8],
            last_repeat_ms: [0; 8],
        }
    }

    /// Sample all buttons. Call once per frame with the time in milliseconds.
    pub fn poll(&mut self, now_ms: u32) {
        self.prev = self.stable;
        self.repeat = 0;
        for (i, pin) in self.pins.iter().enumerate() {
            let bit = 1u8 << i;
            // Buttons pull the line to ground when pressed.
            let down = pin.is_low();
            let was_down = self.stable & bit != 0;

            if down != was_down {
                if now_ms.wrapping_sub(self.last_change_ms[i]) >= DEBOUNCE_MS {
                    self.last_change_ms[i] = now_ms;
                    if down {
                        self.stable |= bit;
                        self.held_since_ms[i] = now_ms;
                        self.last_repeat_ms[i] = now_ms;
                        self.repeat |= bit;
                    } else {
                        self.stable &= !bit;
                    }
                }
            } else if down {
                let held_for = now_ms.wrapping_sub(self.held_since_ms[i]);
                let since_repeat = now_ms.wrapping_sub(self.last_repeat_ms[i]);
                if held_for >= REPEAT_DELAY_MS && since_repeat >= REPEAT_RATE_MS {
                    self.last_repeat_ms[i] = now_ms;
                    self.repeat |= bit;
                }
            }
        }
    }

    /// True while the button is down.
    pub fn held(&self, b: Button) -> bool {
        self.stable & b.bit() != 0
    }

    /// True only on the poll where the button went down.
    pub fn just_pressed(&self, b: Button) -> bool {
        (self.stable & !self.prev) & b.bit() != 0
    }

    /// True only on the poll where the button came up.
    #[allow(dead_code)]
    pub fn just_released(&self, b: Button) -> bool {
        (self.prev & !self.stable) & b.bit() != 0
    }

    /// True on the first press and then periodically while held. Use for menus.
    pub fn repeat(&self, b: Button) -> bool {
        self.repeat & b.bit() != 0
    }

    /// Bit mask of buttons that went down this poll.
    pub fn just_pressed_mask(&self) -> u8 {
        self.stable & !self.prev
    }

    /// Bit mask of buttons currently down.
    pub fn held_mask(&self) -> u8 {
        self.stable
    }

    /// Forget this poll's presses and repeats, so apps do not act on a key
    /// that only served to wake the screen.
    pub fn swallow(&mut self) {
        self.prev = self.stable;
        self.repeat = 0;
    }
}

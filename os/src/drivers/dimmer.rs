//! A PWM output with an 8-bit brightness and a perceptual curve.
//! Used for the backlight and the two white LEDs.

use embedded_hal::pwm::SetDutyCycle;

pub struct Dimmer<C: SetDutyCycle> {
    channel: C,
    level: u8,
}

impl<C: SetDutyCycle> Dimmer<C> {
    pub fn new(channel: C, level: u8) -> Self {
        let mut dimmer = Self { channel, level };
        dimmer.apply();
        dimmer
    }

    /// Brightness from 0 (off) to 255 (full).
    pub fn set(&mut self, level: u8) {
        self.level = level;
        self.apply();
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Change the brightness by `delta`, clamped to `min..=255`.
    pub fn adjust(&mut self, delta: i16, min: u8) {
        let next = (self.level as i16 + delta).clamp(min as i16, 255) as u8;
        self.set(next);
    }

    fn apply(&mut self) {
        // Square the level so low settings look dim instead of half-bright.
        let max = self.channel.max_duty_cycle() as u32;
        let l = self.level as u32;
        let duty = (l * l * max / (255 * 255)) as u16;
        let _ = self.channel.set_duty_cycle(duty);
    }
}

//! Sounds through the MAX98357A amplifier on the I2S pins, driven by PIO1
//! and one DMA channel. The OS owns the speaker the way it owns the LEDs:
//! an app asks for a named sound with [`play`] and carries on. One sound
//! plays at a time; a new request replaces one still playing.
//!
//! Tones are made on the fly at 24 kHz, 16-bit, both channels the same.
//! The speaker is small, so the notes sit high, the wave is a rounded
//! square, and the level holds for most of each note: all three make it
//! carry further than a soft sine would.
//! Between sounds the state machine stops, which also mutes the amp. The
//! level, 0 to 10, comes from the config (`sound`) through [`set_volume`].

use core::mem;

use embassy_rp::Peri;
use embassy_rp::peripherals::{DMA_CH2, PIN_9, PIN_10, PIN_11, PIO1};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use portable_atomic::{AtomicU8, Ordering};
use sprig_proto::record::{SOUND_MAX, SOUND_OFF};

use crate::hw::Irqs;

const SAMPLE_RATE: u32 = 24_000;
const BIT_DEPTH: u32 = 16;
/// Stereo frames per DMA buffer: about 11 ms of sound.
const CHUNK: usize = 256;

/// The sounds the OS knows. Apps pick one; the mix lives here so every
/// device sounds the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    /// A short click: a button did something.
    Tick,
    /// Rising chime: a work session is done.
    Done,
    /// Falling chime: the break is over.
    Rest,
    /// Three loud tones at full scale, for the speaker test. Plays even
    /// when sounds are off.
    Test,
    /// Two quick notes, like picking up a coin.
    Coin,
    /// A fast falling zap.
    Laser,
    /// The first Westminster quarter, up an octave for the small speaker.
    Bell,
    /// Two tones alternating.
    Siren,
    /// Urgent beeps, for the alarm.
    Alarm,
}

/// The tones an alarm can use, by name.
pub const TUNES: [(&str, Sound); 4] =
    [("Beeps", Sound::Alarm), ("Bell", Sound::Bell), ("Siren", Sound::Siren), ("Chime", Sound::Done)];

/// One note: frequency in Hz and length in ms. Frequency 0 is a rest.
type Note = (u16, u16);

impl Sound {
    fn notes(self) -> &'static [Note] {
        match self {
            Sound::Tick => &[(2000, 40)],
            Sound::Done => &[(1047, 120), (1319, 120), (1568, 120), (2093, 300)],
            Sound::Rest => &[(1568, 150), (1319, 150), (1047, 320)],
            Sound::Test => &[(440, 500), (1000, 500), (2000, 500)],
            Sound::Coin => &[(988, 60), (1319, 350)],
            Sound::Laser => &[
                (3000, 25),
                (2500, 25),
                (2100, 25),
                (1750, 25),
                (1450, 25),
                (1200, 25),
                (950, 25),
                (700, 60),
            ],
            Sound::Bell => &[
                (1319, 250),
                (1047, 250),
                (1175, 250),
                (784, 500),
                (0, 150),
                (784, 250),
                (1175, 250),
                (1319, 250),
                (1047, 500),
            ],
            Sound::Siren => &[(800, 180), (1100, 180), (800, 180), (1100, 180), (800, 180), (1100, 180)],
            Sound::Alarm => &[
                (2000, 80),
                (0, 60),
                (2000, 80),
                (0, 60),
                (2000, 80),
                (0, 300),
                (2000, 80),
                (0, 60),
                (2000, 80),
                (0, 60),
                (2000, 80),
            ],
        }
    }

    /// How long the sound lasts.
    pub fn len_ms(self) -> u32 {
        self.notes().iter().map(|n| n.1 as u32).sum()
    }
}

static REQUEST: Signal<CriticalSectionRawMutex, Sound> = Signal::new();
static VOLUME: AtomicU8 = AtomicU8::new(SOUND_OFF);

/// Ask for a sound. Returns at once. Nothing happens when sounds are off.
pub fn play(sound: Sound) {
    if sound == Sound::Test || VOLUME.load(Ordering::Relaxed) != SOUND_OFF {
        REQUEST.signal(sound);
    }
}

/// Set the level, 0 (off) to 10.
pub fn set_volume(level: u8) {
    VOLUME.store(level.min(SOUND_MAX), Ordering::Relaxed);
}

/// Peak sample value per level. Full scale is 32767; the steps are about
/// 3 dB apart at the top and wider at the bottom, which sounds even.
const LEVELS: [i32; SOUND_MAX as usize + 1] =
    [0, 1_500, 2_200, 3_200, 4_600, 6_500, 9_000, 12_500, 17_000, 23_000, 30_000];

fn amplitude() -> i32 {
    LEVELS[VOLUME.load(Ordering::Relaxed) as usize]
}

/// A rounded square wave from a 16-bit phase, peak 16384: a parabolic
/// sine doubled and clipped. Louder than a sine at the same peak, softer
/// than a hard square, and it needs no table.
fn wave(phase: u16) -> i32 {
    let half = (phase & 0x7FFF) as i32;
    let y = ((half * (0x8000 - half)) >> 13).min(16_384);
    if phase & 0x8000 != 0 { -y } else { y }
}

pub struct Pins {
    pub pio: Peri<'static, PIO1>,
    pub dma: Peri<'static, DMA_CH2>,
    pub din: Peri<'static, PIN_9>,
    pub bclk: Peri<'static, PIN_10>,
    pub lrclk: Peri<'static, PIN_11>,
}

#[embassy_executor::task]
pub async fn audio_task(pins: Pins) {
    let Pio { mut common, sm0, .. } = Pio::new(pins.pio, Irqs);
    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        pins.dma,
        Irqs,
        pins.din,
        pins.bclk,
        pins.lrclk,
        SAMPLE_RATE,
        BIT_DEPTH,
        &program,
    );
    let mut buffers = [[0u32; CHUNK]; 2];
    let (mut front, mut back) = buffers.split_at_mut(1);
    info!("audio: up, {} Hz, level {}", SAMPLE_RATE, VOLUME.load(Ordering::Relaxed));

    loop {
        let sound = REQUEST.wait().await;
        let mut synth = Synth::new(sound);
        info!("audio: play {:?} at {}", sound, synth.amp);
        i2s.start();
        let mut chunks = 0u32;
        // Keep one chunk in flight while the next is filled.
        synth.fill(&mut front[0]);
        while !synth.done() {
            let transfer = i2s.write(&front[0]);
            synth.fill(&mut back[0]);
            transfer.await;
            chunks += 1;
            mem::swap(&mut front, &mut back);
            if REQUEST.signaled() {
                break; // a new sound takes over
            }
        }
        // The last chunk, then silence so the amp does not click off.
        i2s.write(&front[0]).await;
        back[0].fill(0);
        i2s.write(&back[0]).await;
        i2s.stop();
        info!("audio: done, {} chunks", chunks + 2);
    }
}

/// Plays one sound's notes with a short attack and a decay to silence.
struct Synth {
    notes: &'static [Note],
    index: usize,
    /// Samples left in the current note.
    left: u32,
    /// Samples in the whole current note, for the envelope.
    total: u32,
    phase: u16,
    step: u16,
    amp: i32,
    done: bool,
}

impl Synth {
    fn new(sound: Sound) -> Self {
        let mut s = Self {
            notes: sound.notes(),
            index: 0,
            left: 0,
            total: 0,
            phase: 0,
            step: 0,
            amp: if sound == Sound::Test { 28_000 } else { amplitude() },
            done: false,
        };
        s.load(0);
        s
    }

    fn load(&mut self, index: usize) {
        match self.notes.get(index) {
            Some(&(hz, ms)) => {
                self.index = index;
                self.total = SAMPLE_RATE * ms as u32 / 1000;
                self.left = self.total;
                self.step = (hz as u32 * 65_536 / SAMPLE_RATE) as u16;
                self.phase = 0;
            }
            None => self.done = true,
        }
    }

    fn done(&self) -> bool {
        self.done
    }

    fn fill(&mut self, buf: &mut [u32; CHUNK]) {
        for frame in buf.iter_mut() {
            *frame = self.next();
        }
    }

    /// One stereo frame: the same 16-bit sample in both halves.
    fn next(&mut self) -> u32 {
        if self.left == 0 && !self.done {
            self.load(self.index + 1);
        }
        if self.done {
            return 0;
        }
        // 4 ms attack, hold, then a release over the last quarter of the
        // note, at most 60 ms.
        const ATTACK: u32 = SAMPLE_RATE / 250;
        let release = (self.total / 4).clamp(1, SAMPLE_RATE * 60 / 1000);
        let played = self.total - self.left;
        let env = if played < ATTACK {
            played * 256 / ATTACK
        } else if self.left < release {
            self.left * 256 / release
        } else {
            256
        };
        let sample = (((wave(self.phase) * self.amp) >> 14) * env as i32) >> 8;
        self.phase = self.phase.wrapping_add(self.step);
        self.left -= 1;
        let s = sample as i16 as u16 as u32;
        (s << 16) | s
    }
}

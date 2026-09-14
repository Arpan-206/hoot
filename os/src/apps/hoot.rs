//! Hoot, the owl. The device's companion rather than one app among many:
//! it lives in the background whatever is on screen, so it gets hungry,
//! plays, and sleeps at night by the clock. Its screen is the first entry
//! of the menu, and the menu comes back to it after a minute of quiet.
//!
//! Family can feed it and play with it from the web page. Those arrive as
//! server commands and land here through [`request`]; the heartbeat
//! reports its mood through [`mood`]. State is saved to the config record
//! after care and every ten minutes, and caught up after time off once
//! the clock is known.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::pet::{Mood, Pet, Stage, TICK_MIN};
use portable_atomic::{AtomicU8, Ordering};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::clock;
use crate::drivers::input::Button;
use crate::ui::owl;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Hoot", group: Group::System, needs_network: false };

const TICK_MS: u32 = TICK_MIN * 60_000;
const SAVE_MS: u32 = 10 * 60_000;
const SAVE_GAP_MS: u32 = 2_000;
const REACT_MS: u32 = 1_600;
/// Stay awake this long after someone cares for it, even at night.
const AWAKE_MS: u32 = 60_000;
const NIGHT_FROM: u8 = 22;
const NIGHT_TO: u8 = 7;

/// Care asked for from the server, as bits.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Care {
    Feed = 1,
    Play = 2,
}

static REQUEST: AtomicU8 = AtomicU8::new(0);
static MOOD: AtomicU8 = AtomicU8::new(Mood::Content as u8);

/// Ask for care on Hoot's behalf, from anywhere. Applied next frame.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn request(care: Care) {
    REQUEST.fetch_or(care as u8, Ordering::Relaxed);
}

/// Hoot's mood as of the last frame, for the heartbeat and the menu.
pub fn mood() -> Mood {
    Mood::from_u8(MOOD.load(Ordering::Relaxed))
}

/// Short word for the menu when Hoot needs something, or is asleep.
pub fn badge() -> Option<&'static str> {
    match mood() {
        Mood::Sleeping => Some("zzz"),
        m if m.needs_care() => Some(m.label()),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reaction {
    Yum,
    Full,
    Hearts,
    TooTired,
}

impl Reaction {
    const fn text(self) -> &'static str {
        match self {
            Reaction::Yum => "yum!",
            Reaction::Full => "full",
            Reaction::Hearts => "<3 <3",
            Reaction::TooTired => "too tired",
        }
    }
}

/// What the last frame showed: mood, eyes shut, bob, reaction, the three
/// needs, and the snore phase.
type DrawKey = (Mood, bool, i32, Option<Reaction>, u8, u8, u8, u32);

pub struct HootApp {
    pet: Pet,
    loaded: bool,
    caught_up: bool,
    asleep: bool,
    awake_until_ms: u32,
    last_ms: u32,
    acc_ms: u32,
    dirty: bool,
    save_soon: bool,
    last_save_ms: u32,
    blink_until_ms: u32,
    next_blink_ms: u32,
    reaction: Option<(Reaction, u32)>,
    drawn: Option<DrawKey>,
}

impl HootApp {
    pub const fn new() -> Self {
        Self {
            pet: Pet::new(),
            loaded: false,
            caught_up: false,
            asleep: false,
            awake_until_ms: 0,
            last_ms: 0,
            acc_ms: 0,
            dirty: false,
            save_soon: false,
            last_save_ms: 0,
            blink_until_ms: 0,
            next_blink_ms: 0,
            reaction: None,
            drawn: None,
        }
    }

    fn night(now_ms: u32) -> bool {
        clock::now(now_ms).is_some_and(|c| c.hour >= NIGHT_FROM || c.hour < NIGHT_TO)
    }

    /// Time passes. Runs every frame, on screen or not.
    fn simulate(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        if !self.loaded {
            self.pet = ctx.store.config().pet;
            self.loaded = true;
            self.last_ms = now;
            self.last_save_ms = now;
            self.next_blink_ms = now.wrapping_add(3_000);
            info!("hoot: hunger {} happy {} energy {}", self.pet.hunger, self.pet.happy, self.pet.energy);
        }
        let dt = now.wrapping_sub(self.last_ms);
        self.last_ms = now;
        self.acc_ms = self.acc_ms.saturating_add(dt);
        let night = Self::night(now);

        // Once the clock is known: hatch, and catch up on the time off.
        if !self.caught_up && let Some(secs) = clock::now_secs(now) {
            self.caught_up = true;
            if self.pet.born == 0 {
                self.pet.born = secs;
                info!("hoot: hatched");
            }
            if self.pet.seen != 0 && secs > self.pet.seen {
                let minutes = (secs - self.pet.seen) / 60;
                self.pet.advance(minutes, night);
                info!("hoot: caught up {} min", minutes.min(24 * 60));
            }
            self.pet.seen = secs;
            self.dirty = true;
        }

        if now.wrapping_sub(self.awake_until_ms) >= 1 << 31 {
            self.asleep = false;
        } else {
            self.asleep = self.pet.sleep_state(self.asleep, night);
        }
        while self.acc_ms >= TICK_MS {
            self.acc_ms -= TICK_MS;
            self.pet.tick(self.asleep);
            if let Some(secs) = clock::now_secs(now) {
                self.pet.seen = secs;
            }
            self.dirty = true;
        }

        // Care from the web page.
        let asked = REQUEST.swap(0, Ordering::Relaxed);
        if asked & Care::Feed as u8 != 0 {
            info!("hoot: fed from the server");
            let r = self.feed(now);
            self.react(r, now);
        }
        if asked & Care::Play as u8 != 0 {
            info!("hoot: played with from the server");
            let r = self.play(now);
            self.react(r, now);
        }

        MOOD.store(self.pet.mood(self.asleep) as u8, Ordering::Relaxed);
        let since_save = now.wrapping_sub(self.last_save_ms);
        if (self.save_soon && since_save >= SAVE_GAP_MS) || (self.dirty && since_save >= SAVE_MS) {
            self.save(ctx);
        }
    }

    fn save(&mut self, ctx: &mut Ctx) {
        let pet = self.pet;
        let _ = ctx.store.update_config(|c| c.pet = pet);
        self.dirty = false;
        self.save_soon = false;
        self.last_save_ms = ctx.now_ms;
    }

    fn wake(&mut self, now: u32) {
        self.asleep = false;
        self.awake_until_ms = now.wrapping_add(AWAKE_MS);
    }

    fn feed(&mut self, now: u32) -> Reaction {
        self.wake(now);
        self.dirty = true;
        self.save_soon = true;
        if self.pet.feed() {
            audio::play(Sound::Coin);
            Reaction::Yum
        } else {
            audio::play(Sound::Rest);
            Reaction::Full
        }
    }

    fn play(&mut self, now: u32) -> Reaction {
        self.wake(now);
        self.dirty = true;
        self.save_soon = true;
        if self.pet.play() {
            audio::play(Sound::Done);
            Reaction::Hearts
        } else {
            audio::play(Sound::Rest);
            Reaction::TooTired
        }
    }

    fn react(&mut self, r: Reaction, now: u32) {
        self.reaction = Some((r, now.wrapping_add(REACT_MS)));
    }
}

impl App for HootApp {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.simulate(ctx);
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let input = ctx.input;
        if input.just_pressed(Button::L) {
            let r = self.feed(now);
            self.react(r, now);
        }
        if input.just_pressed(Button::K) {
            let r = self.play(now);
            self.react(r, now);
        }
        if input.just_pressed(Button::D) {
            self.wake(now);
            self.pet.stroke();
            self.dirty = true;
            audio::play(Sound::Tick);
            self.react(Reaction::Hearts, now);
        }
        if let Some((_, until)) = self.reaction
            && now.wrapping_sub(until) < 1 << 31
        {
            self.reaction = None;
        }

        // Blink now and then, unless asleep.
        if now.wrapping_sub(self.next_blink_ms) < 1 << 31 {
            self.blink_until_ms = now.wrapping_add(150);
            self.next_blink_ms = now.wrapping_add(2_500 + now % 3_000);
        }
        let blinking = now.wrapping_sub(self.blink_until_ms) >= 1 << 31;
        let mood = self.pet.mood(self.asleep);
        let bob = if mood == Mood::Happy && (now / 400).is_multiple_of(2) { -2 } else { 0 };
        let phase = if self.asleep { now / 600 % 3 } else { 0 };
        let key = (mood, self.asleep || blinking, bob, self.reaction.map(|r| r.0), self.pet.hunger, self.pet.happy, self.pet.energy, phase);
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            draw(ctx.fb, &self.pet, mood, self.asleep || blinking, bob, self.reaction.map(|r| r.0), phase, now);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        if self.dirty {
            self.save(ctx);
        }
    }

    fn background(&mut self, ctx: &mut Ctx) {
        self.simulate(ctx);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(fb: &mut Framebuffer, pet: &Pet, mood: Mood, eyes_shut: bool, bob: i32, reaction: Option<Reaction>, phase: u32, now: u32) {
    let secs = clock::now_secs(now);
    let stage = pet.stage(secs);
    let right: StrBuf<24> = match pet.age_days(secs) {
        Some(d) => format(format_args!("{}, {} days", stage.label(), d)),
        None => format(format_args!("{}", stage.label())),
    };
    theme::screen(fb, "Hoot", right.as_str());

    let scale = if stage == Stage::Owlet { 2 } else { 3 };
    let size = owl::SIZE * scale;
    let x = (WIDTH - size) / 2;
    let y = 20 + (48 - size) / 2 + bob;
    owl::draw(fb, x, y, scale, eyes_shut);

    // A word next to the owl: a reaction, snores, or what it wants.
    let bx = x + size + 6;
    let by = y + 6;
    if let Some(r) = reaction {
        fb.draw_text(bx, by, r.text(), theme::ACCENT, None);
    } else if mood == Mood::Sleeping {
        let z = ["z", "z z", "z z z"][phase as usize % 3];
        fb.draw_text(bx, by, z, theme::MUTED, None);
    } else if mood.needs_care() {
        fb.draw_text(bx, by, "!", theme::WARN, None);
    }

    let line: StrBuf<24> = format(format_args!("Hoot is {}", mood.label()));
    fb.draw_text_centered(72, line.as_str(), if mood.needs_care() { theme::WARN } else { theme::TEXT }, None, 1);

    for (i, (label, value)) in [("Full", 100 - pet.hunger), ("Happy", pet.happy), ("Energy", pet.energy)].iter().enumerate() {
        let y = 84 + i as i32 * 10;
        fb.draw_text(4, y - 1, label, theme::MUTED, None);
        let color = if *value < 30 { theme::WARN } else { theme::ACCENT };
        theme::meter(fb, 48, y, 106, 7, (*value as u32 * 255 / 100) as u8, color);
    }
    theme::footer(fb, "L feed   K play   D pet   J back");
}

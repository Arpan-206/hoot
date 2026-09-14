//! Hoot, the owl: a companion in the spirit of Finch. Not a mouth to
//! feed. It grows when you look after yourself: a daily check-in and a
//! handful of small self-care goals give it energy, a full bar sends it
//! on an adventure, and adventures make it grow. Nothing is lost for a
//! missed day. It also breathes with you for a minute when asked.
//!
//! Hoot lives in the background whatever is on screen, so a finished
//! Pomodoro session counts as the focus goal, and a hug sent from the web
//! page arrives as a server command. Its screen is the first menu entry,
//! and the menu returns to it after a quiet minute. State is saved to the
//! config record after each change and every ten minutes.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::pet::{GOAL_BREATHE, GOAL_CHECKIN, GOALS, Pet, Stage, mood_word, reply};
use hoot_proto::time::days_from_secs;
use portable_atomic::{AtomicBool, AtomicU8, Ordering};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::clock;
use crate::drivers::input::Button;
use crate::ui::owl;
use crate::ui::splash;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Hoot", group: Group::System, needs_network: false };

const SAVE_MS: u32 = 10 * 60_000;
const SAVE_GAP_MS: u32 = 2_000;
/// How long a line from Hoot stays up.
const SAY_MS: u32 = 4_000;
/// Stay awake this long after a key press, even at night.
const AWAKE_MS: u32 = 60_000;
const NIGHT_FROM: u8 = 22;
const NIGHT_TO: u8 = 7;
/// Breathing: in, hold, out, hold, each this long; this many rounds.
const BREATH_PHASE_MS: u32 = 4_000;
const BREATH_ROUNDS: u32 = 4;
/// Adventure: fly out, time away, fly back, then the discovery.
const FLY_OUT_MS: u32 = 1_500;
const AWAY_MS: u32 = 2_500;
const FLY_IN_MS: u32 = 1_500;
const SHOW_MS: u32 = 6_000;

const STAGES: [Stage; 5] = [Stage::Hatchling, Stage::Owlet, Stage::Fledgling, Stage::Owl, Stage::Wise];

// Signals in and out, so the agent, the Pomodoro and the menu can reach
// Hoot without holding it.
static HUG: AtomicBool = AtomicBool::new(false);
static FOCUS: AtomicBool = AtomicBool::new(false);
static STAGE: AtomicU8 = AtomicU8::new(0);
static ENERGY: AtomicU8 = AtomicU8::new(0);
static GOALS_TODAY: AtomicU8 = AtomicU8::new(0);
static CHECKIN_DUE: AtomicBool = AtomicBool::new(false);
static ASLEEP: AtomicBool = AtomicBool::new(false);

/// Someone at home sent good wishes (server command "hug").
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn hug() {
    HUG.store(true, Ordering::Relaxed);
}

/// A focus session finished (from the Pomodoro).
pub fn note_focus() {
    FOCUS.store(true, Ordering::Relaxed);
}

/// One line for the heartbeat: stage, energy, goals today. No mood: that
/// stays on the device.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub fn status() -> StrBuf<48> {
    let stage = STAGES[(STAGE.load(Ordering::Relaxed) as usize).min(4)];
    format(format_args!(
        "{}, energy {}, {} goals today",
        stage.label(),
        ENERGY.load(Ordering::Relaxed),
        GOALS_TODAY.load(Ordering::Relaxed)
    ))
}

/// Short word for the menu: a gentle nudge to check in, or snores.
pub fn badge() -> Option<&'static str> {
    if ASLEEP.load(Ordering::Relaxed) {
        Some("zzz")
    } else if CHECKIN_DUE.load(Ordering::Relaxed) {
        Some("hi!")
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    CheckIn,
    Breathe,
    Adventure,
}

/// What the last frame showed.
type DrawKey = (Screen, usize, u8, bool, u8, u8, u32, Option<&'static str>);

pub struct HootApp {
    pet: Pet,
    loaded: bool,
    dirty: bool,
    save_soon: bool,
    last_save_ms: u32,
    asleep: bool,
    awake_until_ms: u32,
    screen: Screen,
    /// Selected goal row on the home screen.
    sel: usize,
    /// The face picked on the check-in screen, 1 to 5.
    face: u8,
    /// When the breathing or the adventure began.
    started_ms: u32,
    /// The adventure ended in a new stage.
    grew: bool,
    returned: bool,
    say: Option<(&'static str, u32)>,
    blink_until_ms: u32,
    next_blink_ms: u32,
    drawn: Option<DrawKey>,
}

impl HootApp {
    pub const fn new() -> Self {
        Self {
            pet: Pet::new(),
            loaded: false,
            dirty: false,
            save_soon: false,
            last_save_ms: 0,
            asleep: false,
            awake_until_ms: 0,
            screen: Screen::Home,
            sel: 0,
            face: 4,
            started_ms: 0,
            grew: false,
            returned: false,
            say: None,
            blink_until_ms: 0,
            next_blink_ms: 0,
            drawn: None,
        }
    }

    fn night(now_ms: u32) -> bool {
        clock::now(now_ms).is_some_and(|c| c.hour >= NIGHT_FROM || c.hour < NIGHT_TO)
    }

    fn say(&mut self, text: &'static str, now: u32) {
        self.say = Some((text, now.wrapping_add(SAY_MS)));
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.save_soon = true;
    }

    fn wake(&mut self, now: u32) {
        self.asleep = false;
        self.awake_until_ms = now.wrapping_add(AWAKE_MS);
    }

    /// Runs every frame, on screen or not.
    fn simulate(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        if !self.loaded {
            self.pet = ctx.store.config().pet;
            self.loaded = true;
            self.last_save_ms = now;
            self.next_blink_ms = now.wrapping_add(3_000);
            info!(
                "hoot: {} adventures, energy {}, {} goals ever",
                self.pet.adventures, self.pet.energy, self.pet.goals_total
            );
        }
        if let Some(secs) = clock::now_secs(now) {
            if self.pet.born == 0 {
                self.pet.born = secs;
                self.dirty = true;
                info!("hoot: hatched");
            }
            let day = days_from_secs(secs as i64) as u16;
            if self.pet.new_day(day) {
                self.dirty = true;
                info!("hoot: a new day");
            }
        }
        if now.wrapping_sub(self.awake_until_ms) >= 1 << 31 {
            self.asleep = false;
        } else {
            self.asleep = Self::night(now);
        }

        if FOCUS.swap(false, Ordering::Relaxed) && self.pet.complete(hoot_proto::pet::GOAL_FOCUS) {
            info!("hoot: focus session counted");
            self.say("Focus done. +20", now);
            audio::play(Sound::Coin);
            self.changed();
        }
        if HUG.swap(false, Ordering::Relaxed) {
            info!("hoot: a hug from home");
            self.pet.hug();
            self.wake(now);
            self.say("A hug from home <3", now);
            audio::play(Sound::Done);
            self.changed();
        }

        STAGE.store(STAGES.iter().position(|&s| s == self.pet.stage()).unwrap_or(0) as u8, Ordering::Relaxed);
        ENERGY.store(self.pet.energy, Ordering::Relaxed);
        GOALS_TODAY.store(self.pet.goals_today() as u8, Ordering::Relaxed);
        CHECKIN_DUE.store(self.pet.day != 0 && self.pet.checkin == 0 && !self.asleep, Ordering::Relaxed);
        ASLEEP.store(self.asleep, Ordering::Relaxed);

        let since_save = now.wrapping_sub(self.last_save_ms);
        if (self.save_soon && since_save >= SAVE_GAP_MS) || (self.dirty && since_save >= SAVE_MS) {
            self.save(ctx);
        }
    }

    fn save(&mut self, ctx: &mut Ctx) {
        if let Some(secs) = clock::now_secs(ctx.now_ms) {
            self.pet.seen = secs;
        }
        let pet = self.pet;
        let _ = ctx.store.update_config(|c| c.pet = pet);
        self.dirty = false;
        self.save_soon = false;
        self.last_save_ms = ctx.now_ms;
    }

    fn home(&mut self, ctx: &mut Ctx) -> Transition {
        let now = ctx.now_ms;
        let input = ctx.input;
        if back_pressed(input) {
            return Transition::Exit;
        }
        if input.repeat(Button::W) {
            self.sel = (self.sel + GOALS.len() - 1) % GOALS.len();
        }
        if input.repeat(Button::S) {
            self.sel = (self.sel + 1) % GOALS.len();
        }
        if input.just_pressed(Button::L) {
            match self.sel {
                GOAL_CHECKIN => {
                    if self.pet.checkin == 0 {
                        self.face = 4;
                        self.screen = Screen::CheckIn;
                    } else {
                        self.say(reply(self.pet.checkin), now);
                    }
                }
                GOAL_BREATHE => {
                    self.started_ms = now;
                    self.screen = Screen::Breathe;
                }
                goal => {
                    if self.pet.done(goal) {
                        self.pet.uncomplete(goal);
                        self.say("Taken back. No harm done.", now);
                    } else {
                        self.pet.complete(goal);
                        self.say(if self.pet.ready() { "Energy full! K to fly" } else { "+20  nice one" }, now);
                        audio::play(Sound::Coin);
                    }
                    self.changed();
                }
            }
        }
        if input.just_pressed(Button::K) && self.pet.ready() {
            let before = self.pet.stage();
            self.pet.adventure();
            self.grew = self.pet.stage() != before;
            self.returned = false;
            self.started_ms = now;
            self.screen = Screen::Adventure;
            audio::play(Sound::Laser);
            self.changed();
        }
        Transition::Stay
    }

    fn check_in(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        let input = ctx.input;
        if input.just_pressed(Button::A) && self.face > 1 {
            self.face -= 1;
        }
        if input.just_pressed(Button::D) && self.face < 5 {
            self.face += 1;
        }
        if input.just_pressed(Button::L) {
            self.pet.check_in(self.face);
            self.say(reply(self.face), now);
            audio::play(Sound::Coin);
            self.changed();
            self.screen = Screen::Home;
        }
        if input.just_pressed(Button::J) {
            self.screen = Screen::Home;
        }
    }

    fn breathe(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        let elapsed = now.wrapping_sub(self.started_ms);
        if ctx.input.just_pressed(Button::J) {
            ctx.hw.led_left.set(0);
            ctx.hw.led_right.set(0);
            self.screen = Screen::Home;
            return;
        }
        if elapsed >= BREATH_PHASE_MS * 4 * BREATH_ROUNDS {
            ctx.hw.led_left.set(0);
            ctx.hw.led_right.set(0);
            if self.pet.complete(GOAL_BREATHE) {
                self.say("+20  well done", now);
                self.changed();
            }
            audio::play(Sound::Done);
            self.screen = Screen::Home;
            return;
        }
        // The LEDs breathe too.
        let level = (breath_radius(elapsed) - 16) * 255 / 24;
        ctx.hw.led_left.set(level as u8);
        ctx.hw.led_right.set(level as u8);
    }

    fn adventure(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        let elapsed = now.wrapping_sub(self.started_ms);
        if elapsed >= FLY_OUT_MS + AWAY_MS && !self.returned {
            self.returned = true;
            audio::play(Sound::Coin);
        }
        let shown = elapsed >= FLY_OUT_MS + AWAY_MS + FLY_IN_MS;
        let done = elapsed >= FLY_OUT_MS + AWAY_MS + FLY_IN_MS + SHOW_MS;
        if done || (shown && ctx.input.just_pressed_mask() != 0) {
            self.screen = Screen::Home;
            self.say(if self.grew { "Hoot grew!" } else { "Back home, happy." }, now);
        }
    }
}

/// Ring radius during the breathing exercise, 16 to 40.
fn breath_radius(elapsed: u32) -> i32 {
    let phase = (elapsed / BREATH_PHASE_MS) % 4;
    let t = (elapsed % BREATH_PHASE_MS) as i32 * 24 / BREATH_PHASE_MS as i32;
    match phase {
        0 => 16 + t,
        1 => 40,
        2 => 40 - t,
        _ => 16,
    }
}

impl App for HootApp {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.drawn = None;
        self.screen = Screen::Home;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.simulate(ctx);
        let now = ctx.now_ms;
        if ctx.input.just_pressed_mask() != 0 {
            self.wake(now);
        }
        let result = match self.screen {
            Screen::Home => self.home(ctx),
            Screen::CheckIn => {
                self.check_in(ctx);
                Transition::Stay
            }
            Screen::Breathe => {
                self.breathe(ctx);
                Transition::Stay
            }
            Screen::Adventure => {
                self.adventure(ctx);
                Transition::Stay
            }
        };
        if result == Transition::Exit {
            return result;
        }
        if let Some((_, until)) = self.say
            && now.wrapping_sub(until) < 1 << 31
        {
            self.say = None;
        }
        if now.wrapping_sub(self.next_blink_ms) < 1 << 31 {
            self.blink_until_ms = now.wrapping_add(150);
            self.next_blink_ms = now.wrapping_add(2_500 + now % 3_000);
        }
        let eyes_shut = self.asleep || now.wrapping_sub(self.blink_until_ms) >= 1 << 31;
        let phase = match self.screen {
            Screen::Home => now / 600 % 3,
            Screen::Breathe | Screen::Adventure => now.wrapping_sub(self.started_ms) / 50,
            Screen::CheckIn => 0,
        };
        let key = (self.screen, self.sel, self.face, eyes_shut, self.pet.energy, self.pet.done_today, phase, self.say.map(|s| s.0));
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            let secs = clock::now_secs(now);
            match self.screen {
                Screen::Home => draw_home(ctx.fb, &self.pet, self.sel, eyes_shut, self.asleep, self.say.map(|s| s.0), secs, phase),
                Screen::CheckIn => draw_check_in(ctx.fb, &self.pet, self.face, eyes_shut),
                Screen::Breathe => draw_breathe(ctx.fb, &self.pet, now.wrapping_sub(self.started_ms), eyes_shut),
                Screen::Adventure => draw_adventure(ctx.fb, &self.pet, now.wrapping_sub(self.started_ms), self.grew),
            }
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.hw.led_left.set(0);
        ctx.hw.led_right.set(0);
        if self.dirty {
            self.save(ctx);
        }
    }

    fn background(&mut self, ctx: &mut Ctx) {
        self.simulate(ctx);
    }
}

fn stage_line(pet: &Pet, secs: Option<u32>) -> StrBuf<24> {
    match pet.age_days(secs) {
        Some(d) => format(format_args!("{}, day {}", pet.stage().label(), d + 1)),
        None => format(format_args!("{}", pet.stage().label())),
    }
}

fn owl_left(fb: &mut Framebuffer, pet: &Pet, eyes_shut: bool) {
    owl::draw(fb, 8, 24, 2, eyes_shut, pet.stage() == Stage::Wise);
}

#[allow(clippy::too_many_arguments)]
fn draw_home(fb: &mut Framebuffer, pet: &Pet, sel: usize, eyes_shut: bool, asleep: bool, say: Option<&str>, secs: Option<u32>, phase: u32) {
    theme::screen(fb, "Hoot", stage_line(pet, secs).as_str());
    owl_left(fb, pet, eyes_shut);
    theme::meter(fb, 6, 62, 36, 6, (pet.energy as u32 * 255 / 100) as u8, theme::ACCENT);
    let energy: StrBuf<8> = format(format_args!("{}%", pet.energy));
    fb.draw_text_centered_in(6, 42, 71, energy.as_str(), theme::MUTED);

    for (i, name) in GOALS.iter().enumerate() {
        let y = 22 + i as i32 * 10;
        if i == sel {
            fb.fill_rect(46, y - 2, WIDTH - 48, 10, theme::BAR);
        }
        let done = pet.done(i);
        if done {
            fb.fill_rect(48, y, 6, 6, theme::ACCENT);
        } else {
            fb.draw_rect(48, y, 6, 6, theme::MUTED);
        }
        let color = if done { theme::MUTED } else { theme::TEXT };
        if i == GOAL_CHECKIN && pet.checkin != 0 {
            let text: StrBuf<20> = format(format_args!("Check in: {}", mood_word(pet.checkin)));
            fb.draw_text(58, y - 1, text.as_str(), color, None);
        } else {
            fb.draw_text(58, y - 1, name, color, None);
        }
    }

    let line: &str = if let Some(s) = say {
        s
    } else if asleep {
        ["z", "z z", "z z z"][phase as usize % 3]
    } else if pet.ready() {
        "Energy full! K to fly"
    } else if pet.goals_today() >= 3 {
        "Proud of you today."
    } else if pet.checkin == 0 && pet.day != 0 {
        "Hello. How are you?"
    } else {
        "Hoot is here."
    };
    fb.draw_text(4, 96, line, if say.is_some() { theme::ACCENT } else { theme::MUTED }, None);
    theme::footer(fb, if pet.ready() { "W/S  L done  K fly  J back" } else { "W/S pick   L done   J back" });
}

/// A small face, 8 by 8, for a check-in value 1 to 5.
fn face_rows(value: u8) -> [u8; 8] {
    let mut r = [
        0b0011_1100,
        0b0100_0010,
        0b1000_0001,
        0b1010_0101,
        0b1000_0001,
        0b1000_0001,
        0b0100_0010,
        0b0011_1100,
    ];
    match value {
        1 => {
            r[4] |= 0b0001_1100; // frown middle, plus a tear
            r[5] |= 0b0010_0100;
        }
        2 => {
            r[4] |= 0b0001_1000;
            r[5] |= 0b0010_0100;
        }
        3 => r[5] |= 0b0011_1100,
        4 => {
            r[4] |= 0b0010_0100;
            r[5] |= 0b0001_1000;
        }
        _ => {
            r[4] |= 0b0010_0100;
            r[5] |= 0b0011_1100;
        }
    }
    r
}

fn draw_check_in(fb: &mut Framebuffer, pet: &Pet, face: u8, eyes_shut: bool) {
    theme::screen(fb, "Hoot", "check in");
    owl_left(fb, pet, eyes_shut);
    fb.draw_text(52, 26, "How are you today?", theme::TEXT, None);
    for v in 1..=5u8 {
        let x = 50 + (v as i32 - 1) * 22;
        let y = 44;
        let picked = v == face;
        if picked {
            fb.fill_rect(x - 3, y - 3, 22, 22, theme::ACCENT_DARK);
        }
        let color = if picked { theme::ACCENT } else { theme::MUTED };
        fb.draw_bitmap_scaled(x, y, 8, 8, &face_rows(v), color, 2);
    }
    fb.draw_text_centered_in(48, WIDTH, 74, mood_word(face), theme::ACCENT);
    fb.draw_text(52, 92, "Hoot won't tell anyone.", theme::MUTED, None);
    theme::footer(fb, "A/D pick   L me   J skip");
}

fn draw_breathe(fb: &mut Framebuffer, pet: &Pet, elapsed: u32, eyes_shut: bool) {
    let round = (elapsed / (BREATH_PHASE_MS * 4)).min(BREATH_ROUNDS - 1) + 1;
    let right: StrBuf<12> = format(format_args!("round {}/{}", round, BREATH_ROUNDS));
    theme::screen(fb, "Breathe", right.as_str());
    let (cx, cy) = (WIDTH / 2, 56);
    let r = breath_radius(elapsed);
    let (inner, outer) = ((r - 1) * (r - 1), r * r);
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 >= inner && d2 <= outer {
                fb.set(cx + dx, cy + dy, theme::ACCENT);
            }
        }
    }
    owl::draw(fb, cx - owl::SIZE, cy - owl::SIZE, 2, eyes_shut, pet.stage() == Stage::Wise);
    let text = match (elapsed / BREATH_PHASE_MS) % 4 {
        0 => "breathe in",
        1 => "hold",
        2 => "breathe out",
        _ => "hold",
    };
    fb.draw_text_centered(104, text, theme::TEXT, None, 1);
    theme::footer(fb, "J stop");
}

fn draw_adventure(fb: &mut Framebuffer, pet: &Pet, elapsed: u32, grew: bool) {
    fb.clear(theme::BG);
    for &(x, y) in &[(14, 10), (38, 22), (129, 30), (147, 12), (22, 58), (140, 66), (70, 14), (100, 40)] {
        fb.set(x, y, theme::MUTED);
    }
    splash::moon(fb, 132, 16, 8);
    let wise = pet.stage() == Stage::Wise;
    let bob = if (elapsed / 200).is_multiple_of(2) { 0 } else { -3 };
    if elapsed < FLY_OUT_MS {
        let x = 8 + (elapsed as i32 * (WIDTH + 8 - 8)) / FLY_OUT_MS as i32;
        owl::draw(fb, x, 40 + bob, 2, false, wise);
    } else if elapsed < FLY_OUT_MS + AWAY_MS {
        fb.draw_text_centered(60, "Hoot is out exploring...", theme::MUTED, None, 1);
    } else if elapsed < FLY_OUT_MS + AWAY_MS + FLY_IN_MS {
        let t = (elapsed - FLY_OUT_MS - AWAY_MS) as i32;
        let x = WIDTH - (t * (WIDTH - 64)) / FLY_IN_MS as i32;
        owl::draw(fb, x, 40 + bob, 2, false, wise);
    } else {
        owl::draw(fb, 64, 28, 2, false, wise);
        fb.draw_text_centered(70, "Hoot found", theme::MUTED, None, 1);
        fb.draw_text_centered(82, pet.discovery(), theme::ACCENT, None, 1);
        if grew {
            let line: StrBuf<32> = format(format_args!("and grew into {}!", pet.stage().label()));
            fb.draw_text_centered(96, line.as_str(), theme::WARN, None, 1);
        }
        theme::footer(fb, "any key");
    }
}

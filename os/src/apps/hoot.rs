//! Hoot, the owl: a companion in the spirit of Finch. Not a mouth to
//! feed. It grows when you look after yourself: a daily check-in and a
//! handful of small self-care goals give it energy, a full bar sends it
//! on an adventure, and adventures make it grow and earn it things to
//! wear. Nothing is lost for a missed day. It breathes with you for a
//! minute when asked, and it keeps the week: check-ins and goals per day.
//!
//! Hoot lives in the background whatever is on screen, so a finished
//! Pomodoro session counts as the focus goal, a note from home gets a
//! mention, and a hug sent from the web page arrives as a server command.
//! Its screen is the first menu entry and the menu returns to it after a
//! quiet minute. It hatches from an egg the first time. State is saved to
//! the config record after each change and every ten minutes.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::pet::{GOAL_BREATHE, GOAL_CHECKIN, GOAL_FOCUS, GOALS, Outfit, Pet, Stage, greeting, line_of_day, mood_word, reply};
use hoot_proto::time::{WEEKDAYS, days_from_secs};
use portable_atomic::{AtomicBool, AtomicU8, Ordering};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::clock;
use crate::drivers::input::Button;
use crate::ui::owl::{self, Look};
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
/// Idle life: a glance every few seconds, a flutter now and then, a hop
/// when a goal is ticked.
const GLANCE_MS: u32 = 700;
const FLAP_MS: u32 = 400;
const HOP_MS: u32 = 260;

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

fn unread_notes() -> u8 {
    #[cfg(feature = "wifi")]
    {
        crate::agent::unread()
    }
    #[cfg(not(feature = "wifi"))]
    {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Intro,
    Home,
    CheckIn,
    Breathe,
    Adventure,
    Week,
}

/// What the last frame showed.
type DrawKey = (Screen, usize, u8, Look, i32, u8, u8, u32, u8);

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
    /// Intro step: 0 egg, 1 and 2 cracks, 3 to 5 the first words.
    intro_step: u8,
    /// When the breathing, the adventure or a crack began.
    started_ms: u32,
    /// The adventure ended in a new stage.
    grew: bool,
    returned: bool,
    say: Option<(StrBuf<26>, u32)>,
    say_seq: u8,
    blink_until_ms: u32,
    next_blink_ms: u32,
    glance: i8,
    glance_until_ms: u32,
    next_glance_ms: u32,
    flap_until_ms: u32,
    next_flap_ms: u32,
    hop_until_ms: u32,
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
            intro_step: 0,
            started_ms: 0,
            grew: false,
            returned: false,
            say: None,
            say_seq: 0,
            blink_until_ms: 0,
            next_blink_ms: 0,
            glance: 0,
            glance_until_ms: 0,
            next_glance_ms: 0,
            flap_until_ms: 0,
            next_flap_ms: 0,
            hop_until_ms: 0,
            drawn: None,
        }
    }

    fn night(now_ms: u32) -> bool {
        clock::now(now_ms).is_some_and(|c| c.hour >= NIGHT_FROM || c.hour < NIGHT_TO)
    }

    fn say(&mut self, text: &str, now: u32) {
        self.say = Some((format(format_args!("{text}")), now.wrapping_add(SAY_MS)));
        self.say_seq = self.say_seq.wrapping_add(1);
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.save_soon = true;
    }

    fn wake(&mut self, now: u32) {
        self.asleep = false;
        self.awake_until_ms = now.wrapping_add(AWAKE_MS);
    }

    fn hop(&mut self, now: u32) {
        self.hop_until_ms = now.wrapping_add(HOP_MS);
    }

    /// Runs every frame, on screen or not.
    fn simulate(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        if !self.loaded {
            self.pet = ctx.store.config().pet;
            self.loaded = true;
            self.last_save_ms = now;
            self.next_blink_ms = now.wrapping_add(3_000);
            self.next_glance_ms = now.wrapping_add(4_000);
            self.next_flap_ms = now.wrapping_add(9_000);
            info!(
                "hoot: {} adventures, energy {}, {} goals ever, intro {}",
                self.pet.adventures,
                self.pet.energy,
                self.pet.goals_total,
                self.pet.intro_done()
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

        if FOCUS.swap(false, Ordering::Relaxed) && self.pet.complete(GOAL_FOCUS) {
            info!("hoot: focus session counted");
            self.say("Focus done. +20", now);
            audio::play(Sound::Coin);
            self.hop(now);
            self.changed();
        }
        if HUG.swap(false, Ordering::Relaxed) {
            info!("hoot: a hug from home");
            self.pet.hug();
            self.wake(now);
            self.say("A hug from home <3", now);
            audio::play(Sound::Done);
            self.hop(now);
            self.changed();
        }

        STAGE.store(STAGES.iter().position(|&s| s == self.pet.stage()).unwrap_or(0) as u8, Ordering::Relaxed);
        ENERGY.store(self.pet.energy, Ordering::Relaxed);
        GOALS_TODAY.store(self.pet.goals_today() as u8, Ordering::Relaxed);
        CHECKIN_DUE.store(
            self.pet.intro_done() && self.pet.day != 0 && self.pet.checkin == 0 && !self.asleep,
            Ordering::Relaxed,
        );
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

    fn intro(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        let elapsed = now.wrapping_sub(self.started_ms);
        let pressed = ctx.input.just_pressed(Button::L);
        match self.intro_step {
            0 if pressed => {
                self.intro_step = 1;
                self.started_ms = now;
                audio::play(Sound::Tick);
            }
            1 if elapsed >= 450 => {
                self.intro_step = 2;
                audio::play(Sound::Tick);
            }
            2 if elapsed >= 900 => {
                self.intro_step = 3;
                audio::play(Sound::Hoot);
                self.hop(now);
            }
            3 | 4 if pressed => self.intro_step += 1,
            5 if pressed => {
                self.pet.finish_intro();
                self.changed();
                self.screen = Screen::Home;
                self.say("Tick a goal to start.", now);
            }
            _ => {}
        }
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
                        self.hop(now);
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
        if input.just_pressed(Button::I) {
            self.screen = Screen::Week;
        }
        if input.just_pressed(Button::D) {
            let before = self.pet.outfit();
            let outfit = self.pet.next_outfit();
            if outfit == before {
                match self.pet.next_locked() {
                    Some(locked) => {
                        let text: StrBuf<26> =
                            format(format_args!("{} at {} trips", locked.label(), locked.unlock_at()));
                        self.say(text.as_str(), now);
                    }
                    None => self.say("Hoot has it all.", now),
                }
            } else {
                let text: StrBuf<26> = format(format_args!("Wearing {}.", outfit.label()));
                self.say(text.as_str(), now);
                audio::play(Sound::Tick);
                self.hop(now);
                self.changed();
            }
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
            self.hop(now);
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
        let level = (breath_radius(elapsed) - 16) * 255 / 24;
        ctx.hw.led_left.set(level as u8);
        ctx.hw.led_right.set(level as u8);
    }

    fn adventure(&mut self, ctx: &mut Ctx) {
        let now = ctx.now_ms;
        let elapsed = now.wrapping_sub(self.started_ms);
        if elapsed >= FLY_OUT_MS + AWAY_MS && !self.returned {
            self.returned = true;
            audio::play(Sound::Hoot);
        }
        let shown = elapsed >= FLY_OUT_MS + AWAY_MS + FLY_IN_MS;
        let done = elapsed >= FLY_OUT_MS + AWAY_MS + FLY_IN_MS + SHOW_MS;
        if done || (shown && ctx.input.just_pressed_mask() != 0) {
            self.screen = Screen::Home;
            if self.grew {
                let text: StrBuf<26> = format(format_args!("Hoot is {} now!", self.pet.stage().label()));
                self.say(text.as_str(), now);
            } else if let Some(o) = Outfit::ALL.iter().find(|o| o.unlock_at() == self.pet.adventures) {
                let text: StrBuf<26> = format(format_args!("New: {}! (D)", o.label()));
                self.say(text.as_str(), now);
            } else {
                self.say("Back home, happy.", now);
            }
        }
    }

    /// Idle life for this frame: blinks, glances, flutters, hops.
    fn look(&mut self, now: u32) -> (Look, i32) {
        if now.wrapping_sub(self.next_blink_ms) < 1 << 31 {
            self.blink_until_ms = now.wrapping_add(150);
            self.next_blink_ms = now.wrapping_add(2_500 + now % 3_000);
        }
        if now.wrapping_sub(self.next_glance_ms) < 1 << 31 {
            self.glance = [-1i8, 1, 0, 1, -1][(now / 7 % 5) as usize];
            self.glance_until_ms = now.wrapping_add(GLANCE_MS);
            self.next_glance_ms = now.wrapping_add(2_000 + now % 4_000);
        }
        if now.wrapping_sub(self.next_flap_ms) < 1 << 31 {
            self.flap_until_ms = now.wrapping_add(FLAP_MS);
            self.next_flap_ms = now.wrapping_add(7_000 + now % 9_000);
        }
        let blinking = now.wrapping_sub(self.blink_until_ms) >= 1 << 31;
        let glancing = now.wrapping_sub(self.glance_until_ms) >= 1 << 31;
        let flapping = now.wrapping_sub(self.flap_until_ms) >= 1 << 31 && (now / 100).is_multiple_of(2);
        let hopping = now.wrapping_sub(self.hop_until_ms) >= 1 << 31;
        let look = Look {
            eyes_shut: self.asleep || blinking,
            glasses: self.pet.stage() == Stage::Wise,
            glance: if glancing && !self.asleep { self.glance } else { 0 },
            flap: flapping && !self.asleep,
            outfit: self.pet.outfit(),
        };
        (look, if hopping { -3 } else { 0 })
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

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.simulate(ctx);
        self.drawn = None;
        if self.pet.intro_done() {
            self.screen = Screen::Home;
            if !self.asleep {
                audio::play(Sound::Hoot);
            }
        } else {
            self.screen = Screen::Intro;
            self.intro_step = 0;
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.simulate(ctx);
        let now = ctx.now_ms;
        if ctx.input.just_pressed_mask() != 0 {
            self.wake(now);
        }
        let result = match self.screen {
            Screen::Home => self.home(ctx),
            Screen::Intro => {
                self.intro(ctx);
                Transition::Stay
            }
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
            Screen::Week => {
                if ctx.input.just_pressed(Button::J) || ctx.input.just_pressed(Button::I) {
                    self.screen = Screen::Home;
                }
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
            self.say_seq = self.say_seq.wrapping_add(1);
        }
        let (look, hop) = self.look(now);
        let phase = match self.screen {
            Screen::Home => now / 600 % 3,
            Screen::Breathe | Screen::Adventure | Screen::Intro => now.wrapping_sub(self.started_ms) / 50,
            Screen::CheckIn | Screen::Week => 0,
        };
        let key = (self.screen, self.sel, self.face.max(self.intro_step), look, hop, self.pet.energy, self.pet.done_today, phase, self.say_seq);
        if self.drawn != Some(key) {
            self.drawn = Some(key);
            let secs = clock::now_secs(now);
            let say = self.say.as_ref().map(|s| &s.0);
            match self.screen {
                Screen::Intro => draw_intro(ctx.fb, self.intro_step, look, hop),
                Screen::Home => draw_home(ctx.fb, &self.pet, self.sel, look, hop, self.asleep, say, secs, phase, now),
                Screen::CheckIn => draw_check_in(ctx.fb, &self.pet, self.face, look),
                Screen::Breathe => draw_breathe(ctx.fb, &self.pet, now.wrapping_sub(self.started_ms), look),
                Screen::Adventure => draw_adventure(ctx.fb, &self.pet, now.wrapping_sub(self.started_ms), self.grew, look),
                Screen::Week => draw_week(ctx.fb, &self.pet, clock::now(now).map(|c| c.weekday)),
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

/// The owl in its usual spot, top-left of the content area.
fn owl_left(fb: &mut Framebuffer, look: Look, hop: i32) {
    owl::draw(fb, 8, 26 + hop, 2, look);
}

fn draw_intro(fb: &mut Framebuffer, step: u8, look: Look, hop: i32) {
    theme::screen(fb, "Hoot", "");
    let (x, y) = ((WIDTH - 32) / 2, 28);
    match step {
        0..=2 => {
            owl::draw_egg(fb, x, y, 2, step);
            fb.draw_text_centered(72, "Something is stirring...", theme::TEXT, None, 1);
            if step == 0 {
                fb.draw_text_centered(88, "L to help it along", theme::MUTED, None, 1);
            }
        }
        _ => {
            owl::draw(fb, x, y + hop, 2, Look { outfit: Outfit::None, ..look });
            let (a, b): (&str, &str) = match step {
                3 => ("Hi! I'm Hoot.", "Pleased to meet you."),
                4 => ("I grow when you look", "after yourself."),
                _ => ("Tick a goal. When I'm", "full, I go exploring."),
            };
            fb.draw_text_centered(72, a, theme::TEXT, None, 1);
            fb.draw_text_centered(84, b, theme::TEXT, None, 1);
            theme::footer(fb, "L next");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_home(
    fb: &mut Framebuffer,
    pet: &Pet,
    sel: usize,
    look: Look,
    hop: i32,
    asleep: bool,
    say: Option<&StrBuf<26>>,
    secs: Option<u32>,
    phase: u32,
    now: u32,
) {
    theme::screen(fb, "Hoot", stage_line(pet, secs).as_str());
    owl_left(fb, look, hop);
    theme::meter(fb, 6, 64, 36, 6, (pet.energy as u32 * 255 / 100) as u8, theme::ACCENT);
    let energy: StrBuf<8> = format(format_args!("{}%", pet.energy));
    fb.draw_text_centered_in(6, 42, 73, energy.as_str(), theme::MUTED);

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

    let unread = unread_notes();
    let hour = clock::now(now).map(|c| c.hour);
    let buf: StrBuf<26>;
    let line: &str = if let Some(s) = say {
        s.as_str()
    } else if asleep {
        ["z", "z z", "z z z"][phase as usize % 3]
    } else if pet.ready() {
        "Energy full! K to fly"
    } else if unread > 0 {
        "A note from home waits."
    } else if pet.checkin == 0 && pet.day != 0 {
        buf = format(format_args!("{} How are you?", greeting(hour.unwrap_or(12))));
        buf.as_str()
    } else if pet.goals_today() >= 3 {
        "Proud of you today."
    } else {
        line_of_day(pet.day)
    };
    let color = if say.is_some() || unread > 0 { theme::ACCENT } else { theme::MUTED };
    fb.draw_text(4, 96, line, color, None);
    theme::footer(fb, if pet.ready() { "K fly   L done   I week" } else { "L done   I week   D outfit" });
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
            r[4] |= 0b0001_1100;
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

fn draw_check_in(fb: &mut Framebuffer, _pet: &Pet, face: u8, look: Look) {
    theme::screen(fb, "Hoot", "check in");
    owl_left(fb, look, 0);
    fb.draw_text(46, 26, "How are you today?", theme::TEXT, None);
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
    fb.draw_text_centered(92, "Hoot won't tell a soul.", theme::MUTED, None, 1);
    theme::footer(fb, "A/D pick   L me   J skip");
}

fn draw_breathe(fb: &mut Framebuffer, _pet: &Pet, elapsed: u32, look: Look) {
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
    owl::draw(fb, cx - owl::SIZE, cy - owl::SIZE, 2, Look { glance: 0, flap: false, ..look });
    let text = match (elapsed / BREATH_PHASE_MS) % 4 {
        0 => "breathe in",
        1 => "hold",
        2 => "breathe out",
        _ => "hold",
    };
    fb.draw_text_centered(104, text, theme::TEXT, None, 1);
    theme::footer(fb, "J stop");
}

fn sparkle(fb: &mut Framebuffer, x: i32, y: i32, phase: u32) {
    for (i, &(dx, dy)) in [(-6, -4), (36, -2), (-4, 30), (38, 28)].iter().enumerate() {
        if (phase / 3 + i as u32).is_multiple_of(2) {
            fb.hline(x + dx - 1, y + dy, 3, theme::ACCENT);
            fb.vline(x + dx, y + dy - 1, 3, theme::ACCENT);
        }
    }
}

fn draw_adventure(fb: &mut Framebuffer, pet: &Pet, elapsed: u32, grew: bool, look: Look) {
    fb.clear(theme::BG);
    for &(x, y) in &[(14, 10), (38, 22), (129, 30), (147, 12), (22, 58), (140, 66), (70, 14), (100, 40)] {
        fb.set(x, y, theme::MUTED);
    }
    splash::moon(fb, 132, 16, 8);
    let flying = Look { eyes_shut: false, glance: 0, flap: (elapsed / 120).is_multiple_of(2), ..look };
    let bob = if (elapsed / 200).is_multiple_of(2) { 0 } else { -3 };
    if elapsed < FLY_OUT_MS {
        let x = 8 + (elapsed as i32 * (WIDTH + 8 - 8)) / FLY_OUT_MS as i32;
        owl::draw(fb, x, 40 + bob, 2, flying);
    } else if elapsed < FLY_OUT_MS + AWAY_MS {
        fb.draw_text_centered(60, "Hoot is out exploring...", theme::MUTED, None, 1);
    } else if elapsed < FLY_OUT_MS + AWAY_MS + FLY_IN_MS {
        let t = (elapsed - FLY_OUT_MS - AWAY_MS) as i32;
        let x = WIDTH - (t * (WIDTH - 64)) / FLY_IN_MS as i32;
        owl::draw(fb, x, 40 + bob, 2, flying);
    } else {
        owl::draw(fb, 64, 28, 2, Look { glance: 0, flap: false, ..look });
        sparkle(fb, 64, 28, elapsed / 50);
        fb.draw_text_centered(70, "Hoot found", theme::MUTED, None, 1);
        fb.draw_text_centered(82, pet.discovery(), theme::ACCENT, None, 1);
        if grew {
            let line: StrBuf<26> = format(format_args!("and grew into {}!", pet.stage().label()));
            fb.draw_text_centered(96, line.as_str(), theme::WARN, None, 1);
        }
        theme::footer(fb, "any key");
    }
}

/// The week: six days back and today, check-in faces and goal bars.
fn draw_week(fb: &mut Framebuffer, pet: &Pet, today_weekday: Option<u8>) {
    let total: u32 = pet.goal_hist[1..].iter().map(|&g| g as u32).sum::<u32>() + pet.goals_today();
    let right: StrBuf<14> = format(format_args!("{} goals", total));
    theme::screen(fb, "This week", right.as_str());
    for i in 0..7usize {
        let x = 8 + i as i32 * 21;
        let today = i == 6;
        let (checkin, goals) = if today { (pet.checkin, pet.goals_today() as u8) } else { (pet.history[i + 1], pet.goal_hist[i + 1]) };
        let letter = match today_weekday {
            Some(w) => &WEEKDAYS[((w as usize + 7 + i).saturating_sub(6)) % 7][..1],
            None => "-",
        };
        fb.draw_text(x + 4, 22, letter, if today { theme::ACCENT } else { theme::MUTED }, None);
        if checkin != 0 {
            fb.draw_bitmap_scaled(x + 2, 34, 8, 8, &face_rows(checkin), theme::TEXT, 1);
        } else {
            fb.hline(x + 4, 38, 4, theme::BAR);
        }
        let h = goals as i32 * 6;
        if h > 0 {
            fb.fill_rect(x + 1, 98 - h, 12, h, if today { theme::ACCENT } else { theme::ACCENT_DARK });
        }
        let n: StrBuf<4> = format(format_args!("{goals}"));
        fb.draw_text(x + 4, 101, n.as_str(), theme::MUTED, None);
    }
    fb.hline(4, 98, WIDTH - 8, theme::MUTED);
    theme::footer(fb, "J back");
}

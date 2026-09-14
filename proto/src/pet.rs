//! Hoot the owl: the care model. Plain numbers, no hardware, so it runs
//! on the device and in host tests.
//!
//! Three needs from 0 to 100: hunger (0 is full), happiness and energy.
//! They drift with time in ten-minute ticks; feeding, playing and a
//! stroke push them back. Hoot sleeps at night or when worn out, and
//! never dies: at worst it sulks until someone comes by.

pub const MAX: u8 = 100;
/// One tick of the model, in minutes.
pub const TICK_MIN: u32 = 10;
const DAY_SECS: u32 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pet {
    /// 0 = full, 100 = starving.
    pub hunger: u8,
    /// 0 = miserable, 100 = delighted.
    pub happy: u8,
    /// 0 = worn out, 100 = rested.
    pub energy: u8,
    /// Local seconds since 1970 when Hoot hatched. 0 until a clock is known.
    pub born: u32,
    /// Local seconds at the last tick that was saved. 0 = never.
    pub seen: u32,
    pub fed: u16,
    pub played: u16,
}

impl Default for Pet {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mood {
    Sleeping = 0,
    Hungry = 1,
    Tired = 2,
    Sad = 3,
    Content = 4,
    Happy = 5,
}

impl Mood {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Mood::Sleeping,
            1 => Mood::Hungry,
            2 => Mood::Tired,
            3 => Mood::Sad,
            5 => Mood::Happy,
            _ => Mood::Content,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Mood::Sleeping => "asleep",
            Mood::Hungry => "hungry",
            Mood::Tired => "tired",
            Mood::Sad => "sad",
            Mood::Content => "content",
            Mood::Happy => "happy",
        }
    }

    /// True when Hoot wants something from you.
    pub const fn needs_care(self) -> bool {
        matches!(self, Mood::Hungry | Mood::Tired | Mood::Sad)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Owlet,
    Owl,
    Wise,
}

impl Stage {
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Owlet => "owlet",
            Stage::Owl => "owl",
            Stage::Wise => "wise owl",
        }
    }
}

fn add(v: u8, delta: i32) -> u8 {
    (v as i32 + delta).clamp(0, MAX as i32) as u8
}

impl Pet {
    pub const fn new() -> Self {
        Self { hunger: 20, happy: 70, energy: 80, born: 0, seen: 0, fed: 0, played: 0 }
    }

    /// Ten minutes pass.
    pub fn tick(&mut self, asleep: bool) {
        if asleep {
            self.hunger = add(self.hunger, 1);
            self.energy = add(self.energy, 5);
        } else {
            self.hunger = add(self.hunger, 2);
            self.energy = add(self.energy, -1);
            let starving = if self.hunger >= 90 { -3 } else { 0 };
            self.happy = add(self.happy, -1 + starving);
        }
    }

    /// `minutes` pass, for catching up after time off. Capped at a day so a
    /// long power cut does not leave Hoot in a state.
    pub fn advance(&mut self, minutes: u32, asleep: bool) {
        for _ in 0..(minutes.min(24 * 60) / TICK_MIN) {
            self.tick(asleep);
        }
    }

    /// A meal. Refused when not hungry at all.
    pub fn feed(&mut self) -> bool {
        if self.hunger < 10 {
            return false;
        }
        self.hunger = add(self.hunger, -40);
        self.happy = add(self.happy, 5);
        self.fed = self.fed.saturating_add(1);
        true
    }

    /// A game. Refused when too tired.
    pub fn play(&mut self) -> bool {
        if self.energy < 15 {
            return false;
        }
        self.happy = add(self.happy, 25);
        self.energy = add(self.energy, -10);
        self.hunger = add(self.hunger, 5);
        self.played = self.played.saturating_add(1);
        true
    }

    /// A stroke on the head.
    pub fn stroke(&mut self) {
        self.happy = add(self.happy, 3);
    }

    /// Whether Hoot is asleep this tick. Night sends it to sleep, so does
    /// exhaustion; it wakes once rested and it is day.
    pub fn sleep_state(&self, asleep: bool, night: bool) -> bool {
        if asleep { night || self.energy < 95 } else { night || self.energy < 15 }
    }

    pub fn mood(&self, asleep: bool) -> Mood {
        if asleep {
            Mood::Sleeping
        } else if self.hunger >= 70 {
            Mood::Hungry
        } else if self.energy < 25 {
            Mood::Tired
        } else if self.happy < 35 {
            Mood::Sad
        } else if self.happy >= 75 {
            Mood::Happy
        } else {
            Mood::Content
        }
    }

    /// Whole days since hatching, if both times are known.
    pub fn age_days(&self, now_secs: Option<u32>) -> Option<u32> {
        let now = now_secs?;
        (self.born != 0 && now >= self.born).then(|| (now - self.born) / DAY_SECS)
    }

    pub fn stage(&self, now_secs: Option<u32>) -> Stage {
        match self.age_days(now_secs) {
            Some(d) if d >= 14 => Stage::Wise,
            Some(d) if d >= 3 => Stage::Owl,
            _ => Stage::Owlet,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_hoot_is_content() {
        let p = Pet::new();
        assert_eq!(p.mood(false), Mood::Content);
        assert_eq!(p.stage(None), Stage::Owlet);
    }

    #[test]
    fn hunger_grows_and_a_meal_helps() {
        let mut p = Pet::new();
        p.advance(5 * 60, false); // five hours awake
        assert_eq!(p.hunger, 20 + 2 * 30);
        assert_eq!(p.mood(false), Mood::Hungry);
        assert!(p.feed());
        assert_eq!(p.hunger, 40);
        assert_eq!(p.fed, 1);
    }

    #[test]
    fn a_full_owl_refuses_food() {
        let mut p = Pet { hunger: 5, ..Pet::new() };
        assert!(!p.feed());
        assert_eq!(p.fed, 0);
    }

    #[test]
    fn sleep_restores_energy_and_a_day_is_the_cap() {
        let mut p = Pet { energy: 10, ..Pet::new() };
        assert!(p.sleep_state(false, false), "worn out: falls asleep");
        p.advance(2 * 60, true);
        assert_eq!(p.energy, 10 + 5 * 12);
        assert!(p.sleep_state(true, false), "still resting");
        p.advance(2 * 60, true);
        assert_eq!(p.energy, MAX);
        assert!(!p.sleep_state(true, false), "rested and daytime: wakes");
        assert!(p.sleep_state(true, true), "rested but night: sleeps on");
        let mut long = Pet::new();
        long.advance(10 * 24 * 60, false);
        let mut day = Pet::new();
        day.advance(24 * 60, false);
        assert_eq!(long, day);
    }

    #[test]
    fn play_needs_energy_and_lifts_the_mood() {
        let mut p = Pet { energy: 10, ..Pet::new() };
        assert!(!p.play());
        let mut p = Pet::new();
        assert!(p.play());
        assert_eq!(p.mood(false), Mood::Happy);
        assert_eq!((p.energy, p.hunger, p.played), (70, 25, 1));
    }

    #[test]
    fn starving_makes_it_sad_fast() {
        let mut p = Pet { hunger: 90, ..Pet::new() };
        p.advance(60, false);
        assert_eq!(p.happy, 70 - 4 * 6);
    }

    #[test]
    fn stages_by_age() {
        let p = Pet { born: 1_000_000, ..Pet::new() };
        assert_eq!(p.stage(Some(1_000_000 + 2 * DAY_SECS)), Stage::Owlet);
        assert_eq!(p.stage(Some(1_000_000 + 3 * DAY_SECS)), Stage::Owl);
        assert_eq!(p.stage(Some(1_000_000 + 20 * DAY_SECS)), Stage::Wise);
        assert_eq!(p.age_days(Some(1_000_000 + 20 * DAY_SECS)), Some(20));
        assert_eq!(Pet::new().age_days(Some(5)), None);
    }

    #[test]
    fn mood_round_trips_as_u8() {
        for m in [Mood::Sleeping, Mood::Hungry, Mood::Tired, Mood::Sad, Mood::Content, Mood::Happy] {
            assert_eq!(Mood::from_u8(m as u8), m);
        }
        assert!(Mood::Hungry.needs_care() && !Mood::Happy.needs_care());
    }
}

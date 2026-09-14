//! Hoot, the way Finch does it. Hoot is not a mouth to feed: it grows
//! when you look after yourself. A daily check-in and small self-care
//! goals give it energy; a full bar sends it on an adventure, and
//! adventures make it grow. Nothing is lost for skipping a day. Plain
//! numbers, no hardware, so it runs on the device and in host tests.

/// The goals, in the order the screen lists them. Bit `i` of
/// `Pet::done_today` is goal `i`.
pub const GOALS: [&str; 7] = [
    "Check in",
    "Drink water",
    "Move a little",
    "See daylight",
    "Focus session",
    "Breathe with Hoot",
    "Wind down",
];
pub const GOAL_CHECKIN: usize = 0;
pub const GOAL_FOCUS: usize = 4;
pub const GOAL_BREATHE: usize = 5;

pub const ENERGY_PER_GOAL: u8 = 20;
pub const ENERGY_PER_HUG: u8 = 10;
pub const ENERGY_FULL: u8 = 100;

/// What Hoot brings back, by adventure number.
pub const DISCOVERIES: [&str; 12] = [
    "a shiny pebble",
    "a fox in the hedge",
    "the last train home",
    "a cloud like a whale",
    "a lost glove, returned",
    "the smell of rain",
    "a cat on a warm car",
    "an owl who said hello",
    "the moon in a puddle",
    "a bakery at dawn",
    "a very good stick",
    "a quiet, starry field",
];

/// Things Hoot can wear. Each unlocks after so many adventures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Outfit {
    #[default]
    None = 0,
    Scarf = 1,
    Bow = 2,
    PartyHat = 3,
    Headphones = 4,
    Crown = 5,
}

impl Outfit {
    pub const ALL: [Outfit; 6] =
        [Outfit::None, Outfit::Scarf, Outfit::Bow, Outfit::PartyHat, Outfit::Headphones, Outfit::Crown];

    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Outfit::Scarf,
            2 => Outfit::Bow,
            3 => Outfit::PartyHat,
            4 => Outfit::Headphones,
            5 => Outfit::Crown,
            _ => Outfit::None,
        }
    }

    /// Adventures needed.
    pub const fn unlock_at(self) -> u16 {
        match self {
            Outfit::None => 0,
            Outfit::Scarf => 1,
            Outfit::Bow => 3,
            Outfit::PartyHat => 5,
            Outfit::Headphones => 10,
            Outfit::Crown => 15,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Outfit::None => "nothing on",
            Outfit::Scarf => "a scarf",
            Outfit::Bow => "a bow",
            Outfit::PartyHat => "a party hat",
            Outfit::Headphones => "headphones",
            Outfit::Crown => "a crown",
        }
    }
}

/// `Pet::flags` bits.
pub const FLAG_INTRO_DONE: u8 = 1;

/// One gentle line a day, by day number. All fit a 26-character line.
pub const LINES: [&str; 21] = [
    "You did enough today.",
    "Small steps still count.",
    "Drink some water, friend.",
    "Rest is not a reward.",
    "One thing at a time.",
    "It's okay to go slow.",
    "Proud of you for trying.",
    "Stretch. I'll wait.",
    "You are not behind.",
    "Breathe. Then decide.",
    "Look out of the window.",
    "Be kind to you today.",
    "A short walk helps.",
    "Call someone you miss.",
    "Good enough is good.",
    "Today can be gentle.",
    "You're doing fine.",
    "Sit up. Sip. Continue.",
    "The moon's out. Wind down.",
    "Hoot believes in you.",
    "Tomorrow is a new page.",
];

pub fn line_of_day(day: u16) -> &'static str {
    LINES[day as usize % LINES.len()]
}

/// A greeting for the hour, short enough to add a question after.
pub const fn greeting(hour: u8) -> &'static str {
    match hour {
        5..=11 => "Good morning!",
        12..=17 => "Afternoon!",
        _ => "Evening!",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pet {
    /// Towards the next adventure, 0 to 100.
    pub energy: u8,
    /// Adventures so far. Growth comes from these.
    pub adventures: u16,
    /// Goals completed, all time.
    pub goals_total: u16,
    /// Local day number (days since 1970) that `done_today` and `checkin`
    /// belong to. 0 = no clock yet.
    pub day: u16,
    /// Bit `i` set = goal `i` done today.
    pub done_today: u8,
    /// Today's check-in: 0 = not yet, 1 rough, 2 low, 3 okay, 4 good, 5 great.
    pub checkin: u8,
    /// The previous seven days' check-ins, newest last. 0 = none that day.
    pub history: [u8; 7],
    /// Goals done on each of the previous seven days, newest last.
    pub goal_hist: [u8; 7],
    /// `FLAG_*` bits.
    pub flags: u8,
    /// What Hoot wears, as `Outfit as u8`.
    pub outfit: u8,
    /// Local seconds since 1970 when Hoot hatched. 0 until a clock is known.
    pub born: u32,
    /// Local seconds at the last save. 0 = never.
    pub seen: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Hatchling,
    Owlet,
    Fledgling,
    Owl,
    Wise,
}

impl Stage {
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Hatchling => "hatchling",
            Stage::Owlet => "owlet",
            Stage::Fledgling => "fledgling",
            Stage::Owl => "owl",
            Stage::Wise => "wise owl",
        }
    }
}

/// The word for a check-in value.
pub const fn mood_word(checkin: u8) -> &'static str {
    match checkin {
        1 => "rough",
        2 => "low",
        3 => "okay",
        4 => "good",
        5 => "great",
        _ => "",
    }
}

/// What Hoot says back after a check-in.
pub const fn reply(checkin: u8) -> &'static str {
    match checkin {
        1 => "I'm right here with you.",
        2 => "Gentle day, then. One thing at a time.",
        3 => "Okay is okay. Glad you're here.",
        4 => "Good! Let's keep it going.",
        5 => "Wonderful! I'm chuffed for you.",
        _ => "",
    }
}

impl Pet {
    pub const fn new() -> Self {
        Self {
            energy: 0,
            adventures: 0,
            goals_total: 0,
            day: 0,
            done_today: 0,
            checkin: 0,
            history: [0; 7],
            goal_hist: [0; 7],
            flags: 0,
            outfit: 0,
            born: 0,
            seen: 0,
        }
    }

    pub fn intro_done(&self) -> bool {
        self.flags & FLAG_INTRO_DONE != 0
    }

    pub fn finish_intro(&mut self) {
        self.flags |= FLAG_INTRO_DONE;
    }

    pub fn outfit(&self) -> Outfit {
        Outfit::from_u8(self.outfit)
    }

    pub fn unlocked(&self, outfit: Outfit) -> bool {
        self.adventures >= outfit.unlock_at()
    }

    /// The next outfit Hoot has earned, round and round. Returns it.
    pub fn next_outfit(&mut self) -> Outfit {
        let start = self.outfit as usize;
        for step in 1..=Outfit::ALL.len() {
            let candidate = Outfit::ALL[(start + step) % Outfit::ALL.len()];
            if self.unlocked(candidate) {
                self.outfit = candidate as u8;
                return candidate;
            }
        }
        Outfit::None
    }

    /// The first outfit still locked, if any, and what it needs.
    pub fn next_locked(&self) -> Option<Outfit> {
        Outfit::ALL.iter().copied().find(|o| !self.unlocked(*o))
    }

    /// The clock says it is `day`. On a new day, yesterday's check-in
    /// joins the history and today starts clean.
    pub fn new_day(&mut self, day: u16) -> bool {
        if self.day == day {
            return false;
        }
        if self.day != 0 {
            self.history.copy_within(1.., 0);
            self.history[6] = self.checkin;
            self.goal_hist.copy_within(1.., 0);
            self.goal_hist[6] = self.goals_today() as u8;
        }
        self.day = day;
        self.done_today = 0;
        self.checkin = 0;
        true
    }

    pub fn done(&self, goal: usize) -> bool {
        self.done_today & (1 << goal) != 0
    }

    pub fn goals_today(&self) -> u32 {
        self.done_today.count_ones()
    }

    /// Mark a goal done. False if it already was.
    pub fn complete(&mut self, goal: usize) -> bool {
        if self.done(goal) {
            return false;
        }
        self.done_today |= 1 << goal;
        self.goals_total = self.goals_total.saturating_add(1);
        self.energy = self.energy.saturating_add(ENERGY_PER_GOAL).min(ENERGY_FULL);
        true
    }

    /// Take a goal back. False if it was not done.
    pub fn uncomplete(&mut self, goal: usize) -> bool {
        if !self.done(goal) {
            return false;
        }
        self.done_today &= !(1 << goal);
        self.goals_total = self.goals_total.saturating_sub(1);
        self.energy = self.energy.saturating_sub(ENERGY_PER_GOAL);
        true
    }

    /// Today's check-in, 1 to 5. Once a day; counts as the first goal.
    pub fn check_in(&mut self, value: u8) -> bool {
        if self.checkin != 0 || !(1..=5).contains(&value) {
            return false;
        }
        self.checkin = value;
        self.complete(GOAL_CHECKIN);
        true
    }

    pub fn ready(&self) -> bool {
        self.energy >= ENERGY_FULL
    }

    /// Off it goes. False unless the bar is full.
    pub fn adventure(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        self.energy = 0;
        self.adventures = self.adventures.saturating_add(1);
        true
    }

    pub fn discovery(&self) -> &'static str {
        DISCOVERIES[(self.adventures as usize).wrapping_sub(1) % DISCOVERIES.len()]
    }

    /// Someone at home sent good wishes.
    pub fn hug(&mut self) {
        self.energy = self.energy.saturating_add(ENERGY_PER_HUG).min(ENERGY_FULL);
    }

    pub fn stage(&self) -> Stage {
        match self.adventures {
            0..=2 => Stage::Hatchling,
            3..=9 => Stage::Owlet,
            10..=24 => Stage::Fledgling,
            25..=59 => Stage::Owl,
            _ => Stage::Wise,
        }
    }

    /// Whole days since hatching, if both times are known.
    pub fn age_days(&self, now_secs: Option<u32>) -> Option<u32> {
        let now = now_secs?;
        (self.born != 0 && now >= self.born).then(|| (now - self.born) / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_fill_the_bar_and_an_adventure_empties_it() {
        let mut p = Pet::new();
        for g in 1..=5 {
            assert!(p.complete(g));
        }
        assert!(!p.complete(1), "no double credit");
        assert_eq!((p.energy, p.goals_today(), p.goals_total), (100, 5, 5));
        assert!(p.ready());
        assert!(p.adventure());
        assert_eq!((p.energy, p.adventures), (0, 1));
        assert!(!p.adventure());
        assert_eq!(p.discovery(), DISCOVERIES[0]);
    }

    #[test]
    fn taking_a_goal_back() {
        let mut p = Pet::new();
        p.complete(2);
        assert!(p.uncomplete(2));
        assert!(!p.uncomplete(2));
        assert_eq!((p.energy, p.goals_total, p.done_today), (0, 0, 0));
    }

    #[test]
    fn check_in_once_a_day_and_history_rolls() {
        let mut p = Pet::new();
        assert!(p.new_day(100));
        assert!(p.check_in(4));
        assert!(!p.check_in(2), "once a day");
        assert!(p.done(GOAL_CHECKIN));
        assert_eq!(p.energy, ENERGY_PER_GOAL);
        assert!(!p.new_day(100), "same day: nothing happens");
        p.complete(2);
        assert!(p.new_day(101));
        assert_eq!(p.history[6], 4);
        assert_eq!(p.goal_hist[6], 2, "check-in and one goal");
        assert_eq!((p.checkin, p.done_today), (0, 0));
        assert_eq!(p.energy, 2 * ENERGY_PER_GOAL, "energy carries over");
        assert!(!p.check_in(9));
    }

    #[test]
    fn first_day_has_no_yesterday() {
        let mut p = Pet::new();
        p.checkin = 5; // garbage from before any clock
        p.new_day(7);
        assert_eq!(p.history, [0; 7]);
    }

    #[test]
    fn stages_come_from_adventures() {
        let mut p = Pet::new();
        assert_eq!(p.stage(), Stage::Hatchling);
        p.adventures = 3;
        assert_eq!(p.stage(), Stage::Owlet);
        p.adventures = 10;
        assert_eq!(p.stage(), Stage::Fledgling);
        p.adventures = 25;
        assert_eq!(p.stage(), Stage::Owl);
        p.adventures = 60;
        assert_eq!(p.stage(), Stage::Wise);
        assert!(Stage::Wise > Stage::Owl);
    }

    #[test]
    fn outfits_unlock_with_adventures() {
        let mut p = Pet::new();
        assert_eq!(p.next_outfit(), Outfit::None, "nothing earned yet");
        assert_eq!(p.next_locked(), Some(Outfit::Scarf));
        p.adventures = 3;
        assert_eq!(p.next_outfit(), Outfit::Scarf);
        assert_eq!(p.next_outfit(), Outfit::Bow);
        assert_eq!(p.next_outfit(), Outfit::None, "round it goes");
        assert_eq!(p.next_locked(), Some(Outfit::PartyHat));
        p.adventures = 99;
        assert_eq!(p.next_locked(), None);
        assert_eq!(Outfit::from_u8(5), Outfit::Crown);
        assert_eq!(Outfit::from_u8(77), Outfit::None);
    }

    #[test]
    fn intro_flag_and_lines() {
        let mut p = Pet::new();
        assert!(!p.intro_done());
        p.finish_intro();
        assert!(p.intro_done());
        assert!(LINES.iter().all(|l| l.len() <= 26), "every line fits the screen");
        assert_eq!(line_of_day(0), line_of_day(21));
        assert_eq!(greeting(8), "Good morning!");
        assert_eq!(greeting(23), "Evening!");
    }

    #[test]
    fn hugs_help_a_little() {
        let mut p = Pet { energy: 95, ..Pet::new() };
        p.hug();
        assert_eq!(p.energy, 100);
        assert_eq!(mood_word(3), "okay");
        assert!(!reply(1).is_empty() && reply(0).is_empty());
    }

    #[test]
    fn age_needs_both_times() {
        let p = Pet { born: 1_000, ..Pet::new() };
        assert_eq!(p.age_days(Some(1_000 + 3 * 86_400)), Some(3));
        assert_eq!(Pet::new().age_days(Some(5)), None);
        assert_eq!(p.age_days(None), None);
    }
}

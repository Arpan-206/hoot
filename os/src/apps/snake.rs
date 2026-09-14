//! Snake on a 20 by 14 board of 8-pixel cells. W A S D steer, L starts
//! or restarts, K pauses. The best score is kept in the config record.

use hoot_gfx::{Framebuffer, WIDTH};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Snake", group: Group::Fun, needs_network: false };

const COLS: i32 = 20;
const ROWS: i32 = 14;
const CELL: i32 = 8;
/// Top of the board, under the title bar.
const TOP: i32 = 14;
const MAX: usize = (COLS * ROWS) as usize;
const START_STEP_MS: u32 = 160;
const FASTEST_STEP_MS: u32 = 70;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Running,
    Paused,
    Over,
}

pub struct Snake {
    /// Head first.
    body: [(u8, u8); MAX],
    len: usize,
    dir: (i8, i8),
    next_dir: (i8, i8),
    food: (u8, u8),
    state: State,
    score: u16,
    best: u16,
    step_ms: u32,
    next_step_ms: u32,
    rng: u32,
    dirty: bool,
}

impl Snake {
    pub const fn new() -> Self {
        Self {
            body: [(0, 0); MAX],
            len: 0,
            dir: (1, 0),
            next_dir: (1, 0),
            food: (0, 0),
            state: State::Ready,
            score: 0,
            best: 0,
            step_ms: START_STEP_MS,
            next_step_ms: 0,
            rng: 1,
            dirty: true,
        }
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn occupied(&self, cell: (u8, u8)) -> bool {
        self.body[..self.len].contains(&cell)
    }

    fn place_food(&mut self) {
        for _ in 0..64 {
            let r = self.random();
            let cell = ((r % COLS as u32) as u8, ((r >> 8) % ROWS as u32) as u8);
            if !self.occupied(cell) {
                self.food = cell;
                return;
            }
        }
        // A crowded board: take the first free cell.
        for y in 0..ROWS as u8 {
            for x in 0..COLS as u8 {
                if !self.occupied((x, y)) {
                    self.food = (x, y);
                    return;
                }
            }
        }
    }

    fn start(&mut self, now: u32) {
        self.rng = now | 1;
        self.len = 3;
        for (i, cell) in self.body[..3].iter_mut().enumerate() {
            *cell = ((COLS / 2 - i as i32) as u8, (ROWS / 2) as u8);
        }
        self.dir = (1, 0);
        self.next_dir = (1, 0);
        self.score = 0;
        self.step_ms = START_STEP_MS;
        self.next_step_ms = now.wrapping_add(self.step_ms);
        self.place_food();
        self.state = State::Running;
        self.dirty = true;
    }

    fn step(&mut self, ctx: &mut Ctx) {
        self.dir = self.next_dir;
        let head = self.body[0];
        let nx = head.0 as i32 + self.dir.0 as i32;
        let ny = head.1 as i32 + self.dir.1 as i32;
        let hit_wall = !(0..COLS).contains(&nx) || !(0..ROWS).contains(&ny);
        let new_head = (nx.max(0) as u8, ny.max(0) as u8);
        // The tail cell frees up unless the snake grows this step.
        let eats = !hit_wall && new_head == self.food;
        let hit_self = !hit_wall && self.body[..self.len - usize::from(!eats)].contains(&new_head);
        if hit_wall || hit_self {
            self.state = State::Over;
            audio::play(Sound::Rest);
            if self.score > self.best {
                self.best = self.score;
                let best = self.best;
                let _ = ctx.store.update_config(|c| c.snake_best = best);
            }
            self.dirty = true;
            return;
        }
        if eats && self.len < MAX {
            self.len += 1;
        }
        self.body.copy_within(0..self.len - 1, 1);
        self.body[0] = new_head;
        if eats {
            self.score += 1;
            self.step_ms = (START_STEP_MS - self.score as u32 * 4).max(FASTEST_STEP_MS);
            audio::play(Sound::Coin);
            self.place_food();
        }
        self.dirty = true;
    }
}

impl App for Snake {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.best = ctx.store.config().snake_best;
        if self.state == State::Running {
            self.state = State::Paused;
        }
        self.dirty = true;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let input = ctx.input;
        for (button, d) in [(Button::W, (0, -1)), (Button::S, (0, 1)), (Button::A, (-1, 0)), (Button::D, (1, 0))] {
            if input.just_pressed(button) && (d.0 != -self.dir.0 || d.1 != -self.dir.1) {
                self.next_dir = d;
            }
        }
        if input.just_pressed(Button::L) {
            match self.state {
                State::Ready | State::Over => self.start(now),
                State::Paused => {
                    self.state = State::Running;
                    self.next_step_ms = now.wrapping_add(self.step_ms);
                    self.dirty = true;
                }
                State::Running => {}
            }
        }
        if input.just_pressed(Button::K) && matches!(self.state, State::Running | State::Paused) {
            self.state = if self.state == State::Running { State::Paused } else { State::Running };
            self.next_step_ms = now.wrapping_add(self.step_ms);
            self.dirty = true;
        }
        if self.state == State::Running && now.wrapping_sub(self.next_step_ms) < 1 << 31 {
            self.next_step_ms = now.wrapping_add(self.step_ms);
            self.step(ctx);
        }
        if self.dirty {
            self.dirty = false;
            draw(ctx.fb, self);
        }
        Transition::Stay
    }
}

fn draw(fb: &mut Framebuffer, s: &Snake) {
    let right: StrBuf<20> = format(format_args!("{}  best {}", s.score, s.best));
    theme::screen(fb, "Snake", right.as_str());
    fb.fill_rect(0, TOP - 1, WIDTH, ROWS * CELL + 2, theme::BAR);
    fb.fill_rect(0, TOP, WIDTH, ROWS * CELL, theme::BG);
    if s.state != State::Ready {
        let (fx, fy) = (s.food.0 as i32 * CELL, TOP + s.food.1 as i32 * CELL);
        fb.fill_rect(fx + 2, fy + 2, CELL - 4, CELL - 4, theme::WARN);
        for (i, &(x, y)) in s.body[..s.len].iter().enumerate() {
            let color = if i == 0 { theme::ACCENT } else { theme::TEXT };
            fb.fill_rect(x as i32 * CELL + 1, TOP + y as i32 * CELL + 1, CELL - 2, CELL - 2, color);
        }
    }
    let (a, b): (&str, &str) = match s.state {
        State::Ready => ("W A S D to steer", "L start   J back"),
        State::Paused => ("paused", "L go on   J back"),
        State::Over => ("game over", "L again   J back"),
        State::Running => return,
    };
    let (w, h) = (120, 34);
    let (x, y) = ((WIDTH - w) / 2, TOP + (ROWS * CELL - h) / 2);
    fb.fill_rect(x, y, w, h, theme::BAR);
    fb.draw_rect(x, y, w, h, theme::ACCENT_DARK);
    fb.draw_text_centered(y + 6, a, theme::TEXT, None, 1);
    fb.draw_text_centered(y + 20, b, theme::MUTED, None, 1);
}

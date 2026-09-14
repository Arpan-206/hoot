//! Fireplace: the classic "Doom fire" on a 2x2 pixel grid. Ambient, cheap
//! and warm. W and S turn the heat up and down.

use sprig_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::drivers::input::Button;

pub const INFO: AppInfo = AppInfo { name: "Fireplace", group: Group::Fun, needs_network: false };

const CELL: i32 = 2;
const COLS: usize = (WIDTH / CELL) as usize;
const ROWS: usize = (HEIGHT / CELL) as usize;
const FRAME_MS: u32 = 45;
const LEVELS: usize = 37;

/// Black through deep red, orange and yellow to white.
const PALETTE: [Rgb565; LEVELS] = {
    const RGB: [(u8, u8, u8); LEVELS] = [
        (7, 7, 7), (31, 7, 7), (47, 15, 7), (71, 15, 7), (87, 23, 7), (103, 31, 7), (119, 31, 7), (143, 39, 7),
        (159, 47, 7), (175, 63, 7), (191, 71, 7), (199, 71, 7), (223, 79, 7), (223, 87, 7), (223, 87, 7), (215, 95, 7),
        (215, 95, 7), (215, 103, 15), (207, 111, 15), (207, 119, 15), (207, 127, 15), (207, 135, 23), (199, 135, 23), (199, 143, 23),
        (199, 151, 31), (191, 159, 31), (191, 159, 31), (191, 167, 39), (191, 167, 39), (191, 175, 47), (183, 175, 47), (183, 183, 47),
        (183, 183, 55), (207, 207, 111), (223, 223, 159), (239, 239, 199), (255, 255, 255),
    ];
    let mut out = [Rgb565::BLACK; LEVELS];
    let mut i = 0;
    while i < LEVELS {
        out[i] = Rgb565::new(RGB[i].0, RGB[i].1, RGB[i].2);
        i += 1;
    }
    out
};

pub struct Fireplace {
    heat: [u8; COLS * ROWS],
    rng: u32,
    /// Fuel level of the bottom row: 0 to 36.
    fuel: u8,
    next_frame_ms: u32,
}

impl Fireplace {
    pub const fn new() -> Self {
        Self { heat: [0; COLS * ROWS], rng: 0x2545_F491, fuel: 36, next_frame_ms: 0 }
    }

    fn rand(&mut self) -> u32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn step(&mut self) {
        let fuel = self.fuel;
        for x in 0..COLS {
            self.heat[(ROWS - 1) * COLS + x] = fuel;
        }
        for y in 0..ROWS - 1 {
            for x in 0..COLS {
                let r = self.rand();
                let below = self.heat[(y + 1) * COLS + x] as i32;
                let decay = (r & 3) as i32 & 1;
                let dst_x = (x as i32 - (r >> 2 & 1) as i32).clamp(0, COLS as i32 - 1) as usize;
                self.heat[y * COLS + dst_x] = (below - decay).max(0) as u8;
            }
        }
    }

    fn draw(&self, fb: &mut Framebuffer) {
        for y in 0..ROWS {
            for x in 0..COLS {
                let level = self.heat[y * COLS + x] as usize;
                fb.fill_rect(x as i32 * CELL, y as i32 * CELL, CELL, CELL, PALETTE[level.min(LEVELS - 1)]);
            }
        }
    }
}

impl App for Fireplace {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.rng = ctx.now_ms | 1;
        self.heat = [0; COLS * ROWS];
        self.next_frame_ms = ctx.now_ms;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        if ctx.input.repeat(Button::W) {
            self.fuel = (self.fuel + 4).min(36);
        }
        if ctx.input.repeat(Button::S) {
            self.fuel = self.fuel.saturating_sub(4).max(8);
        }
        if ctx.now_ms.wrapping_sub(self.next_frame_ms) < 1 << 31 {
            self.next_frame_ms = ctx.now_ms.wrapping_add(FRAME_MS);
            self.step();
            self.draw(ctx.fb);
        }
        Transition::Stay
    }
}

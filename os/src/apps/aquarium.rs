//! Aquarium: fish, bubbles and weed, drifting. Ambient. Nothing to do.

use hoot_gfx::{Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};

pub const INFO: AppInfo = AppInfo { name: "Aquarium", group: Group::Fun, needs_network: false };

const FRAME_MS: u32 = 40;
const FISH: usize = 5;
const BUBBLES: usize = 7;
const SAND_H: i32 = 12;

/// 12x8 fish facing right, two tail frames. Rows packed MSB first.
const FISH_A: [u8; 16] = [
    0b0000_0110, 0b0000_0000,
    0b0000_1111, 0b0000_0000,
    0b0011_1111, 0b1000_0000,
    0b0111_1101, 0b1101_0000,
    0b1111_1111, 0b1111_0000,
    0b0111_1111, 0b1101_0000,
    0b0011_1111, 0b1000_0000,
    0b0000_1111, 0b0000_0000,
];
const FISH_B: [u8; 16] = [
    0b0000_0110, 0b0000_0000,
    0b0000_1111, 0b0000_0000,
    0b0011_1111, 0b1001_0000,
    0b0111_1101, 0b1111_0000,
    0b1111_1111, 0b1110_0000,
    0b0111_1111, 0b1111_0000,
    0b0011_1111, 0b1001_0000,
    0b0000_1111, 0b0000_0000,
];
const FISH_W: i32 = 12;
const FISH_H: i32 = 8;

const FISH_COLORS: [Rgb565; FISH] = [
    Rgb565::hex(0xF2A33C),
    Rgb565::hex(0xE85D5D),
    Rgb565::hex(0xF7D154),
    Rgb565::hex(0x8FD3F4),
    Rgb565::hex(0xC58AF9),
];

#[derive(Clone, Copy)]
struct Fish {
    /// Position in 1/16 pixel.
    x: i32,
    y: i32,
    /// Speed in 1/16 pixel per frame, sign is direction.
    vx: i32,
    /// Vertical drift target.
    target_y: i32,
    phase: u32,
}

#[derive(Clone, Copy)]
struct Bubble {
    x: i32,
    y: i32,
    speed: i32,
    phase: u32,
}

pub struct Aquarium {
    fish: [Fish; FISH],
    bubbles: [Bubble; BUBBLES],
    rng: u32,
    tick: u32,
    next_frame_ms: u32,
}

impl Aquarium {
    pub const fn new() -> Self {
        Self {
            fish: [Fish { x: 0, y: 0, vx: 0, target_y: 0, phase: 0 }; FISH],
            bubbles: [Bubble { x: 0, y: 0, speed: 0, phase: 0 }; BUBBLES],
            rng: 0x9E37_79B9,
            tick: 0,
            next_frame_ms: 0,
        }
    }

    fn rand(&mut self, n: u32) -> i32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x % n.max(1)) as i32
    }

    fn reset(&mut self) {
        for i in 0..FISH {
            let dir = if self.rand(2) == 0 { 1 } else { -1 };
            let y = 20 + self.rand((HEIGHT - SAND_H - 30) as u32);
            self.fish[i] = Fish {
                x: self.rand(WIDTH as u32) * 16,
                y: y * 16,
                vx: dir * (6 + self.rand(10)),
                target_y: y,
                phase: self.rand(1000) as u32,
            };
        }
        for i in 0..BUBBLES {
            self.bubbles[i] = Bubble {
                x: self.rand(WIDTH as u32),
                y: HEIGHT - SAND_H - self.rand(80),
                speed: 1 + self.rand(2),
                phase: self.rand(1000) as u32,
            };
        }
    }

    fn step(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        for i in 0..FISH {
            let mut f = self.fish[i];
            f.x += f.vx;
            // Turn around a little beyond the edges, so the whole fish leaves.
            if f.x > (WIDTH + FISH_W) * 16 {
                f.vx = -f.vx.abs();
            } else if f.x < -FISH_W * 16 {
                f.vx = f.vx.abs();
            }
            // Ease towards a depth target that wanders now and then.
            if self.rand(200) == 0 {
                f.target_y = 20 + self.rand((HEIGHT - SAND_H - 30) as u32);
            }
            let dy = f.target_y * 16 - f.y;
            f.y += dy.signum() * (dy.abs() / 64).clamp(0, 4);
            self.fish[i] = f;
        }
        for i in 0..BUBBLES {
            let mut b = self.bubbles[i];
            b.y -= b.speed;
            if b.y < 14 {
                b.y = HEIGHT - SAND_H - 2;
                b.x = self.rand(WIDTH as u32);
                b.speed = 1 + self.rand(2);
            }
            self.bubbles[i] = b;
        }
    }

    fn draw(&self, fb: &mut Framebuffer) {
        // Water: bands from light to deep.
        let bands = 8;
        let band_h = (HEIGHT - SAND_H) / bands;
        for i in 0..bands {
            let t = i as u32;
            let c = Rgb565::new(
                (24 - t * 3) as u8,
                (110 - t * 10) as u8,
                (170 - t * 12) as u8,
            );
            fb.fill_rect(0, i * band_h, WIDTH, band_h + 1, c);
        }
        // Sand with a few darker grains.
        fb.fill_rect(0, HEIGHT - SAND_H, WIDTH, SAND_H, Rgb565::hex(0xC9B27C));
        for k in 0..18 {
            let x = (k * 37 + 11) % WIDTH;
            let y = HEIGHT - SAND_H + 2 + (k * 7) % (SAND_H - 4);
            fb.set(x, y, Rgb565::hex(0x9C8A5A));
        }
        // Weed: three swaying stalks.
        for (k, base_x) in [22, 96, 140].iter().enumerate() {
            let height = 26 + k as i32 * 8;
            let mut x = *base_x;
            for s in 0..height {
                let y = HEIGHT - SAND_H - s;
                let sway = tri((self.tick * 3 + (s as u32 * 9) + k as u32 * 70) % 64) - 16;
                let px = x + sway / 8;
                fb.fill_rect(px, y, 2, 1, Rgb565::hex(0x2E8B57));
                if s % 9 == 0 {
                    x += if (s / 9) % 2 == 0 { 1 } else { -1 };
                }
            }
        }
        // Bubbles.
        for b in &self.bubbles {
            let sway = tri((self.tick * 2 + b.phase) % 64) / 8 - 2;
            fb.draw_rect(b.x + sway, b.y, 3, 3, Rgb565::hex(0xBFE6FF));
        }
        // Fish, tail flapping.
        for (i, f) in self.fish.iter().enumerate() {
            let frame = (self.tick / 6 + f.phase).is_multiple_of(2);
            let sprite = if frame { &FISH_A } else { &FISH_B };
            fb.draw_bitmap(f.x / 16, f.y / 16, FISH_W, FISH_H, sprite, FISH_COLORS[i], f.vx < 0);
            // Eye.
            let eye_x = if f.vx < 0 { f.x / 16 + 2 } else { f.x / 16 + FISH_W - 3 };
            fb.set(eye_x, f.y / 16 + 3, Rgb565::BLACK);
        }
    }
}

/// Triangle wave 0..=32 over a period of 64.
fn tri(t: u32) -> i32 {
    let t = (t % 64) as i32;
    if t < 32 { t } else { 63 - t }
}

impl App for Aquarium {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.rng = ctx.now_ms | 1;
        self.reset();
        self.next_frame_ms = ctx.now_ms;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        if ctx.now_ms.wrapping_sub(self.next_frame_ms) < 1 << 31 {
            self.next_frame_ms = ctx.now_ms.wrapping_add(FRAME_MS);
            self.step();
            self.draw(ctx.fb);
        }
        Transition::Stay
    }
}

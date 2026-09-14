//! Sounds: a board of the OS sounds on seven keys. Good for trying the
//! speaker, and a little fun. J goes back.

use hoot_gfx::{CELL_HEIGHT, Framebuffer};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Sounds", group: Group::Fun, needs_network: false };

const KEYS: [(Button, &str, &str, Sound); 7] = [
    (Button::W, "W", "chime up", Sound::Done),
    (Button::A, "A", "chime down", Sound::Rest),
    (Button::S, "S", "coin", Sound::Coin),
    (Button::D, "D", "laser", Sound::Laser),
    (Button::I, "I", "bell", Sound::Bell),
    (Button::K, "K", "siren", Sound::Siren),
    (Button::L, "L", "beeps", Sound::Alarm),
];
const FLASH_MS: u32 = 300;

pub struct Sounds {
    lit: Option<(usize, u32)>,
    drawn: Option<Option<usize>>,
}

impl Sounds {
    pub const fn new() -> Self {
        Self { lit: None, drawn: None }
    }
}

impl App for Sounds {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        for (i, (button, _, _, sound)) in KEYS.iter().enumerate() {
            if ctx.input.just_pressed(*button) {
                audio::play(*sound);
                self.lit = Some((i, now.wrapping_add(FLASH_MS)));
            }
        }
        if let Some((_, until)) = self.lit
            && now.wrapping_sub(until) < 1 << 31
        {
            self.lit = None;
        }
        let lit = self.lit.map(|(i, _)| i);
        if self.drawn != Some(lit) {
            self.drawn = Some(lit);
            draw(ctx.fb, lit);
        }
        Transition::Stay
    }
}

fn draw(fb: &mut Framebuffer, lit: Option<usize>) {
    theme::screen(fb, "Sounds", "");
    let row_h = CELL_HEIGHT + 6;
    for (i, (_, key, name, _)) in KEYS.iter().enumerate() {
        let (col, row) = if i < 4 { (0, i) } else { (1, i - 4) };
        let x = 8 + col * 78;
        let y = theme::CONTENT_Y + 2 + row as i32 * row_h;
        let on = lit == Some(i);
        let (box_fill, key_color) = if on { (theme::ACCENT, theme::BG) } else { (theme::BAR, theme::ACCENT) };
        fb.fill_rect(x, y - 3, 14, row_h - 2, box_fill);
        fb.draw_text(x + 4, y, key, key_color, None);
        fb.draw_text(x + 20, y, name, if on { theme::TEXT } else { theme::MUTED }, None);
    }
    theme::footer(fb, "press a key   J back");
}

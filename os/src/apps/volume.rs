//! Volume: one screen in Settings. A/D or W/S move the level from 0 to
//! 10 with a tick at each step, L plays the chime to judge it, J goes
//! back and saves. The level lives in the config record as `sound`.

use hoot_gfx::{Framebuffer, WIDTH};
use hoot_proto::record::SOUND_MAX;

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Volume", group: Group::System, needs_network: false };

pub struct Volume {
    level: u8,
    saved: u8,
    drawn: Option<u8>,
}

impl Volume {
    pub const fn new() -> Self {
        Self { level: 0, saved: 0, drawn: None }
    }
}

impl App for Volume {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.level = ctx.store.config().sound.min(SOUND_MAX);
        self.saved = self.level;
        self.drawn = None;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let input = ctx.input;
        let up = input.repeat(Button::D) || input.repeat(Button::W);
        let down = input.repeat(Button::A) || input.repeat(Button::S);
        if up && self.level < SOUND_MAX {
            self.level += 1;
            audio::set_volume(self.level);
            audio::play(Sound::Tick);
        }
        if down && self.level > 0 {
            self.level -= 1;
            audio::set_volume(self.level);
            audio::play(Sound::Tick);
        }
        if input.just_pressed(Button::L) {
            audio::play(Sound::Done);
        }
        if self.drawn != Some(self.level) {
            self.drawn = Some(self.level);
            draw(ctx.fb, self.level);
        }
        Transition::Stay
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        if self.level != self.saved {
            let level = self.level;
            let _ = ctx.store.update_config(|c| c.sound = level);
            self.saved = level;
            info!("volume saved: {}", level);
        }
    }
}

fn draw(fb: &mut Framebuffer, level: u8) {
    theme::screen(fb, "Volume", "");
    let label: StrBuf<8> =
        if level == 0 { format(format_args!("off")) } else { format(format_args!("{level}")) };
    let color = if level == 0 { theme::MUTED } else { theme::ACCENT };
    fb.draw_text_centered(30, label.as_str(), color, None, 3);

    // Ten rising bars, lit up to the level.
    let (seg_w, gap) = (12, 2);
    let x0 = (WIDTH - (10 * seg_w + 9 * gap)) / 2;
    for i in 0..10 {
        let lit = (i as u8) < level;
        let h = 6 + i * 2;
        let fill = if lit { theme::ACCENT } else { theme::BAR };
        fb.fill_rect(x0 + i * (seg_w + gap), 92 - h, seg_w, h, fill);
    }
    theme::footer(fb, "A/D change   L test   J back");
}

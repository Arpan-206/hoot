//! Speaker test: a loud three-tone sweep at full scale, whatever the
//! Sounds setting says, plus the normal chime. If the sweep is silent the
//! fault is in the amplifier or the speaker, not in the level.

use hoot_gfx::{CELL_HEIGHT, Framebuffer};
use crate::ui::text::{StrBuf, format};

use crate::apps::{App, AppInfo, Ctx, Group, Transition, back_pressed};
use crate::audio::{self, Sound};
use crate::drivers::input::Button;
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Speaker test", group: Group::Developer, needs_network: false };

pub struct SpeakerTest {
    last: Option<&'static str>,
    drawn: bool,
}

impl SpeakerTest {
    pub const fn new() -> Self {
        Self { last: None, drawn: false }
    }
}

impl App for SpeakerTest {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, _ctx: &mut Ctx) {
        self.drawn = false;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        if ctx.input.just_pressed(Button::L) {
            audio::play(Sound::Test);
            self.last = Some("playing: full-scale sweep");
            self.drawn = false;
        }
        if ctx.input.just_pressed(Button::K) {
            audio::play(Sound::Done);
            self.last = Some("playing: chime at the set level");
            self.drawn = false;
        }
        if !self.drawn {
            self.drawn = true;
            draw(ctx.fb, ctx.store.config().sound, self.last);
        }
        Transition::Stay
    }
}

fn draw(fb: &mut Framebuffer, level: u8, last: Option<&str>) {
    theme::screen(fb, "Speaker test", "");
    let level: StrBuf<8> =
        if level == 0 { format(format_args!("off")) } else { format(format_args!("{level}/10")) };
    let mut y = theme::CONTENT_Y;
    for line in ["I2S 24 kHz, 16-bit", "DIN GP9  BCLK GP10", "LRCLK GP11, PIO1"] {
        fb.draw_text(8, y, line, theme::MUTED, None);
        y += CELL_HEIGHT + 2;
    }
    y += 4;
    fb.draw_text(8, y, "Volume:", theme::TEXT, None);
    fb.draw_text(8 + 8 * 6, y, level.as_str(), theme::ACCENT, None);
    y += CELL_HEIGHT + 6;
    fb.draw_text(8, y, last.unwrap_or("L plays 440, 1000, 2000 Hz"), theme::TEXT, None);
    theme::footer(fb, "L sweep   K chime   J back");
}

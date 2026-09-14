//! Radio and network status, with a button to connect.

use sprig_gfx::{CELL_HEIGHT, Framebuffer, Rgb565};

use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::drivers::module::Module;
use crate::net::NetState;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Network", group: Group::System, needs_network: true };

pub struct NetworkApp;

const LABEL_X: i32 = 4;
const VALUE_X: i32 = 52;

fn row(fb: &mut Framebuffer, y: i32, label: &str, value: &str, color: Rgb565) {
    fb.draw_text(LABEL_X, y, label, theme::MUTED, None);
    fb.draw_text(VALUE_X, y, value, color, None);
}

impl App for NetworkApp {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        if ctx.input.just_pressed(Button::L) || ctx.input.just_pressed(Button::D) {
            ctx.net.request_connect();
        }
        if ctx.input.just_pressed(Button::K) {
            ctx.net.request_portal();
        }

        let state = ctx.net.state();
        if state == NetState::Portal {
            crate::ui::setup::draw(ctx.fb);
            theme::footer(ctx.fb, "J back");
            return Transition::Stay;
        }
        let ssid = ctx.net.ssid();
        let has_radio = ctx.net.has_radio();
        let module = ctx.hw.module;
        let ip: StrBuf<16> = match state {
            NetState::Up(ip) => format(format_args!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])),
            _ => format(format_args!("-")),
        };

        let fb = &mut *ctx.fb;
        theme::screen(fb, "Network", "");
        let step = CELL_HEIGHT + 3;
        let mut y = theme::CONTENT_Y;
        row(fb, y, "Module", module.name(), theme::TEXT);
        y += step;
        row(fb, y, "Radio", state.label(), theme::net_color(state));
        y += step;
        let ssid_text = if ssid.is_empty() { "(not set)" } else { ssid.as_str() };
        row(fb, y, "SSID", ssid_text, theme::TEXT);
        y += step;
        row(fb, y, "IP", ip.as_str(), theme::TEXT);
        y += step * 2;
        if module == Module::PicoW && !has_radio {
            fb.draw_text(LABEL_X, y, "Built without Wi-Fi.", theme::WARN, None);
            y += CELL_HEIGHT + 1;
            fb.draw_text(LABEL_X, y, "Flash with: cargo run-w", theme::MUTED, None);
        } else if !has_radio {
            fb.draw_text(LABEL_X, y, "This module has no radio.", theme::MUTED, None);
        }
        theme::footer(fb, if has_radio { "L connect  K setup  J back" } else { "J back" });
        Transition::Stay
    }
}

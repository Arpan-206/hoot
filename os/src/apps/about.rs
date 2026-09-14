//! Version, module, Wi-Fi, uptime, power and frame-time readout.

use sprig_gfx::{CELL_HEIGHT, Framebuffer, Rgb565};

use crate::VERSION;
use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::net::NetState;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "About", group: Group::System, needs_network: false };

pub struct About;

const LABEL_X: i32 = 4;
const VALUE_X: i32 = 58;

fn row(fb: &mut Framebuffer, y: i32, label: &str, value: &str) {
    row_colored(fb, y, label, value, theme::TEXT);
}

fn row_colored(fb: &mut Framebuffer, y: i32, label: &str, value: &str, color: Rgb565) {
    fb.draw_text(LABEL_X, y, label, theme::MUTED, None);
    fb.draw_text(VALUE_X, y, value, color, None);
}

impl App for About {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }

        let module = ctx.hw.module.name();
        let net = ctx.net.state();
        let ip: StrBuf<16> = match net {
            NetState::Up(ip) => format(format_args!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])),
            other => format(format_args!("{}", other.label())),
        };
        let display_dma = ctx.hw.display_dma;
        let power = ctx.power;
        let saver: StrBuf<16> = format(format_args!(
            "{} ({})",
            if ctx.saver { "on" } else { "off" },
            match ctx.store.config().power_mode {
                sprig_proto::record::POWER_SAVER => "manual",
                sprig_proto::record::POWER_NORMAL => "manual",
                _ => "auto",
            }
        ));
        let secs = ctx.now_ms / 1000;
        let uptime: StrBuf<12> = format(format_args!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        ));
        let source = match (power.usb_known, power.usb) {
            (true, true) => "USB",
            (true, false) => "battery",
            (false, _) => "unknown",
        };
        let volts: StrBuf<20> = if power.vsys_known {
            format(format_args!(
                "{}.{:02} V {}",
                power.vsys_mv / 1000,
                (power.vsys_mv % 1000) / 10,
                source
            ))
        } else {
            format(format_args!("{source}"))
        };
        let frame: StrBuf<12> = format(format_args!("{} ms", ctx.frame_ms));

        let fb = &mut *ctx.fb;
        theme::screen(fb, "About", "");
        let mut y = theme::CONTENT_Y;
        let step = CELL_HEIGHT + 3;
        row(fb, y, "Version", VERSION);
        y += step;
        row(fb, y, "Module", module);
        y += step;
        row_colored(fb, y, "Wi-Fi", ip.as_str(), theme::net_color(net));
        y += step;
        row(fb, y, "Chip", "RP2040 125 MHz");
        y += step;
        row(fb, y, "Display", if display_dma { "ST7735 DMA" } else { "ST7735 no DMA!" });
        y += step;
        row(fb, y, "Uptime", uptime.as_str());
        y += step;
        row(fb, y, "Power", volts.as_str());
        y += step;
        row(fb, y, "Saver", saver.as_str());
        y += step;
        row(fb, y, "Frame", frame.as_str());
        theme::footer(fb, "J back");
        Transition::Stay
    }
}

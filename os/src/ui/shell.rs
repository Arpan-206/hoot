//! The home screen: a scrolling menu that launches apps and runs a few
//! system actions, and hands the active app the frame.

use sprig_gfx::{CELL_HEIGHT, WIDTH};

use crate::apps::about::{self, About};
use crate::apps::display_test::{self, DisplayTest};
use crate::apps::input_test::{self, InputTest};
use crate::apps::leds::{self, Leds};
#[cfg(feature = "wifi")]
use crate::apps::network::{self, NetworkApp};
#[cfg(feature = "wifi")]
use crate::apps::photo_frame::{self, PhotoFrame};
use crate::apps::{App, AppInfo, Ctx, Transition};
use crate::drivers::input::Button;
use crate::drivers::power::PowerStatus;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

const POWER_POLL_MS: u32 = 500;
const ROW_H: i32 = CELL_HEIGHT + 4;
/// Rows that fit between the title bar and the footer.
const VISIBLE_ROWS: usize = ((theme::FOOTER_Y - theme::CONTENT_Y) / ROW_H) as usize;
/// Upper bound on menu entries. Raise it when the registry grows.
const MENU_MAX: usize = 12;
/// How long a one-line notice such as "cache cleared" stays up.
#[cfg(feature = "wifi")]
const NOTICE_MS: u32 = 1_200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppId {
    About,
    InputTest,
    Leds,
    DisplayTest,
    #[cfg(feature = "wifi")]
    PhotoFrame,
    #[cfg(feature = "wifi")]
    Network,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Launch(AppId),
    #[cfg(feature = "wifi")]
    ClearPhotoCache,
    Reboot,
    RebootToUsb,
}

#[derive(Clone, Copy)]
struct Entry {
    name: &'static str,
    needs_network: bool,
    action: Action,
}

const fn app(info: &'static AppInfo, id: AppId) -> Entry {
    Entry { name: info.name, needs_network: info.needs_network, action: Action::Launch(id) }
}

#[cfg(feature = "wifi")]
const CLEAR_CACHE: Entry = Entry { name: "Clear photo cache", needs_network: true, action: Action::ClearPhotoCache };
const REBOOT: Entry = Entry { name: "Reboot", needs_network: false, action: Action::Reboot };
const REBOOT_USB: Entry = Entry { name: "Reboot to USB", needs_network: false, action: Action::RebootToUsb };

/// Every app and action in this build, in menu order. Entries that need the
/// network exist only in `wifi` builds, and are hidden when there is no radio.
#[cfg(feature = "wifi")]
const REGISTRY: &[Entry] = &[
    app(&about::INFO, AppId::About),
    app(&photo_frame::INFO, AppId::PhotoFrame),
    app(&network::INFO, AppId::Network),
    app(&input_test::INFO, AppId::InputTest),
    app(&leds::INFO, AppId::Leds),
    app(&display_test::INFO, AppId::DisplayTest),
    CLEAR_CACHE,
    REBOOT,
    REBOOT_USB,
];
#[cfg(not(feature = "wifi"))]
const REGISTRY: &[Entry] = &[
    app(&about::INFO, AppId::About),
    app(&input_test::INFO, AppId::InputTest),
    app(&leds::INFO, AppId::Leds),
    app(&display_test::INFO, AppId::DisplayTest),
    REBOOT,
    REBOOT_USB,
];

/// A reset that happens on the next frame, after its notice was drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingReset {
    Normal,
    Usb,
}

pub struct Shell {
    menu: [Entry; MENU_MAX],
    menu_len: usize,
    selected: usize,
    /// First visible row.
    first: usize,
    running: Option<AppId>,
    pending_reset: Option<PendingReset>,
    notice: Option<(&'static str, u32)>,
    power: PowerStatus,
    next_power_poll: u32,
    about: About,
    input_test: InputTest,
    leds: Leds,
    display_test: DisplayTest,
    #[cfg(feature = "wifi")]
    photo_frame: PhotoFrame,
    #[cfg(feature = "wifi")]
    network: NetworkApp,
}

impl Shell {
    /// Build the menu from the registry, keeping network entries only when
    /// the board has a usable radio.
    pub fn new(has_radio: bool) -> Self {
        let mut menu = [REBOOT; MENU_MAX];
        let mut menu_len = 0;
        for entry in REGISTRY {
            if (!entry.needs_network || has_radio) && menu_len < MENU_MAX {
                menu[menu_len] = *entry;
                menu_len += 1;
            }
        }
        Self {
            menu,
            menu_len,
            selected: 0,
            first: 0,
            running: None,
            pending_reset: None,
            notice: None,
            power: PowerStatus::default(),
            next_power_poll: 0,
            about: About,
            input_test: InputTest::new(),
            leds: Leds,
            display_test: DisplayTest::new(),
            #[cfg(feature = "wifi")]
            photo_frame: PhotoFrame::new(),
            #[cfg(feature = "wifi")]
            network: NetworkApp,
        }
    }

    fn entries(&self) -> &[Entry] {
        &self.menu[..self.menu_len]
    }

    fn app(&mut self, id: AppId) -> &mut dyn App {
        match id {
            AppId::About => &mut self.about,
            AppId::InputTest => &mut self.input_test,
            AppId::Leds => &mut self.leds,
            AppId::DisplayTest => &mut self.display_test,
            #[cfg(feature = "wifi")]
            AppId::PhotoFrame => &mut self.photo_frame,
            #[cfg(feature = "wifi")]
            AppId::Network => &mut self.network,
        }
    }

    /// Run one frame: either the active app or the menu.
    pub fn update(&mut self, ctx: &mut Ctx) {
        if let Some(kind) = self.pending_reset {
            // The notice from the previous frame is on screen now.
            match kind {
                PendingReset::Usb => embassy_rp::rom_data::reset_to_usb_boot(0, 0),
                PendingReset::Normal => cortex_m::peripheral::SCB::sys_reset(),
            }
            // Neither call returns. If one ever did, the watchdog would
            // reboot us from this loop.
            loop {
                cortex_m::asm::nop();
            }
        }

        if ctx.now_ms.wrapping_sub(self.next_power_poll) < u32::MAX / 2 {
            self.power = ctx.hw.power.read();
            self.next_power_poll = ctx.now_ms.wrapping_add(POWER_POLL_MS);
        }

        if let Some(id) = self.running {
            if self.app(id).update(ctx) == Transition::Exit {
                log::info!("close app");
                self.app(id).on_exit(ctx);
                self.running = None;
                ctx.fb.mark_dirty();
            }
            return;
        }

        if let Some((text, until)) = self.notice {
            if ctx.now_ms.wrapping_sub(until) < u32::MAX / 2 {
                self.notice = None;
            } else {
                self.draw_notice(ctx, text);
                return;
            }
        }

        self.handle_menu_input(ctx);
        if self.running.is_some() {
            // An app was just opened. Whatever its `on_enter` drew must reach
            // the screen untouched; the menu would paint over it otherwise.
            return;
        }
        match self.pending_reset {
            Some(PendingReset::Usb) => self.draw_notice(ctx, "USB flash mode"),
            Some(PendingReset::Normal) => self.draw_notice(ctx, "Rebooting..."),
            None => self.draw_menu(ctx),
        }
    }

    fn handle_menu_input(&mut self, ctx: &mut Ctx) {
        let n = self.menu_len;
        let up = ctx.input.repeat(Button::W) || ctx.input.repeat(Button::I);
        let down = ctx.input.repeat(Button::S) || ctx.input.repeat(Button::K);
        let select = ctx.input.just_pressed(Button::L) || ctx.input.just_pressed(Button::D);

        if up {
            self.selected = (self.selected + n - 1) % n;
        }
        if down {
            self.selected = (self.selected + 1) % n;
        }
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + VISIBLE_ROWS {
            self.first = self.selected + 1 - VISIBLE_ROWS;
        }

        if select {
            let entry = self.menu[self.selected];
            match entry.action {
                Action::Launch(id) => {
                    log::info!("open app: {}", entry.name);
                    self.running = Some(id);
                    self.app(id).on_enter(ctx);
                }
                #[cfg(feature = "wifi")]
                Action::ClearPhotoCache => {
                    let text = match photo_frame::clear_cache(ctx.store) {
                        Ok(()) => "Photo cache cleared",
                        Err(_) => "Could not clear cache",
                    };
                    self.notice = Some((text, ctx.now_ms.wrapping_add(NOTICE_MS)));
                }
                Action::Reboot => {
                    log::info!("reboot requested");
                    self.pending_reset = Some(PendingReset::Normal);
                }
                Action::RebootToUsb => {
                    log::info!("reboot to USB requested");
                    self.pending_reset = Some(PendingReset::Usb);
                }
            }
        }
    }

    fn draw_menu(&self, ctx: &mut Ctx) {
        let fb = &mut *ctx.fb;
        let status = power_label(&self.power);
        theme::screen(fb, "Sprig OS", status.as_str());

        let entries = self.entries();
        let end = (self.first + VISIBLE_ROWS).min(entries.len());
        let mut y = theme::CONTENT_Y;
        for (i, entry) in entries.iter().enumerate().take(end).skip(self.first) {
            if i == self.selected {
                fb.fill_rect(2, y - 2, WIDTH - 4, ROW_H, theme::ACCENT_DARK);
                fb.fill_rect(2, y - 2, 2, ROW_H, theme::ACCENT);
                fb.draw_text(10, y, entry.name, theme::TEXT, None);
            } else {
                fb.draw_text(10, y, entry.name, theme::MUTED, None);
            }
            y += ROW_H;
        }
        // Scroll hints at the right edge.
        if self.first > 0 {
            fb.draw_text(WIDTH - 9, theme::CONTENT_Y, "^", theme::MUTED, None);
        }
        if end < entries.len() {
            fb.draw_text(WIDTH - 9, theme::FOOTER_Y - CELL_HEIGHT - 2, "v", theme::MUTED, None);
        }
        theme::footer(fb, "W/S move    L select");
    }

    fn draw_notice(&self, ctx: &mut Ctx, text: &str) {
        let fb = &mut *ctx.fb;
        theme::screen(fb, "Sprig OS", "");
        fb.draw_text_centered(56, text, theme::TEXT, None, 1);
    }
}

/// "USB" on external power, the battery percentage otherwise, nothing when
/// the board cannot measure.
fn power_label(p: &PowerStatus) -> StrBuf<8> {
    if !p.known {
        format(format_args!(""))
    } else if p.usb {
        format(format_args!("USB"))
    } else {
        format(format_args!("{}%", p.battery_percent()))
    }
}

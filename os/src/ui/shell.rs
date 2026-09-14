//! The home screen: a two-level menu that launches apps and runs system
//! actions, and hands the active app the frame.
//!
//! Top level: one entry per app group (Frame, Fun, Tools), About, then
//! Settings and Developer. Apps never sit on the top level by themselves:
//! each app names its group in `AppInfo`, and the shell builds the group
//! submenus from that, so a new app lands in the right place on its own.
//! A group with nothing usable on this board is hidden. Settings holds the
//! network, battery saver, cache and reboot actions. Developer holds the
//! hardware test screens. J goes back, as in the apps.

use hoot_gfx::{CELL_HEIGHT, WIDTH};
use hoot_proto::record::{POWER_AUTO, POWER_NORMAL, POWER_SAVER};

use crate::apps::about::{self, About};
use crate::apps::alarm::{self, Alarm};
use crate::apps::aquarium::{self, Aquarium};
use crate::apps::bedside::{self, Bedside};
use crate::apps::clock::{self, Clock};
use crate::apps::display_test::{self, DisplayTest};
use crate::apps::fireplace::{self, Fireplace};
use crate::apps::hoot::{self, HootApp};
use crate::apps::input_test::{self, InputTest};
use crate::apps::leds::{self, Leds};
#[cfg(feature = "wifi")]
use crate::apps::messages::{self, Messages};
#[cfg(feature = "wifi")]
use crate::apps::network::{self, NetworkApp};
#[cfg(feature = "wifi")]
use crate::apps::photo_frame::{self, PhotoFrame};
use crate::apps::pomodoro::{self, Pomodoro};
#[cfg(feature = "wifi")]
use crate::apps::slideshow::{self, Slideshow};
use crate::apps::snake::{self, Snake};
use crate::apps::sounds::{self, Sounds};
use crate::apps::speaker_test::{self, SpeakerTest};
use crate::apps::stopwatch::{self, Stopwatch};
use crate::apps::volume::{self, Volume};
#[cfg(feature = "wifi")]
use crate::apps::weather::{self, Weather};
use crate::apps::{App, AppInfo, Ctx, Group, Transition};
use crate::drivers::input::Button;
use crate::drivers::power::PowerStatus;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

const ROW_H: i32 = CELL_HEIGHT + 4;
/// Rows that fit between the title bar and the footer.
const VISIBLE_ROWS: usize = ((theme::FOOTER_Y - theme::CONTENT_Y) / ROW_H) as usize;
/// Upper bound on entries in one menu. Raise it when a registry grows.
const MENU_MAX: usize = 12;
/// How long a one-line notice such as "cache cleared" stays up.
const NOTICE_MS: u32 = 1_200;
/// Quiet time on the top menu before Hoot's screen comes back.
const IDLE_TO_HOOT_MS: u32 = 60_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppId {
    Hoot,
    About,
    Volume,
    InputTest,
    Leds,
    DisplayTest,
    SpeakerTest,
    Pomodoro,
    Stopwatch,
    Clock,
    Alarm,
    Bedside,
    Fireplace,
    Aquarium,
    Sounds,
    Snake,
    #[cfg(feature = "wifi")]
    PhotoFrame,
    #[cfg(feature = "wifi")]
    Messages,
    #[cfg(feature = "wifi")]
    Slideshow,
    #[cfg(feature = "wifi")]
    Weather,
    #[cfg(feature = "wifi")]
    Network,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Main,
    /// The apps of one group.
    Group(Group),
    Settings,
}

impl Menu {
    const fn title(self) -> &'static str {
        match self {
            Menu::Main => "Hoot",
            Menu::Group(g) => g.title(),
            Menu::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Launch(AppId),
    Open(Menu),
    #[cfg(feature = "wifi")]
    ClearPhotoCache,
    /// Cycle battery saver: auto, on, off.
    BatterySaver,
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

const fn item(name: &'static str, action: Action) -> Entry {
    Entry { name, needs_network: false, action }
}

const fn group(g: Group) -> Entry {
    item(g.title(), Action::Open(Menu::Group(g)))
}

fn usable(entry: &Entry, has_radio: bool) -> bool {
    !entry.needs_network || has_radio
}

// Registries. Entries that need the network exist only in `wifi` builds,
// and are hidden at runtime when there is no radio.

/// Every app, in the order it shows inside its group. The group submenus
/// are filtered out of this list, so an app only needs to be added here.
const APPS: &[(Group, Entry)] = &[
    #[cfg(feature = "wifi")]
    (photo_frame::INFO.group, app(&photo_frame::INFO, AppId::PhotoFrame)),
    #[cfg(feature = "wifi")]
    (messages::INFO.group, app(&messages::INFO, AppId::Messages)),
    #[cfg(feature = "wifi")]
    (slideshow::INFO.group, app(&slideshow::INFO, AppId::Slideshow)),
    #[cfg(feature = "wifi")]
    (weather::INFO.group, app(&weather::INFO, AppId::Weather)),
    (fireplace::INFO.group, app(&fireplace::INFO, AppId::Fireplace)),
    (aquarium::INFO.group, app(&aquarium::INFO, AppId::Aquarium)),
    (sounds::INFO.group, app(&sounds::INFO, AppId::Sounds)),
    (snake::INFO.group, app(&snake::INFO, AppId::Snake)),
    (pomodoro::INFO.group, app(&pomodoro::INFO, AppId::Pomodoro)),
    (stopwatch::INFO.group, app(&stopwatch::INFO, AppId::Stopwatch)),
    (clock::INFO.group, app(&clock::INFO, AppId::Clock)),
    (alarm::INFO.group, app(&alarm::INFO, AppId::Alarm)),
    (bedside::INFO.group, app(&bedside::INFO, AppId::Bedside)),
    (input_test::INFO.group, app(&input_test::INFO, AppId::InputTest)),
    (leds::INFO.group, app(&leds::INFO, AppId::Leds)),
    (display_test::INFO.group, app(&display_test::INFO, AppId::DisplayTest)),
    (speaker_test::INFO.group, app(&speaker_test::INFO, AppId::SpeakerTest)),
];

const MAIN: &[Entry] = &[
    app(&hoot::INFO, AppId::Hoot),
    group(Group::Frame),
    group(Group::Fun),
    group(Group::Tools),
    app(&about::INFO, AppId::About),
    item("Settings", Action::Open(Menu::Settings)),
    group(Group::Developer),
];

#[cfg(feature = "wifi")]
const SETTINGS: &[Entry] = &[
    app(&network::INFO, AppId::Network),
    item("Battery saver", Action::BatterySaver),
    app(&volume::INFO, AppId::Volume),
    Entry { name: "Clear photo cache", needs_network: true, action: Action::ClearPhotoCache },
    item("Reboot", Action::Reboot),
    item("Reboot to USB", Action::RebootToUsb),
];
#[cfg(not(feature = "wifi"))]
const SETTINGS: &[Entry] = &[
    item("Battery saver", Action::BatterySaver),
    app(&volume::INFO, AppId::Volume),
    item("Reboot", Action::Reboot),
    item("Reboot to USB", Action::RebootToUsb),
];

/// A reset that happens on the next frame, after its notice was drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingReset {
    Normal,
    Usb,
}

pub struct Shell {
    has_radio: bool,
    current: Menu,
    entries: [Entry; MENU_MAX],
    len: usize,
    selected: usize,
    /// First visible row.
    first: usize,
    /// Selection in the main menu, restored when a submenu closes.
    main_selected: usize,
    running: Option<AppId>,
    pending_reset: Option<PendingReset>,
    notice: Option<(&'static str, u32)>,
    /// True while the right LED is pulsing for unread messages.
    cue_on: bool,
    /// Last frame with a key held, for the idle return to Hoot.
    last_input_ms: u32,
    hoot: HootApp,
    about: About,
    volume: Volume,
    input_test: InputTest,
    leds: Leds,
    display_test: DisplayTest,
    speaker_test: SpeakerTest,
    pomodoro: Pomodoro,
    stopwatch: Stopwatch,
    clock: Clock,
    alarm: Alarm,
    bedside: Bedside,
    fireplace: Fireplace,
    aquarium: Aquarium,
    sounds: Sounds,
    snake: Snake,
    #[cfg(feature = "wifi")]
    photo_frame: PhotoFrame,
    #[cfg(feature = "wifi")]
    messages: Messages,
    #[cfg(feature = "wifi")]
    slideshow: Slideshow,
    #[cfg(feature = "wifi")]
    weather: Weather,
    #[cfg(feature = "wifi")]
    network: NetworkApp,
}

impl Shell {
    pub fn new(has_radio: bool) -> Self {
        let mut shell = Self {
            has_radio,
            current: Menu::Main,
            entries: [item("", Action::Open(Menu::Main)); MENU_MAX],
            len: 0,
            selected: 0,
            first: 0,
            main_selected: 0,
            running: None,
            pending_reset: None,
            notice: None,
            cue_on: false,
            last_input_ms: 0,
            hoot: HootApp::new(),
            about: About,
            volume: Volume::new(),
            input_test: InputTest::new(),
            leds: Leds,
            display_test: DisplayTest::new(),
            speaker_test: SpeakerTest::new(),
            pomodoro: Pomodoro::new(),
            stopwatch: Stopwatch::new(),
            clock: Clock::new(),
            alarm: Alarm::new(),
            bedside: Bedside::new(),
            fireplace: Fireplace::new(),
            aquarium: Aquarium::new(),
            sounds: Sounds::new(),
            snake: Snake::new(),
            #[cfg(feature = "wifi")]
            photo_frame: PhotoFrame::new(),
            #[cfg(feature = "wifi")]
            messages: Messages::new(),
            #[cfg(feature = "wifi")]
            slideshow: Slideshow::new(),
            #[cfg(feature = "wifi")]
            weather: Weather::new(),
            #[cfg(feature = "wifi")]
            network: NetworkApp,
        };
        shell.open(Menu::Main, 0);
        shell
    }

    /// Show `menu`, keeping only entries the board can use. Group menus
    /// are built from `APPS`; an empty group is left off the top menu.
    fn open(&mut self, menu: Menu, selected: usize) {
        let has_radio = self.has_radio;
        let mut len = 0;
        let mut push = |entry: &Entry| {
            if len < MENU_MAX {
                self.entries[len] = *entry;
                len += 1;
            }
        };
        match menu {
            Menu::Group(g) => {
                for (_, entry) in APPS.iter().filter(|(gg, e)| *gg == g && usable(e, has_radio)) {
                    push(entry);
                }
            }
            Menu::Main | Menu::Settings => {
                let registry = if menu == Menu::Main { MAIN } else { SETTINGS };
                for entry in registry {
                    let shown = match entry.action {
                        Action::Open(Menu::Group(g)) => {
                            APPS.iter().any(|(gg, e)| *gg == g && usable(e, has_radio))
                        }
                        _ => usable(entry, has_radio),
                    };
                    if shown {
                        push(entry);
                    }
                }
            }
        }
        self.len = len;
        self.current = menu;
        self.selected = selected.min(self.len.saturating_sub(1));
        self.first = 0;
        self.scroll_to_selected();
    }

    fn scroll_to_selected(&mut self) {
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + VISIBLE_ROWS {
            self.first = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    /// Name of the app on screen, or the menu shown. Reported in the heartbeat.
    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub fn current_app(&self) -> &'static str {
        match self.running {
            None => match self.current {
                Menu::Main => "Menu",
                other => other.title(),
            },
            Some(id) => self.entries[..self.len]
                .iter()
                .find(|e| matches!(e.action, Action::Launch(x) if x == id))
                .map_or("App", |e| e.name),
        }
    }

    fn app(&mut self, id: AppId) -> &mut dyn App {
        match id {
            AppId::Hoot => &mut self.hoot,
            AppId::About => &mut self.about,
            AppId::Volume => &mut self.volume,
            AppId::InputTest => &mut self.input_test,
            AppId::Leds => &mut self.leds,
            AppId::DisplayTest => &mut self.display_test,
            AppId::SpeakerTest => &mut self.speaker_test,
            AppId::Pomodoro => &mut self.pomodoro,
            AppId::Stopwatch => &mut self.stopwatch,
            AppId::Clock => &mut self.clock,
            AppId::Alarm => &mut self.alarm,
            AppId::Bedside => &mut self.bedside,
            AppId::Fireplace => &mut self.fireplace,
            AppId::Aquarium => &mut self.aquarium,
            AppId::Sounds => &mut self.sounds,
            AppId::Snake => &mut self.snake,
            #[cfg(feature = "wifi")]
            AppId::PhotoFrame => &mut self.photo_frame,
            #[cfg(feature = "wifi")]
            AppId::Messages => &mut self.messages,
            #[cfg(feature = "wifi")]
            AppId::Slideshow => &mut self.slideshow,
            #[cfg(feature = "wifi")]
            AppId::Weather => &mut self.weather,
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

        // Settings saved on the setup portal page become the stored config.
        #[cfg(feature = "wifi")]
        if let Some(saved) = ctx.net.take_portal_result() {
            let outcome = ctx.store.update_config(|c| {
                c.wifi_ssid = saved.ssid;
                c.wifi_password = saved.password;
                if c.frame_server != saved.server || c.frame_name != saved.name {
                    c.frame_last_modified.clear();
                }
                c.frame_server = saved.server;
                c.frame_name = saved.name;
            });
            info!("settings from portal stored: {:?}", outcome.is_ok());
            if self.running.is_none() {
                self.notice = Some(("Settings saved", ctx.now_ms.wrapping_add(NOTICE_MS)));
            }
        }

        self.message_cue(ctx);

        // Timers keep time off screen, and Hoot lives on.
        let launchers = APPS.iter().map(|(_, e)| e).chain(MAIN.iter()).chain(SETTINGS.iter());
        for entry in launchers {
            if let Action::Launch(id) = entry.action
                && self.running != Some(id)
            {
                self.app(id).background(ctx);
            }
        }
        // A quiet minute on the top menu goes back to Hoot.
        if ctx.input.held_mask() != 0 {
            self.last_input_ms = ctx.now_ms;
        }
        if self.running.is_none()
            && self.current == Menu::Main
            && self.notice.is_none()
            && ctx.now_ms.wrapping_sub(self.last_input_ms) >= IDLE_TO_HOOT_MS
        {
            self.last_input_ms = ctx.now_ms;
            info!("open app: Hoot (idle)");
            self.running = Some(AppId::Hoot);
            self.hoot.on_enter(ctx);
        }
        // A ringing alarm takes the screen from whatever is on it.
        if self.alarm.is_ringing() && self.running != Some(AppId::Alarm) {
            if let Some(id) = self.running {
                self.app(id).on_exit(ctx);
            }
            info!("open app: Alarm (ringing)");
            self.running = Some(AppId::Alarm);
            self.alarm.on_enter(ctx);
        }

        if let Some(id) = self.running {
            if self.app(id).update(ctx) == Transition::Exit {
                info!("close app");
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

        self.handle_input(ctx);
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

    /// Pulse the right LED softly while messages wait, except inside apps
    /// that drive the LEDs themselves.
    fn message_cue(&mut self, ctx: &mut Ctx) {
        #[cfg(feature = "wifi")]
        let unread = crate::agent::unread();
        #[cfg(not(feature = "wifi"))]
        let unread = 0u8;
        let owns_leds = matches!(self.running, Some(AppId::Leds) | Some(AppId::Pomodoro) | Some(AppId::InputTest));
        if unread > 0 && !owns_leds {
            let t = (ctx.now_ms % 2400) as i32;
            let level = if t < 1200 { t / 20 } else { (2400 - t) / 20 };
            ctx.hw.led_right.set(level as u8);
            self.cue_on = true;
        } else if self.cue_on {
            ctx.hw.led_right.set(0);
            self.cue_on = false;
        }
    }

    fn handle_input(&mut self, ctx: &mut Ctx) {
        let n = self.len.max(1);
        let up = ctx.input.repeat(Button::W) || ctx.input.repeat(Button::I);
        let down = ctx.input.repeat(Button::S) || ctx.input.repeat(Button::K);
        let select = ctx.input.just_pressed(Button::L) || ctx.input.just_pressed(Button::D);
        let back = ctx.input.just_pressed(Button::J) || ctx.input.just_pressed(Button::A);

        if up {
            self.selected = (self.selected + n - 1) % n;
        }
        if down {
            self.selected = (self.selected + 1) % n;
        }
        self.scroll_to_selected();

        if back && self.current != Menu::Main {
            self.open(Menu::Main, self.main_selected);
            return;
        }
        if select && self.len > 0 {
            let entry = self.entries[self.selected];
            match entry.action {
                Action::Launch(id) => {
                    info!("open app: {}", entry.name);
                    self.running = Some(id);
                    self.app(id).on_enter(ctx);
                }
                Action::Open(menu) => {
                    if self.current == Menu::Main {
                        self.main_selected = self.selected;
                    }
                    self.open(menu, 0);
                }
                #[cfg(feature = "wifi")]
                Action::ClearPhotoCache => {
                    let text = match photo_frame::clear_cache(ctx.store) {
                        Ok(()) => "Photo cache cleared",
                        Err(_) => "Could not clear cache",
                    };
                    self.notice = Some((text, ctx.now_ms.wrapping_add(NOTICE_MS)));
                }
                Action::BatterySaver => {
                    let mut text = "Battery saver: auto";
                    let _ = ctx.store.update_config(|c| {
                        c.power_mode = match c.power_mode {
                            POWER_AUTO => POWER_SAVER,
                            POWER_SAVER => POWER_NORMAL,
                            _ => POWER_AUTO,
                        };
                        text = match c.power_mode {
                            POWER_SAVER => "Battery saver: on",
                            POWER_NORMAL => "Battery saver: off",
                            _ => "Battery saver: auto",
                        };
                    });
                    info!("{text}");
                    self.notice = Some((text, ctx.now_ms.wrapping_add(NOTICE_MS)));
                }
                Action::Reboot => {
                    info!("reboot requested");
                    self.pending_reset = Some(PendingReset::Normal);
                }
                Action::RebootToUsb => {
                    info!("reboot to USB requested");
                    self.pending_reset = Some(PendingReset::Usb);
                }
            }
        }
    }

    fn draw_menu(&self, ctx: &mut Ctx) {
        let status = power_label(&ctx.power, ctx.saver);
        let fb = &mut *ctx.fb;
        theme::screen(fb, self.current.title(), status.as_str());

        let entries = &self.entries[..self.len];
        let end = (self.first + VISIBLE_ROWS).min(entries.len());
        let mut y = theme::CONTENT_Y;
        for (i, entry) in entries.iter().enumerate().take(end).skip(self.first) {
            let is_menu = matches!(entry.action, Action::Open(_));
            if i == self.selected {
                fb.fill_rect(2, y - 2, WIDTH - 4, ROW_H, theme::ACCENT_DARK);
                fb.fill_rect(2, y - 2, 2, ROW_H, theme::ACCENT);
                fb.draw_text(10, y, entry.name, theme::TEXT, None);
            } else {
                fb.draw_text(10, y, entry.name, theme::MUTED, None);
            }
            if is_menu {
                fb.draw_text(WIDTH - 12, y, ">", theme::MUTED, None);
            }
            // Hoot's mood, when it needs something or sleeps.
            if matches!(entry.action, Action::Launch(AppId::Hoot))
                && let Some(word) = hoot::badge()
            {
                let color = if word == "zzz" { theme::MUTED } else { theme::WARN };
                fb.draw_text_right(WIDTH - 6, y, word, color, None);
            }
            // Unread badge on Messages, and on the group that holds it, so
            // the count shows from the top menu too.
            #[cfg(feature = "wifi")]
            if matches!(entry.action, Action::Launch(AppId::Messages))
                || matches!(entry.action, Action::Open(Menu::Group(g)) if g == messages::INFO.group)
            {
                let n = crate::agent::unread();
                if n > 0 {
                    let badge: StrBuf<6> = format(format_args!("{n}"));
                    fb.draw_text_right(WIDTH - 6, y, badge.as_str(), theme::ACCENT, None);
                }
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
        let hint = if self.current == Menu::Main { "W/S move    L select" } else { "L select    J back" };
        theme::footer(fb, hint);
    }

    fn draw_notice(&self, ctx: &mut Ctx, text: &str) {
        let fb = &mut *ctx.fb;
        theme::screen(fb, self.current.title(), "");
        fb.draw_text_centered(56, text, theme::TEXT, None, 1);
    }
}

/// "USB" on external power, the battery percentage when it can be measured,
/// "BAT" when only the source is known, nothing otherwise. A leading "z"
/// marks battery saver.
fn power_label(p: &PowerStatus, saver: bool) -> StrBuf<12> {
    let z = if saver { "z " } else { "" };
    if p.usb_known && p.usb {
        format(format_args!("{z}USB"))
    } else if p.vsys_known {
        format(format_args!("{z}{}%", p.battery_percent()))
    } else if p.usb_known {
        format(format_args!("{z}BAT"))
    } else {
        format(format_args!("{z}"))
    }
}

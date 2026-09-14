//! Apps are screens that the shell runs one at a time.
//!
//! # The app template
//!
//! Every app, built in or (later) loaded as WASM, has the same shape:
//!
//! ```ignore
//! pub const INFO: AppInfo = AppInfo { name: "My app", group: Group::Tools, needs_network: false };
//!
//! pub struct MyApp { /* state that survives between frames */ }
//!
//! impl App for MyApp {
//!     fn info(&self) -> &'static AppInfo { &INFO }
//!
//!     fn on_enter(&mut self, ctx: &mut Ctx) {
//!         // Runs once when the shell opens the app. Load saved state here.
//!     }
//!
//!     fn update(&mut self, ctx: &mut Ctx) -> Transition {
//!         if back_pressed(ctx.input) { return Transition::Exit; }
//!         // 1. React to input: ctx.input.just_pressed(Button::L), ...
//!         // 2. Start or poll background work: ctx.net.fetch(...), ctx.net.take_job()
//!         // 3. Draw into ctx.fb. Draw nothing to keep the last frame.
//!         Transition::Stay
//!     }
//!
//!     fn on_exit(&mut self, ctx: &mut Ctx) {
//!         // Runs once when the app closes. Switch LEDs off, save state.
//!     }
//! }
//! ```
//!
//! Rules that keep apps simple and the OS in control:
//!
//! - `update` runs about 60 times a second and must return quickly. Never
//!   sleep or wait in it. Long work is a request to a service, polled next
//!   frame. The watchdog reboots the device if the loop stalls for 3 s.
//! - The frame is only sent to the display when something was drawn. An app
//!   that draws nothing costs nothing.
//! - The J button is the shared "back" gesture. Use `back_pressed`.
//! - Persist through `ctx.store`. Fetch through `ctx.net`. Blink LEDs through
//!   `ctx.hw`. Apps never touch peripherals directly.
//!
//! To register an app, add it to the registry in `ui::shell`. Set
//! `needs_network` if it cannot work without Wi-Fi: it is then left out of
//! plain-Pico builds and hidden when there is no radio.
//!
//! Sounds: call `crate::audio::play(Sound::Tick)` and carry on. The OS
//! owns the speaker and the volume setting. An app that must finish
//! something while another screen is up implements `background`.

pub mod about;
pub mod alarm;
pub mod aquarium;
pub mod clock;
pub mod display_test;
pub mod fireplace;
pub mod hoot;
pub mod input_test;
pub mod leds;
#[cfg(feature = "wifi")]
pub mod messages;
#[cfg(feature = "wifi")]
pub mod network;
#[cfg(feature = "wifi")]
pub mod photo_frame;
pub mod pomodoro;
#[cfg(feature = "wifi")]
pub mod slideshow;
pub mod sounds;
pub mod speaker_test;
pub mod stopwatch;
pub mod volume;

use hoot_gfx::Framebuffer;

use crate::drivers::input::{Button, Input};
use crate::drivers::power::PowerStatus;
use crate::hw::Hardware;
use crate::net::NetHandle;
use crate::storage::Storage;

/// Static facts about an app, used by the launcher.
pub struct AppInfo {
    pub name: &'static str,
    /// Where the launcher files the app. Apps never sit on the top menu by
    /// themselves: the shell folds each group into its own submenu.
    pub group: Group,
    /// Apps that need the network are only built with the `wifi` feature,
    /// and the launcher hides them when the board has no usable radio.
    pub needs_network: bool,
}

/// The launcher groups. Pick one for every app; add a group here when
/// none fits, and the shell gets the submenu for free.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The photo frame and what comes with it.
    Frame,
    /// Things to watch.
    Fun,
    /// Things to use.
    Tools,
    /// About and Network: the shell places these by hand.
    System,
    /// Hardware test screens.
    Developer,
}

impl Group {
    pub const fn title(self) -> &'static str {
        match self {
            Group::Frame => "Frame",
            Group::Fun => "Fun",
            Group::Tools => "Tools",
            Group::System => "System",
            Group::Developer => "Developer",
        }
    }
}

/// Everything an app can see during one frame.
pub struct Ctx<'a> {
    pub fb: &'a mut Framebuffer,
    pub input: &'a Input,
    pub hw: &'a mut Hardware,
    pub net: &'a mut NetHandle,
    /// Only the network apps persist anything so far.
    #[cfg_attr(not(feature = "wifi"), allow(dead_code))]
    pub store: &'a mut Storage,
    /// Latest power reading, refreshed twice a second by the OS.
    pub power: PowerStatus,
    /// True when battery saver is active: poll less, expect a dim screen.
    pub saver: bool,
    /// Milliseconds since boot at the start of this frame.
    pub now_ms: u32,
    /// How long the previous frame took to update and draw.
    pub frame_ms: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transition {
    Stay,
    Exit,
}

pub trait App {
    /// Used by the launcher and, later, by the app store.
    #[allow(dead_code)]
    fn info(&self) -> &'static AppInfo;

    /// Called once when the shell launches the app.
    fn on_enter(&mut self, _ctx: &mut Ctx) {}

    /// Called once when the app exits.
    fn on_exit(&mut self, _ctx: &mut Ctx) {}

    /// Called every frame for every app that is not on screen. Timers use
    /// it to finish and sound their chime while another screen is up. Keep
    /// it cheap, and do not draw: the screen belongs to someone else.
    fn background(&mut self, _ctx: &mut Ctx) {}

    /// Called once per frame. Draw into `ctx.fb`, or draw nothing to keep
    /// the previous frame on screen.
    fn update(&mut self, ctx: &mut Ctx) -> Transition;
}

/// The shared "back" gesture.
pub fn back_pressed(input: &Input) -> bool {
    input.just_pressed(Button::J)
}

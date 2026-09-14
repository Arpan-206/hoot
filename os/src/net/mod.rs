//! Networking service and its app-facing handle.
//!
//! Apps never touch sockets. They ask the [`NetHandle`] to connect and to
//! fetch URLs, and they poll for the result once per frame. The Wi-Fi task
//! in [`wifi`] does the work. The same request/poll shape will be exposed
//! to WASM apps later, which is why nothing here is async.
//!
//! Requests travel on two lanes: `App` for the app on screen and `System`
//! for the OS agent (heartbeat, warnings, updates), so an app can never
//! block the device from being serviced. One request per lane at a time.
//! Large bodies stream straight into a blob slot in flash, so a 40 KiB
//! photo costs 4 KiB of RAM, not 40.
//!
//! The service only exists with the `wifi` Cargo feature. Without it the
//! handle reports `NoRadio` and every fetch fails with `NoNetwork`, so apps
//! need no conditional code of their own.

// Without the radio most of this module is inert.
#![cfg_attr(not(feature = "wifi"), allow(dead_code))]

#[cfg(feature = "wifi")]
pub mod dhcp;
#[cfg(feature = "wifi")]
pub mod dns;
#[cfg(feature = "wifi")]
pub mod http;
#[cfg(feature = "wifi")]
pub mod portal;
#[cfg(feature = "wifi")]
pub mod wifi;

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

pub use hoot_proto::record::FixedStr;

use crate::storage::Config;

/// Name of the open setup hotspot.
pub const PORTAL_SSID: &str = "Hoot-Setup";
/// The Sprig's address while the hotspot is up.
pub const PORTAL_IP: [u8; 4] = [192, 168, 4, 1];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetState {
    /// Plain Pico: no radio on the board.
    NoRadio,
    /// Pico W, radio not started yet.
    Off,
    /// Loading radio firmware.
    Starting,
    Joining,
    /// Joined, waiting for an address.
    Dhcp,
    Up([u8; 4]),
    JoinFailed,
    /// Was up, link went away. The service rejoins by itself.
    Lost,
    /// Running the setup hotspot and captive portal.
    Portal,
    /// Pico W, but the radio firmware partition is empty.
    NoRadioFirmware,
}

impl NetState {
    pub const fn label(self) -> &'static str {
        match self {
            NetState::NoRadio => "No wifi",
            NetState::Off => "Off",
            NetState::Starting => "Starting",
            NetState::Joining => "Joining",
            NetState::Dhcp => "Getting IP",
            NetState::Up(_) => "Connected",
            NetState::JoinFailed => "Join failed",
            NetState::Lost => "Lost",
            NetState::Portal => "Setup mode",
            NetState::NoRadioFirmware => "No radio fw",
        }
    }

    pub const fn is_up(self) -> bool {
        matches!(self, NetState::Up(_))
    }
}

/// Where a response body goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sink {
    /// Stream into a blob slot in flash, tagged with `kind` and `seq`.
    Blob { slot: u8, kind: u32, seq: u32 },
    /// Keep up to `SMALL_BODY_MAX` bytes in RAM. Read with `small_body`.
    Small,
    /// Stream a firmware image into the update partition and mark it.
    Firmware,
}

/// What to send with the request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Body {
    /// Plain GET.
    None,
    /// POST the warnings collected since the last post, as text lines.
    RecentWarnings,
    /// POST a small `application/x-www-form-urlencoded` body.
    Form(FixedStr<64>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FetchRequest {
    pub url: FixedStr<128>,
    /// Empty means no `If-Modified-Since` header.
    pub if_modified_since: FixedStr<40>,
    /// Extra header lines, each ending in `\r\n`. Used for the heartbeat.
    pub headers: FixedStr<192>,
    pub body: Body,
    pub sink: Sink,
}

impl FetchRequest {
    pub fn get(url: FixedStr<128>, sink: Sink) -> Self {
        Self { url, if_modified_since: FixedStr::new(), headers: FixedStr::new(), body: Body::None, sink }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FetchResult {
    pub status: u16,
    /// Body bytes received. Zero for 304 and other bodiless answers.
    pub len: u32,
    pub last_modified: FixedStr<40>,
    /// CRC-32 of the stored body when the sink was a blob slot.
    pub crc32: u32,
    /// `X-Sprig-Command` from the server, or empty.
    pub command: FixedStr<16>,
    /// `X-Sprig-Unread` from the server: messages waiting.
    pub unread: u8,
    /// `X-Sprig-Time` from the server, seconds since 1970 UTC, if sent.
    pub time: Option<u32>,
    /// `X-Sprig-Tz`: the frame's zone offset in minutes. Zero if not sent.
    pub tz_min: i16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FetchError {
    NoNetwork,
    Busy,
    BadUrl,
    Dns,
    Connect,
    Timeout,
    Protocol,
    Chunked,
    TooLarge,
    Storage,
    /// The updater refused: not booted, or the image did not fit.
    Update,
}

impl FetchError {
    pub const fn label(self) -> &'static str {
        match self {
            FetchError::NoNetwork => "no network",
            FetchError::Busy => "busy",
            FetchError::BadUrl => "bad URL",
            FetchError::Dns => "DNS failed",
            FetchError::Connect => "can't connect",
            FetchError::Timeout => "timed out",
            FetchError::Protocol => "bad reply",
            FetchError::Chunked => "chunked reply",
            FetchError::TooLarge => "too large",
            FetchError::Storage => "flash error",
            FetchError::Update => "update failed",
        }
    }
}

/// Settings entered on the captive portal page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortalResult {
    pub ssid: FixedStr<32>,
    pub password: FixedStr<64>,
    pub server: FixedStr<96>,
    pub name: FixedStr<24>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Idle,
    Pending,
    Done(FetchResult),
    Failed(FetchError),
}

pub const SMALL_BODY_MAX: usize = 1024;

/// Who a request belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    App = 0,
    System = 1,
}
const LANES: usize = 2;

/// State shared between the handle (shell task) and the Wi-Fi task.
pub struct Shared {
    pub state: NetState,
    pub connect_requested: bool,
    pub portal_requested: bool,
    /// Filled by the portal when the user saves; the shell persists it.
    pub portal_result: Option<PortalResult>,
    pub ssid: FixedStr<32>,
    pub password: FixedStr<64>,
    /// Photo server and frame name, shown as defaults on the portal page.
    pub server: FixedStr<96>,
    pub name: FixedStr<24>,
    /// USB power present, read from the radio chip on a Pico W. `None`
    /// until the radio is up.
    pub usb_power: Option<bool>,
    pub requests: [Option<FetchRequest>; LANES],
    pub jobs: [JobState; LANES],
    pub small_body: [[u8; SMALL_BODY_MAX]; LANES],
    pub small_len: [usize; LANES],
}

impl Shared {
    const fn new() -> Self {
        Self {
            state: NetState::NoRadio,
            connect_requested: false,
            portal_requested: false,
            portal_result: None,
            ssid: FixedStr::new(),
            password: FixedStr::new(),
            server: FixedStr::new(),
            name: FixedStr::new(),
            usb_power: None,
            requests: [None, None],
            jobs: [JobState::Idle; LANES],
            small_body: [[0; SMALL_BODY_MAX]; LANES],
            small_len: [0; LANES],
        }
    }
}

pub static SHARED: Mutex<CriticalSectionRawMutex, RefCell<Shared>> =
    Mutex::new(RefCell::new(Shared::new()));
/// Wakes the Wi-Fi task when there is something new to do.
pub static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub(crate) fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    SHARED.lock(|cell| f(&mut cell.borrow_mut()))
}

/// What apps see. Cheap to copy around; all state lives in `SHARED`.
pub struct NetHandle {
    has_radio: bool,
}

impl NetHandle {
    pub fn new(has_radio: bool, cfg: &Config) -> Self {
        with(|s| {
            s.state = if has_radio { NetState::Off } else { NetState::NoRadio };
            s.ssid = cfg.wifi_ssid;
            s.password = cfg.wifi_password;
            s.server = cfg.frame_server;
            s.name = cfg.frame_name;
        });
        Self { has_radio }
    }

    pub fn has_radio(&self) -> bool {
        self.has_radio
    }

    pub fn state(&self) -> NetState {
        with(|s| s.state)
    }

    /// USB power as seen by the radio chip (Pico W). `None` when unknown.
    pub fn usb_power(&self) -> Option<bool> {
        with(|s| s.usb_power)
    }

    pub fn ssid(&self) -> FixedStr<32> {
        with(|s| s.ssid)
    }

    /// Store new Wi-Fi credentials. They are used on the next join.
    #[allow(dead_code)]
    pub fn set_credentials(&mut self, ssid: &str, password: &str) {
        with(|s| {
            s.ssid.set(ssid);
            s.password.set(password);
        });
    }

    /// Open the setup hotspot and captive portal. The radio is powered if
    /// it is not yet. Nothing happens without a radio.
    pub fn request_portal(&mut self) {
        if !self.has_radio {
            return;
        }
        info!("net: setup portal requested");
        with(|s| {
            s.connect_requested = true;
            s.portal_requested = true;
        });
        WAKE.signal(());
    }

    /// Settings saved on the portal page, once. The caller persists them.
    pub fn take_portal_result(&mut self) -> Option<PortalResult> {
        with(|s| s.portal_result.take())
    }

    /// Ask the service to bring the network up. Safe to call repeatedly.
    pub fn request_connect(&mut self) {
        if !self.has_radio {
            return;
        }
        info!("net: connect requested");
        with(|s| s.connect_requested = true);
        WAKE.signal(());
    }

    /// Start a fetch on the app lane. Poll `take_job` each frame for the outcome.
    pub fn fetch(&mut self, req: FetchRequest) -> Result<(), FetchError> {
        self.fetch_on(Lane::App, req)
    }

    /// App-lane job state. A finished job is returned once, then reset to Idle.
    pub fn take_job(&mut self) -> JobState {
        self.take_job_on(Lane::App)
    }

    /// Start a fetch on `lane`.
    pub fn fetch_on(&mut self, lane: Lane, req: FetchRequest) -> Result<(), FetchError> {
        if !self.has_radio {
            return Err(FetchError::NoNetwork);
        }
        with(|s| {
            if !s.state.is_up() {
                return Err(FetchError::NoNetwork);
            }
            if s.jobs[lane as usize] == JobState::Pending {
                return Err(FetchError::Busy);
            }
            s.requests[lane as usize] = Some(req);
            s.jobs[lane as usize] = JobState::Pending;
            Ok(())
        })?;
        WAKE.signal(());
        Ok(())
    }

    /// Job state on `lane`. A finished job is returned once, then reset to Idle.
    pub fn take_job_on(&mut self, lane: Lane) -> JobState {
        with(|s| {
            let job = &mut s.jobs[lane as usize];
            match *job {
                JobState::Done(_) | JobState::Failed(_) => core::mem::replace(job, JobState::Idle),
                other => other,
            }
        })
    }

    /// Read the body of the last `Sink::Small` fetch on `lane`.
    pub fn small_body_on<R>(&self, lane: Lane, f: impl FnOnce(&[u8]) -> R) -> R {
        with(|s| f(&s.small_body[lane as usize][..s.small_len[lane as usize]]))
    }
}

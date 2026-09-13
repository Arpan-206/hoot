//! Networking service and its app-facing handle.
//!
//! Apps never touch sockets. They ask the [`NetHandle`] to connect and to
//! fetch URLs, and they poll for the result once per frame. The Wi-Fi task
//! in [`wifi`] does the work. The same request/poll shape will be exposed
//! to WASM apps later, which is why nothing here is async.
//!
//! One fetch runs at a time. Large bodies stream straight into a blob slot
//! in flash, so a 40 KiB photo costs 4 KiB of RAM, not 40.
//!
//! The service only exists with the `wifi` Cargo feature. Without it the
//! handle reports `NoRadio` and every fetch fails with `NoNetwork`, so apps
//! need no conditional code of their own.

// Without the radio most of this module is inert.
#![cfg_attr(not(feature = "wifi"), allow(dead_code))]

#[cfg(feature = "wifi")]
pub mod http;
#[cfg(feature = "wifi")]
pub mod wifi;

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

pub use sprig_proto::record::FixedStr;

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
        }
    }

    pub const fn is_up(self) -> bool {
        matches!(self, NetState::Up(_))
    }
}

/// Where a response body goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sink {
    /// Stream into a blob slot in flash, tagged with `kind`.
    Blob { slot: u8, kind: u32 },
    /// Keep up to `SMALL_BODY_MAX` bytes in RAM. Read with `small_body`.
    Small,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FetchRequest {
    pub url: FixedStr<128>,
    /// Empty means no `If-Modified-Since` header.
    pub if_modified_since: FixedStr<40>,
    pub sink: Sink,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FetchResult {
    pub status: u16,
    /// Body bytes received. Zero for 304 and other bodiless answers.
    pub len: u32,
    pub last_modified: FixedStr<40>,
    /// CRC-32 of the stored body when the sink was a blob slot.
    pub crc32: u32,
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
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Idle,
    Pending,
    Done(FetchResult),
    Failed(FetchError),
}

pub const SMALL_BODY_MAX: usize = 512;

/// State shared between the handle (shell task) and the Wi-Fi task.
pub struct Shared {
    pub state: NetState,
    pub connect_requested: bool,
    pub ssid: FixedStr<32>,
    pub password: FixedStr<64>,
    pub request: Option<FetchRequest>,
    pub job: JobState,
    pub small_body: [u8; SMALL_BODY_MAX],
    pub small_len: usize,
}

impl Shared {
    const fn new() -> Self {
        Self {
            state: NetState::NoRadio,
            connect_requested: false,
            ssid: FixedStr::new(),
            password: FixedStr::new(),
            request: None,
            job: JobState::Idle,
            small_body: [0; SMALL_BODY_MAX],
            small_len: 0,
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
    pub fn new(has_radio: bool, ssid: &str, password: &str) -> Self {
        with(|s| {
            s.state = if has_radio { NetState::Off } else { NetState::NoRadio };
            s.ssid.set(ssid);
            s.password.set(password);
        });
        Self { has_radio }
    }

    pub fn has_radio(&self) -> bool {
        self.has_radio
    }

    pub fn state(&self) -> NetState {
        with(|s| s.state)
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

    /// Ask the service to bring the network up. Safe to call repeatedly.
    pub fn request_connect(&mut self) {
        if !self.has_radio {
            return;
        }
        log::info!("net: connect requested");
        with(|s| s.connect_requested = true);
        WAKE.signal(());
    }

    /// Start a fetch. Poll `take_job` each frame for the outcome.
    pub fn fetch(&mut self, req: FetchRequest) -> Result<(), FetchError> {
        if !self.has_radio {
            return Err(FetchError::NoNetwork);
        }
        with(|s| {
            if !s.state.is_up() {
                return Err(FetchError::NoNetwork);
            }
            if s.job == JobState::Pending {
                return Err(FetchError::Busy);
            }
            s.request = Some(req);
            s.job = JobState::Pending;
            Ok(())
        })?;
        WAKE.signal(());
        Ok(())
    }

    /// Current job state. A finished job is returned once, then reset to Idle.
    pub fn take_job(&mut self) -> JobState {
        with(|s| match s.job {
            JobState::Done(_) | JobState::Failed(_) => core::mem::replace(&mut s.job, JobState::Idle),
            other => other,
        })
    }

    /// Read the body of the last `Sink::Small` fetch.
    #[allow(dead_code)]
    pub fn small_body<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        with(|s| f(&s.small_body[..s.small_len]))
    }
}

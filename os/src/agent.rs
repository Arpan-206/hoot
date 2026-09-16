//! The OS agent: keeps every Sprig serviceable from the photo server, no
//! matter which app is on screen.
//!
//! Every 30 s it polls `<server>/device/<name>` with heartbeat headers
//! (version, uptime, module, app on screen, last warning) and carries out
//! the one command the server may answer with: reboot, clear-cache,
//! refetch, portal, or update. It posts new warnings to `/log/<name>`, and
//! it checks `/firmware/version.txt` a minute after boot and every six
//! hours, streaming a differing version into the update partition and
//! rebooting into it. All requests use the system lane of the network
//! service, so an app can never block them.

use portable_atomic::{AtomicU8, Ordering};

use crate::VERSION;
use crate::apps::photo_frame;
use crate::clock::{self, Source};
use crate::net::{Body, FetchRequest, FetchResult, FixedStr, JobState, Lane, NetHandle, Sink};
use crate::storage::{Config, Storage};
use hoot_proto::pet::GOAL_NAME_MAX;
use hoot_proto::record::{POWER_AUTO, POWER_NORMAL, POWER_SAVER};
use crate::ui::text::{StrBuf, format};

const FIRST_POLL_MS: u32 = 5_000;
const HEARTBEAT_MS: u32 = 30_000;
/// Heartbeat interval in battery saver mode.
const HEARTBEAT_SAVER_MS: u32 = 300_000;
const RETRY_MS: u32 = 15_000;
const OTA_FIRST_MS: u32 = 60_000;
const OTA_EVERY_MS: u32 = 6 * 60 * 60 * 1000;
const LOG_POST_MS: u32 = 60_000;
/// A new server or name has this long to answer a heartbeat before the
/// device goes back to the old ones.
const SERVER_TRIAL_MS: u32 = 30 * 60_000;

/// Unread messages waiting on the server, from the last heartbeat.
static UNREAD: AtomicU8 = AtomicU8::new(0);

pub fn unread() -> u8 {
    UNREAD.load(Ordering::Relaxed)
}

pub fn set_unread(n: u8) {
    UNREAD.store(n, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    None,
    Heartbeat,
    Warnings,
    Version,
    Firmware,
    Goals,
}

pub struct Agent {
    module: &'static str,
    started: bool,
    job: Job,
    next_poll_ms: u32,
    next_ota_ms: u32,
    next_log_ms: u32,
    check_update: bool,
    /// The server's goal names changed: fetch them.
    fetch_goals: bool,
    goals_stamp: u32,
    fails: u8,
    /// A heartbeat has been answered since boot: the server is reachable.
    heartbeat_ok: bool,
    /// Uptime when a server trial began, for boards without a clock.
    trial_started_ms: u32,
}

fn due(now: u32, at: u32) -> bool {
    now.wrapping_sub(at) < 1 << 31
}

fn url(cfg: &Config, path: &str, name: bool) -> FixedStr<128> {
    let mut u = FixedStr::new();
    u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
    u.push_str(path);
    if name {
        u.push_str(cfg.frame_name.as_str());
    }
    u
}

impl Agent {
    pub const fn new(module: &'static str) -> Self {
        Self {
            module,
            started: false,
            job: Job::None,
            next_poll_ms: 0,
            next_ota_ms: 0,
            next_log_ms: 0,
            check_update: false,
            fetch_goals: false,
            goals_stamp: 0,
            heartbeat_ok: false,
            trial_started_ms: 0,
            fails: 0,
        }
    }

    pub fn heartbeat_ok(&self) -> bool {
        self.heartbeat_ok
    }

    /// Run once per frame. `app` is the name of the screen currently shown.
    pub fn update(&mut self, net: &mut NetHandle, store: &mut Storage, now: u32, app: &'static str, saver: bool) {
        if !net.has_radio() {
            return;
        }
        if !self.started {
            self.started = true;
            self.next_poll_ms = now.wrapping_add(FIRST_POLL_MS);
            self.next_ota_ms = now.wrapping_add(OTA_FIRST_MS);
            self.next_log_ms = now;
            // Bring the network up at boot, whatever app is open.
            if !store.config().wifi_ssid.is_empty() {
                net.request_connect();
            }
        }

        self.check_server_trial(net, store, now);

        if self.job != Job::None {
            match net.take_job_on(Lane::System) {
                JobState::Done(r) => {
                    let job = core::mem::replace(&mut self.job, Job::None);
                    match job {
                        Job::Heartbeat => self.finish_heartbeat(net, store, now, r),
                        Job::Version => self.finish_version(net, store, r),
                        Job::Firmware => self.finish_firmware(r),
                        Job::Goals => self.finish_goals(net, store, r),
                        Job::Warnings | Job::None => {}
                    }
                }
                JobState::Failed(e) => {
                    let job = core::mem::replace(&mut self.job, Job::None);
                    self.fails = self.fails.saturating_add(1);
                    warn!("agent: request failed: {} (failure {})", e.label(), self.fails);
                    if job == Job::Heartbeat {
                        self.next_poll_ms = now.wrapping_add(RETRY_MS);
                    }
                }
                _ => {}
            }
            return;
        }

        if !net.state().is_up() {
            return;
        }
        if self.check_update || due(now, self.next_ota_ms) {
            self.start_version_check(net, store, now);
        } else if due(now, self.next_poll_ms) {
            self.start_heartbeat(net, store, now, app, saver);
        } else if self.fetch_goals {
            self.start_goals(net, store);
        } else if crate::logging::unsent() > 0 && due(now, self.next_log_ms) {
            self.start_warnings(net, store, now);
        }
    }

    /// A new server or name is on trial. Give it up when the time is over
    /// and no heartbeat got through: back to the previous settings.
    fn check_server_trial(&mut self, net: &mut NetHandle, store: &mut Storage, now: u32) {
        let until = store.config().server_trial_until;
        if until == 0 {
            return;
        }
        let expired = match clock::now_secs(now) {
            Some(secs) if until > 1 => secs >= until,
            _ => now.wrapping_sub(self.trial_started_ms) >= SERVER_TRIAL_MS,
        };
        if !expired {
            return;
        }
        warn!("agent: new server never answered; going back to the old one");
        let _ = store.update_config(|c| {
            if !c.prev_server.is_empty() {
                c.frame_server = c.prev_server;
            }
            if !c.prev_name.is_empty() {
                c.frame_name = c.prev_name;
            }
            c.prev_server.clear();
            c.prev_name.clear();
            c.server_trial_until = 0;
            c.frame_last_modified.clear();
        });
        net.apply_config(store.config());
        self.next_poll_ms = now.wrapping_add(2_000);
    }

    fn start(&mut self, net: &mut NetHandle, job: Job, request: FetchRequest) {
        match net.fetch_on(Lane::System, request) {
            Ok(()) => self.job = job,
            Err(e) => warn!("agent: cannot start request: {}", e.label()),
        }
    }

    fn headers(&self, now: u32, app: &'static str, saver: bool) -> FixedStr<320> {
        let mut last = [0u8; 48];
        let n = crate::logging::latest(&mut last);
        let last = core::str::from_utf8(&last[..n]).unwrap_or("");
        let text: StrBuf<320> = format(format_args!(
            "X-Sprig-Version: {}\r\nX-Sprig-Uptime: {}\r\nX-Sprig-Module: {}\r\nX-Sprig-App: {}\r\nX-Sprig-Saver: {}\r\nX-Sprig-Fails: {}\r\nX-Sprig-Pet: {}\r\nX-Sprig-Error: {}\r\n",
            VERSION,
            now / 1000,
            self.module,
            app,
            if saver { "on" } else { "off" },
            self.fails,
            crate::apps::hoot::status().as_str(),
            last
        ));
        FixedStr::truncated(text.as_str())
    }

    fn start_heartbeat(&mut self, net: &mut NetHandle, store: &Storage, now: u32, app: &'static str, saver: bool) {
        let mut request = FetchRequest::get(url(store.config(), "/device/", true), Sink::Small);
        request.headers = self.headers(now, app, saver);
        self.next_poll_ms = now.wrapping_add(if saver { HEARTBEAT_SAVER_MS } else { HEARTBEAT_MS });
        self.start(net, Job::Heartbeat, request);
    }

    fn start_warnings(&mut self, net: &mut NetHandle, store: &Storage, now: u32) {
        let mut request = FetchRequest::get(url(store.config(), "/log/", true), Sink::Small);
        request.body = Body::RecentWarnings;
        self.next_log_ms = now.wrapping_add(LOG_POST_MS);
        self.start(net, Job::Warnings, request);
    }

    fn start_goals(&mut self, net: &mut NetHandle, store: &Storage) {
        let request = FetchRequest::get(url(store.config(), "/goals/", true), Sink::Small);
        self.fetch_goals = false;
        self.start(net, Job::Goals, request);
    }

    /// Five lines from the server become the five customisable goal names.
    /// An empty line keeps the built-in name.
    fn finish_goals(&mut self, net: &mut NetHandle, store: &mut Storage, r: FetchResult) {
        if r.status != 200 {
            warn!("goals: HTTP {}", r.status);
            return;
        }
        let mut names: [FixedStr<GOAL_NAME_MAX>; 5] = Default::default();
        net.small_body_on(Lane::System, |body| {
            let text = core::str::from_utf8(body).unwrap_or("");
            for (slot, line) in text.lines().take(5).enumerate() {
                names[slot] = FixedStr::truncated(line.trim_end_matches('\r').trim());
            }
        });
        let stamp = self.goals_stamp;
        let _ = store.update_config(|c| {
            c.goal_names = names;
            c.goals_stamp = stamp;
        });
        info!("goals: updated from the server (stamp {})", stamp);
    }

    fn start_version_check(&mut self, net: &mut NetHandle, store: &Storage, now: u32) {
        let request = FetchRequest::get(url(store.config(), "/firmware/version.txt", false), Sink::Small);
        self.check_update = false;
        self.next_ota_ms = now.wrapping_add(OTA_EVERY_MS);
        info!("ota: checking {}", request.url.as_str());
        self.start(net, Job::Version, request);
    }

    fn start_firmware(&mut self, net: &mut NetHandle, store: &Storage) {
        let request = FetchRequest::get(url(store.config(), "/firmware/hoot.bin", false), Sink::Firmware);
        info!("ota: downloading {}", request.url.as_str());
        self.start(net, Job::Firmware, request);
    }

    fn finish_heartbeat(&mut self, net: &mut NetHandle, store: &mut Storage, now: u32, r: FetchResult) {
        if r.status / 100 != 2 && r.status != 304 {
            warn!("agent: heartbeat answered HTTP {}", r.status);
            self.fails = self.fails.saturating_add(1);
        } else {
            self.fails = 0;
            self.heartbeat_ok = true;
            if store.config().server_trial_until != 0 {
                info!("agent: the new server answers; change confirmed");
                let _ = store.update_config(|c| {
                    c.prev_server.clear();
                    c.prev_name.clear();
                    c.server_trial_until = 0;
                });
            }
            set_unread(r.unread);
            if r.goals_stamp != 0 && r.goals_stamp != store.config().goals_stamp {
                self.goals_stamp = r.goals_stamp;
                self.fetch_goals = true;
            }
            if let Some(utc) = r.time {
                let was_set = clock::source() != Source::Unset;
                clock::set(utc.wrapping_add_signed(r.tz_min as i32 * 60), now, Source::Server);
                if !was_set {
                    info!("clock set from server, zone offset {} min", r.tz_min);
                }
            }
        }
        if !r.command.is_empty() {
            self.run_command(net, store, now, r.command.as_str());
        }
    }

    /// One instruction from the server, delivered with a heartbeat.
    fn run_command(&mut self, net: &mut NetHandle, store: &mut Storage, now: u32, command: &str) {
        info!("command from server: {command}");
        if let Some(from) = command.strip_prefix("hug") {
            crate::apps::hoot::hug_from(from.trim_start_matches(':').trim());
            return;
        }
        // Remote service: move the device to another server, rename it,
        // or give it a key. The photo is fetched afresh afterwards. A new
        // server or name is on trial: see `check_server_trial`.
        let trial_until = clock::now_secs(now).map_or(1, |s| s + SERVER_TRIAL_MS / 1000);
        if let Some(v) = command.strip_prefix("server:") {
            self.trial_started_ms = now;
            let _ = store.update_config(|c| {
                if c.server_trial_until == 0 {
                    c.prev_server = c.frame_server;
                    c.prev_name = c.frame_name;
                }
                c.server_trial_until = trial_until;
                c.frame_server.set(v.trim());
                c.frame_last_modified.clear();
            });
        } else if let Some(v) = command.strip_prefix("name:") {
            self.trial_started_ms = now;
            let _ = store.update_config(|c| {
                if c.server_trial_until == 0 {
                    c.prev_server = c.frame_server;
                    c.prev_name = c.frame_name;
                }
                c.server_trial_until = trial_until;
                c.frame_name.set(v.trim());
                c.frame_last_modified.clear();
            });
        } else if let Some(v) = command.strip_prefix("key:") {
            let _ = store.update_config(|c| {
                c.device_key.set(v.trim());
            });
        }
        if command.starts_with("server:") || command.starts_with("name:") || command.starts_with("key:") {
            net.apply_config(store.config());
            info!("agent: settings changed by command");
            self.next_poll_ms = now.wrapping_add(2_000);
            return;
        }
        match command {
            "reboot" => cortex_m::peripheral::SCB::sys_reset(),
            "clear-cache" => {
                let _ = photo_frame::clear_cache(store);
            }
            // Forgetting the timestamp makes the next photo poll fetch again.
            "refetch" => {
                let _ = store.update_config(|c| c.frame_last_modified.clear());
            }
            "portal" => net.request_portal(),
            "hoot-reset" => crate::apps::hoot::reset(),
            "update" => {
                self.check_update = true;
                self.next_ota_ms = now;
            }
            "saver-on" | "saver-off" | "saver-auto" => {
                let mode = match command {
                    "saver-on" => POWER_SAVER,
                    "saver-off" => POWER_NORMAL,
                    _ => POWER_AUTO,
                };
                let _ = store.update_config(|c| c.power_mode = mode);
            }
            other => warn!("unknown command '{other}'"),
        }
    }

    fn finish_version(&mut self, net: &mut NetHandle, store: &Storage, r: FetchResult) {
        if r.status != 200 {
            info!("ota: no version file (HTTP {})", r.status);
            return;
        }
        let differs = net.small_body_on(Lane::System, |body| {
            let published = core::str::from_utf8(body).unwrap_or("").trim();
            info!("ota: published {published}, running {VERSION}");
            !published.is_empty() && published != VERSION
        });
        if differs {
            self.start_firmware(net, store);
        }
    }

    fn finish_firmware(&mut self, r: FetchResult) {
        if r.status == 200 && r.len > 0 {
            info!("ota: {} bytes written, rebooting into the new firmware", r.len);
            cortex_m::peripheral::SCB::sys_reset();
        }
        warn!("ota: download failed (HTTP {}, {} bytes)", r.status, r.len);
    }
}

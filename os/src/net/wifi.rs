//! The Wi-Fi service task for the Pico W.
//!
//! It powers the radio only when something asks for the network, joins the
//! configured access point, gets an address by DHCP, then serves fetch
//! requests from `SHARED` until the link drops, and then rejoins.

use cyw43::{A4, Aligned, JoinOptions, PowerManagementMode};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use embassy_executor::Spawner;
use embassy_net::{Config, StackResources};
use embassy_rp::Peri;
use embassy_rp::clocks::RoscRng;
use embassy_rp::dma;
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::{DMA_CH1, PIN_24, PIN_29, PIO0};
use embassy_rp::pio::Pio;
use embassy_time::{Duration, Timer, with_timeout};
use static_cell::StaticCell;

use super::{JobState, Lane, NetState, SMALL_BODY_MAX, Sink, WAKE, http, portal, with};
use crate::hw::Irqs;
use crate::storage::{FlashMutex, xip};

type Cyw43Runner = cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>;

const DHCP_TIMEOUT: Duration = Duration::from_secs(20);
/// The radio chip's GPIO 2 is the USB power sense on a Pico W.
const RADIO_GPIO_VBUS: u8 = 2;
/// How often to ask the radio for the USB sense while idle.
const VBUS_POLL_TICKS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_secs(5);
/// After this many failed joins in a row the setup portal opens.
const MAX_JOIN_FAILURES: u8 = 3;

#[embassy_executor::task]
async fn cyw43_runner(runner: Cyw43Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

/// The radio firmware partition: header, then firmware, CLM and NVRAM
/// blobs, each padded to 4 bytes. Written once by `tools/mkradio.py`.
/// A 4-byte aligned blob in flash, as the radio driver wants it.
type Blob = &'static Aligned<A4, [u8]>;

fn radio_firmware() -> Option<(Blob, Blob, Blob)> {
    use crate::board::flash_map::{RADIO_SIZE, RADIO_START};
    let header = xip(RADIO_START, 20);
    if &header[..4] != b"SRAD" || u32::from_le_bytes(header[4..8].try_into().ok()?) != 1 {
        return None;
    }
    let len = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap()) as usize;
    let (fw_len, clm_len, nvram_len) = (len(8), len(12), len(16));
    let pad4 = |n: usize| n.div_ceil(4) * 4;
    let fw_off = 0x100;
    let clm_off = fw_off + pad4(fw_len);
    let nvram_off = clm_off + pad4(clm_len);
    if nvram_off + nvram_len > RADIO_SIZE as usize || fw_len < 1024 {
        return None;
    }
    let blob = |off: usize, len: usize| -> Blob {
        let bytes = xip(RADIO_START + off as u32, len);
        // SAFETY: `Aligned<A4, [u8]>` is a transparent wrapper with 4-byte
        // alignment, and every blob offset is a multiple of 4.
        unsafe { &*(bytes as *const [u8] as *const Aligned<A4, [u8]>) }
    };
    Some((blob(fw_off, fw_len), blob(clm_off, clm_len), blob(nvram_off, nvram_len)))
}

/// Pins that the radio owns on a Pico W. Handed over whole by `main`.
pub struct RadioPins {
    pub pwr: Peri<'static, embassy_rp::peripherals::PIN_23>,
    pub cs: Peri<'static, embassy_rp::peripherals::PIN_25>,
    pub dio: Peri<'static, PIN_24>,
    pub clk: Peri<'static, PIN_29>,
    pub pio: Peri<'static, PIO0>,
    pub dma: Peri<'static, DMA_CH1>,
}

#[embassy_executor::task]
pub async fn wifi_service(spawner: Spawner, pins: RadioPins, flash: &'static FlashMutex) {
    // Do not power the radio until an app asks for the network.
    while !with(|s| s.connect_requested) {
        WAKE.wait().await;
    }
    with(|s| s.state = NetState::Starting);
    info!("wifi: powering radio, loading firmware");

    let pwr = Output::new(pins.pwr, embassy_rp::gpio::Level::Low);
    let cs = Output::new(pins.cs, embassy_rp::gpio::Level::High);
    let mut pio = Pio::new(pins.pio, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        pins.dio,
        pins.clk,
        dma::Channel::new(pins.dma, Irqs),
    );

    let Some((fw, clm, nvram)) = radio_firmware() else {
        warn!("wifi: no radio firmware in flash; flash target/radio.bin at 0x10148000");
        with(|s| s.state = NetState::NoRadioFirmware);
        loop {
            Timer::after_secs(3600).await;
        }
    };

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let (device, mut control, runner) =
        cyw43::new(STATE.init(cyw43::State::new()), pwr, spi, fw, nvram).await;
    spawner.spawn(cyw43_runner(runner).unwrap());
    info!("wifi: firmware loaded, initialising");
    control.init(clm).await;
    control.set_power_management(PowerManagementMode::PowerSave).await;
    let usb = control.gpio_get(RADIO_GPIO_VBUS).await;
    with(|s| s.usb_power = Some(usb));
    info!("wifi: radio ready, usb power: {}", usb);

    // Sockets: DHCP client, DNS client, HTTP client, and the portal's DHCP,
    // DNS and HTTP servers.
    static RESOURCES: StaticCell<StackResources<6>> = StaticCell::new();
    let seed = RoscRng.next_u64();
    let (stack, runner) = embassy_net::new(
        device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_runner(runner).unwrap());

    let mut small = [0u8; SMALL_BODY_MAX];
    let mut failures: u8 = 0;
    loop {
        let (ssid, password, portal_wanted) = with(|s| (s.ssid, s.password, s.portal_requested));

        // Setup portal: on request, when nothing is configured, or when the
        // saved network keeps refusing us.
        if portal_wanted || ssid.is_empty() || failures >= MAX_JOIN_FAILURES {
            with(|s| {
                s.portal_requested = false;
                s.state = NetState::Portal;
            });
            control.leave().await;
            info!("portal: opening setup hotspot");
            if let Some(result) = portal::run(&mut control, stack).await {
                info!("portal: settings saved, network '{}'", result.ssid.as_str());
                with(|s| {
                    s.ssid = result.ssid;
                    s.password = result.password;
                    s.server = result.server;
                    s.name = result.name;
                    s.portal_result = Some(result);
                });
            }
            failures = 0;
            if with(|s| s.ssid.is_empty()) {
                // Still nothing to join: reopen the portal after a moment.
                Timer::after_secs(1).await;
            }
            continue;
        }

        with(|s| s.state = NetState::Joining);
        info!("wifi: joining '{}'", ssid.as_str());
        let options = if password.is_empty() {
            JoinOptions::new_open()
        } else {
            JoinOptions::new(password.as_str().as_bytes())
        };
        if let Err(e) = control.join(ssid.as_str(), options).await {
            warn!("wifi: join failed: {:?}", e);
            with(|s| s.state = NetState::JoinFailed);
            failures += 1;
            Timer::after(RETRY_DELAY).await;
            continue;
        }

        with(|s| s.state = NetState::Dhcp);
        info!("wifi: joined, waiting for DHCP");
        if with_timeout(DHCP_TIMEOUT, stack.wait_config_up()).await.is_err() {
            warn!("wifi: no DHCP lease within {} s", DHCP_TIMEOUT.as_secs());
            with(|s| s.state = NetState::JoinFailed);
            failures += 1;
            control.leave().await;
            Timer::after(RETRY_DELAY).await;
            continue;
        }
        failures = 0;
        let ip = stack
            .config_v4()
            .map(|c| c.address.address().octets())
            .unwrap_or([0; 4]);
        with(|s| s.state = NetState::Up(ip));
        info!("wifi: up, ip {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);

        // Serve requests until the link drops.
        let mut ticks: u32 = 0;
        loop {
            ticks = ticks.wrapping_add(1);
            if ticks.is_multiple_of(VBUS_POLL_TICKS) {
                let usb = control.gpio_get(RADIO_GPIO_VBUS).await;
                with(|s| s.usb_power = Some(usb));
            }
            if !stack.is_link_up() || !stack.is_config_up() {
                warn!("wifi: link lost, rejoining");
                with(|s| s.state = NetState::Lost);
                break;
            }
            if with(|s| s.portal_requested) {
                break;
            }
            // The system lane goes first: the device must stay serviceable
            // whatever the app on screen is doing.
            let request = with(|s| {
                for lane in [Lane::System, Lane::App] {
                    if let Some(r) = s.requests[lane as usize].take() {
                        return Some((lane, r));
                    }
                }
                None
            });
            match request {
                Some((lane, req)) => {
                    info!("http: {} {}", if matches!(req.body, super::Body::None) { "GET" } else { "POST" }, req.url.as_str());
                    let outcome = http::fetch(stack, &req, flash, &mut small).await;
                    match &outcome {
                        Ok(r) => info!("http: {} ({} bytes)", r.status, r.len),
                        Err(e) => warn!("http: {}", e.label()),
                    }
                    let i = lane as usize;
                    with(|s| match outcome {
                        Ok(r) => {
                            if req.sink == Sink::Small {
                                let n = (r.len as usize).min(SMALL_BODY_MAX);
                                s.small_body[i][..n].copy_from_slice(&small[..n]);
                                s.small_len[i] = n;
                            }
                            s.jobs[i] = JobState::Done(r);
                        }
                        Err(e) => s.jobs[i] = JobState::Failed(e),
                    });
                }
                // Wake for new work, or once a second to check the link.
                None => {
                    let _ = with_timeout(Duration::from_secs(1), WAKE.wait()).await;
                }
            }
        }
    }
}

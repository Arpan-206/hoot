//! The Wi-Fi service task for the Pico W.
//!
//! It powers the radio only when something asks for the network, joins the
//! configured access point, gets an address by DHCP, then serves fetch
//! requests from `SHARED` until the link drops, and then rejoins.

use cyw43::{JoinOptions, PowerManagementMode};
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

use super::{JobState, NetState, SMALL_BODY_MAX, Sink, WAKE, http, with};
use crate::hw::Irqs;
use crate::storage::FlashMutex;

type Cyw43Runner = cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>;

const DHCP_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[embassy_executor::task]
async fn cyw43_runner(runner: Cyw43Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
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
    log::info!("wifi: powering radio, loading firmware");

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

    let fw = cyw43::aligned_bytes!("../../firmware/cyw43/43439A0.bin");
    let clm = cyw43::aligned_bytes!("../../firmware/cyw43/43439A0_clm.bin");
    let nvram = cyw43::aligned_bytes!("../../firmware/cyw43/nvram_rp2040.bin");

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let (device, mut control, runner) =
        cyw43::new(STATE.init(cyw43::State::new()), pwr, spi, fw, nvram).await;
    spawner.spawn(cyw43_runner(runner).unwrap());
    log::info!("wifi: firmware loaded, initialising");
    control.init(clm).await;
    control.set_power_management(PowerManagementMode::PowerSave).await;
    log::info!("wifi: radio ready");

    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let seed = RoscRng.next_u64();
    let (stack, runner) = embassy_net::new(
        device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_runner(runner).unwrap());

    let mut small = [0u8; SMALL_BODY_MAX];
    loop {
        let (ssid, password) = with(|s| (s.ssid, s.password));
        with(|s| s.state = NetState::Joining);
        log::info!("wifi: joining '{}'", ssid.as_str());
        let options = if password.is_empty() {
            JoinOptions::new_open()
        } else {
            JoinOptions::new(password.as_str().as_bytes())
        };
        if let Err(e) = control.join(ssid.as_str(), options).await {
            log::warn!("wifi: join failed: {:?}", e);
            with(|s| s.state = NetState::JoinFailed);
            Timer::after(RETRY_DELAY).await;
            continue;
        }

        with(|s| s.state = NetState::Dhcp);
        log::info!("wifi: joined, waiting for DHCP");
        if with_timeout(DHCP_TIMEOUT, stack.wait_config_up()).await.is_err() {
            log::warn!("wifi: no DHCP lease within {} s", DHCP_TIMEOUT.as_secs());
            with(|s| s.state = NetState::JoinFailed);
            control.leave().await;
            Timer::after(RETRY_DELAY).await;
            continue;
        }
        let ip = stack
            .config_v4()
            .map(|c| c.address.address().octets())
            .unwrap_or([0; 4]);
        with(|s| s.state = NetState::Up(ip));
        log::info!("wifi: up, ip {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);

        // Serve requests until the link drops.
        loop {
            if !stack.is_link_up() || !stack.is_config_up() {
                log::warn!("wifi: link lost, rejoining");
                with(|s| s.state = NetState::Lost);
                break;
            }
            let request = with(|s| s.request.take());
            match request {
                Some(req) => {
                    log::info!("http: GET {}", req.url.as_str());
                    let outcome = http::fetch(stack, &req, flash, &mut small).await;
                    match &outcome {
                        Ok(r) => log::info!("http: {} ({} bytes)", r.status, r.len),
                        Err(e) => log::warn!("http: {}", e.label()),
                    }
                    with(|s| match outcome {
                        Ok(r) => {
                            if req.sink == Sink::Small {
                                let n = (r.len as usize).min(SMALL_BODY_MAX);
                                s.small_body[..n].copy_from_slice(&small[..n]);
                                s.small_len = n;
                            }
                            s.job = JobState::Done(r);
                        }
                        Err(e) => s.job = JobState::Failed(e),
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

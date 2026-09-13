//! Sprig OS: a small operating system for the Hack Club Sprig.
//!
//! Built on the Embassy async runtime. Boot order: clocks, module detection,
//! flash storage, PWM, buttons, display, power monitor, radio (Pico W with
//! the `wifi` feature), splash, then the shell loop at about 60 frames per
//! second. The shell is one task; the radio service is another.

#![no_std]
#![no_main]

mod apps;
mod board;
mod drivers;
mod hw;
mod net;
mod panic;
mod storage;
mod ui;

use core::cell::RefCell;

use embassy_executor::Spawner;
use embassy_rp::adc::{self, Adc};
use embassy_rp::flash::Flash;
use embassy_rp::gpio::{Input as GpioInput, Level, Output, Pull};
use embassy_rp::peripherals::USB;
use embassy_rp::pwm::{self, Pwm};
use embassy_rp::spi::{self, Spi};
use embassy_rp::usb;
use embassy_rp::watchdog::Watchdog;
use log::info;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Delay, Duration, Instant, Ticker, Timer, with_timeout};
use sprig_gfx::Framebuffer;
use static_cell::{ConstStaticCell, StaticCell};

use crate::apps::Ctx;
use crate::drivers::dimmer::Dimmer;
use crate::drivers::input::Input;
use crate::drivers::module::{self, Module};
use crate::drivers::power::Power;
use crate::drivers::st7735::St7735;
use crate::hw::{Display, Hardware, Irqs};
use crate::net::NetHandle;
use crate::storage::{Config, FlashMutex, Storage};
use crate::ui::shell::Shell;

/// Version string shown in the UI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The single framebuffer (40 KiB). It lives in RAM (.bss), not on the stack.
static FRAMEBUFFER: ConstStaticCell<Framebuffer> = ConstStaticCell::new(Framebuffer::new());
/// The flash driver, shared by the shell task and the radio task.
static FLASH: StaticCell<FlashMutex> = StaticCell::new();
/// The display driver. Static so the panic handler can reach it.
static DISPLAY: StaticCell<Display> = StaticCell::new();

/// Target frame period. 16 ms is about 60 frames per second.
const FRAME_MS: u64 = 16;
/// Reboot if the shell loop stalls for this long.
const WATCHDOG: Duration = Duration::from_millis(3_000);
/// A DMA frame takes about 11 ms. Longer means the transfer is stuck.
const FRAME_DMA_TIMEOUT: Duration = Duration::from_millis(100);

/// Send the frame by DMA. If a transfer ever fails to complete, switch to
/// the blocking path for good and remember it, so About can say so.
async fn push_frame(display: &mut Display, fb: &Framebuffer, dma_ok: &mut bool) {
    if *dma_ok {
        if with_timeout(FRAME_DMA_TIMEOUT, display.write_frame_async(fb.as_bytes()))
            .await
            .is_ok()
        {
            return;
        }
        *dma_ok = false;
        log::warn!("DMA frame transfer timed out; using blocking transfers from now on");
    }
    display.write_frame(fb.as_bytes());
}

/// Config used until the device has saved one of its own. The values come
/// from `os/secrets.toml` at build time, see `build.rs`.
fn default_config() -> Config {
    let mut c = Config::default();
    c.wifi_ssid.set(option_env!("SPRIG_WIFI_SSID").unwrap_or(""));
    c.wifi_password.set(option_env!("SPRIG_WIFI_PASSWORD").unwrap_or(""));
    c.frame_server.set(option_env!("SPRIG_FRAME_SERVER").unwrap_or("http://192.168.1.9:8000"));
    c.frame_name.set(option_env!("SPRIG_FRAME_NAME").unwrap_or("arpan"));
    c.poll_secs = 15;
    c
}

/// Debug log over USB serial. Connect with `screen /dev/tty.usbmodem*`.
/// Lines logged before the host opens the port are kept in a 2 KiB buffer.
#[embassy_executor::task]
async fn usb_logger(driver: usb::Driver<'static, USB>) {
    embassy_usb_logger::run!(2048, log::LevelFilter::Debug, driver);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut p = embassy_rp::init(Default::default());
    spawner.spawn(usb_logger(usb::Driver::new(p.USB, Irqs)).unwrap());
    info!("Sprig OS {} booting", VERSION);

    // Display first, so that a panic anywhere later in boot can be read on
    // screen. ST7735 on SPI0, write-only, SPI mode 0, frames sent by DMA.
    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = board::TFT_SPI_HZ;
    let spi = Spi::new_txonly(p.SPI0, p.PIN_18, p.PIN_19, p.DMA_CH0, Irqs, spi_cfg);
    let cs = Output::new(p.PIN_20, Level::High);
    let dc = Output::new(p.PIN_22, Level::Low);
    let rst = Output::new(p.PIN_26, Level::Low);
    let display = DISPLAY.init(St7735::new(spi, cs, dc, rst));
    display.init(&mut Delay);
    let fb = FRAMEBUFFER.take();
    panic::arm(display, fb);
    info!("display up");

    // Which Pico module is this? Needs the ADC, so bring that up first.
    let mut adc = Adc::new_blocking(p.ADC, adc::Config::default());
    let module = module::detect(&mut adc, p.PIN_29.reborrow(), p.PIN_25.reborrow());
    let has_radio = module == Module::PicoW && cfg!(feature = "wifi");
    info!("module: {}, radio usable: {}", module.name(), has_radio);

    // Flash storage and the saved config.
    let flash: &'static FlashMutex =
        FLASH.init(Mutex::new(RefCell::new(Flash::new_blocking(p.FLASH))));
    let mut store = Storage::new(flash, default_config());
    info!(
        "config: ssid '{}', server '{}', frame '{}', slot {}",
        store.config().wifi_ssid.as_str(),
        store.config().frame_server.as_str(),
        store.config().frame_name.as_str(),
        store.config().frame_slot
    );

    // PWM outputs: backlight on slice 0 B, LEDs on slice 6 A (left) and 2 A (right).
    // Default config: 16-bit counter, about 1.9 kHz at 125 MHz, enabled.
    // Each dimmer keeps the whole slice handle. Do not `split()` these:
    // dropping the `Pwm` handle afterwards disables the slice.
    let pwm_cfg = pwm::Config::default();
    let backlight_pwm = Pwm::new_output_b(p.PWM_SLICE0, p.PIN_17, pwm_cfg.clone());
    let led_left_pwm = Pwm::new_output_a(p.PWM_SLICE6, p.PIN_28, pwm_cfg.clone());
    let led_right_pwm = Pwm::new_output_a(p.PWM_SLICE2, p.PIN_4, pwm_cfg);
    // The backlight starts dark. It fades in after the first frame is drawn.
    let mut backlight = Dimmer::new(backlight_pwm, 0);
    let led_left = Dimmer::new(led_left_pwm, 0);
    let led_right = Dimmer::new(led_right_pwm, 0);

    // Buttons, in `Button` order: W A S D I J K L.
    let mut input = Input::new([
        GpioInput::new(p.PIN_5, Pull::Up),
        GpioInput::new(p.PIN_6, Pull::Up),
        GpioInput::new(p.PIN_7, Pull::Up),
        GpioInput::new(p.PIN_8, Pull::Up),
        GpioInput::new(p.PIN_12, Pull::Up),
        GpioInput::new(p.PIN_13, Pull::Up),
        GpioInput::new(p.PIN_14, Pull::Up),
        GpioInput::new(p.PIN_15, Pull::Up),
    ]);

    // Power monitor and radio. On a plain Pico GP24 and GP29 measure power.
    // On a Pico W they belong to the radio, which the Wi-Fi task owns.
    let (power, mut net) = match module {
        Module::Pico => {
            let vsys = adc::Channel::new_pin(p.PIN_29, Pull::None);
            let vbus = GpioInput::new(p.PIN_24, Pull::None);
            // Keep the Pico's own LED off.
            let _led = Output::new(p.PIN_25, Level::Low);
            (Power::new(adc, Some(vsys), Some(vbus)), NetHandle::new(false, "", ""))
        }
        Module::PicoW => {
            #[cfg(feature = "wifi")]
            spawner.spawn(
                net::wifi::wifi_service(
                    spawner,
                    net::wifi::RadioPins {
                        pwr: p.PIN_23,
                        cs: p.PIN_25,
                        dio: p.PIN_24,
                        clk: p.PIN_29,
                        pio: p.PIO0,
                        dma: p.DMA_CH1,
                    },
                    flash,
                )
                .unwrap(),
            );
            let cfg = store.config();
            let net = NetHandle::new(has_radio, cfg.wifi_ssid.as_str(), cfg.wifi_password.as_str());
            (Power::new(adc, None, None), net)
        }
    };
    #[cfg(not(feature = "wifi"))]
    let _ = &spawner;

    // Splash screen, then fade the backlight in over about 300 ms.
    let mut dma_ok = true;
    ui::splash::draw(fb);
    push_frame(display, fb, &mut dma_ok).await;
    fb.take_dirty();
    for level in (0..=255u8).step_by(5) {
        backlight.set(level);
        Timer::after_millis(6).await;
    }
    backlight.set(255);
    Timer::after_millis(400).await;

    let mut hw = Hardware {
        module,
        backlight,
        led_left,
        led_right,
        power,
        display_dma: dma_ok,
    };
    let mut shell = Shell::new(has_radio);

    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(WATCHDOG);
    info!("shell running, watchdog armed");

    let mut ticker = Ticker::every(Duration::from_millis(FRAME_MS));
    let mut frame_ms = 0u32;
    loop {
        let start = Instant::now();
        let now_ms = start.as_millis() as u32;
        input.poll(now_ms);
        {
            let mut ctx = Ctx {
                fb: &mut *fb,
                input: &input,
                hw: &mut hw,
                net: &mut net,
                store: &mut store,
                now_ms,
                frame_ms,
            };
            shell.update(&mut ctx);
        }
        // Only send a frame when something was drawn. An idle screen costs
        // nothing, and the photo frame never redraws a photo it already shows.
        if fb.take_dirty() {
            push_frame(display, fb, &mut dma_ok).await;
            hw.display_dma = dma_ok;
        }
        watchdog.feed(WATCHDOG);
        frame_ms = start.elapsed().as_millis() as u32;

        // Sleep until the next frame. Other tasks run in the meantime.
        ticker.next().await;
    }
}

//! Hoot: a small operating system for the Hack Club Sprig.
//!
//! Built on the Embassy async runtime. Boot order: clocks, module detection,
//! flash storage, PWM, buttons, display, power monitor, radio (Pico W with
//! the `wifi` feature), splash, then the shell loop at about 60 frames per
//! second. The shell is one task; the radio service is another.

#![no_std]
#![no_main]

#[macro_use]
mod logging;

#[cfg(feature = "wifi")]
mod agent;
mod apps;
mod audio;
mod clock;
mod board;
mod drivers;
mod hw;
mod net;
mod ota;
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
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Delay, Duration, Instant, Ticker, Timer, with_timeout};
use hoot_gfx::Framebuffer;
use static_cell::{ConstStaticCell, StaticCell};

use crate::apps::Ctx;
use crate::drivers::dimmer::Dimmer;
use crate::drivers::input::Input;
use crate::drivers::power::PowerStatus;
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
/// After running this long, tell the boot loader the firmware is good.
const CONFIRM_BOOT_MS: u32 = 20_000;
/// A trial boot (fresh update) that cannot reach the server in this long
/// is reset, and the boot loader brings the previous firmware back. An
/// update can then never strand a device we only reach over the network.
const TRIAL_LIMIT_MS: u32 = 10 * 60_000;
/// How often the power reading is refreshed.
const POWER_POLL_MS: u32 = 500;
/// Battery saver: dim the backlight after this long without a key press.
const DIM_AFTER_MS: u32 = 30_000;
/// Battery saver: dimmed brightness as a share of the user's setting.
const DIM_PERCENT: u32 = 30;
const DIM_MIN: u8 = 8;

/// Battery saver decision from the stored mode and the power reading.
fn saver_active(mode: u8, power: &PowerStatus) -> bool {
    match mode {
        hoot_proto::record::POWER_SAVER => true,
        hoot_proto::record::POWER_NORMAL => false,
        _ => power.usb_known && !power.usb,
    }
}

/// Dims the backlight when idle in battery saver mode, and restores it on
/// the first key press, which is swallowed so no app acts on it.
struct IdleDimmer {
    last_input_ms: u32,
    dimmed: bool,
    user_level: u8,
}

impl IdleDimmer {
    fn update(&mut self, input: &mut Input, hw: &mut Hardware, saver: bool, now: u32) {
        if input.held_mask() != 0 {
            self.last_input_ms = now;
            if self.dimmed {
                self.dimmed = false;
                hw.backlight.set(self.user_level);
                input.swallow();
            }
            return;
        }
        if self.dimmed && !saver {
            self.dimmed = false;
            hw.backlight.set(self.user_level);
        } else if saver && !self.dimmed && now.wrapping_sub(self.last_input_ms) >= DIM_AFTER_MS {
            self.user_level = hw.backlight.level();
            let dim = (self.user_level as u32 * DIM_PERCENT / 100) as u8;
            hw.backlight.set(dim.max(DIM_MIN));
            self.dimmed = true;
        }
    }
}
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
        warn!("DMA frame transfer timed out; using blocking transfers from now on");
    }
    display.write_frame(fb.as_bytes());
}

/// Config used until the device has saved one of its own. The values come
/// from `os/secrets.toml` at build time, see `build.rs`.
fn default_config() -> Config {
    let mut c = Config::default();
    c.wifi_ssid.set(option_env!("HOOT_WIFI_SSID").unwrap_or(""));
    c.wifi_password.set(option_env!("HOOT_WIFI_PASSWORD").unwrap_or(""));
    c.frame_server.set(option_env!("HOOT_FRAME_SERVER").unwrap_or("http://192.168.1.9:8000"));
    c.frame_name.set(option_env!("HOOT_FRAME_NAME").unwrap_or("arpan"));
    c.poll_secs = 15;
    c.sound = hoot_proto::record::SOUND_DEFAULT;
    c.alarm_min = hoot_proto::record::ALARM_DEFAULT_MIN;
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
    info!("Hoot {} booting", VERSION);

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
    fb.mark_dirty();
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

    // Speaker: I2S from PIO1 and DMA channel 2, owned by the audio task.
    audio::set_volume(store.config().sound);
    info!("sound level: {}", store.config().sound);
    spawner.spawn(
        audio::audio_task(audio::Pins {
            pio: p.PIO1,
            dma: p.DMA_CH2,
            din: p.PIN_9,
            bclk: p.PIN_10,
            lrclk: p.PIN_11,
        })
        .unwrap(),
    );

    // Power monitor and radio. On a plain Pico GP24 and GP29 measure power.
    // On a Pico W they belong to the radio, which the Wi-Fi task owns.
    let (power, mut net) = match module {
        Module::Pico => {
            let vsys = adc::Channel::new_pin(p.PIN_29, Pull::None);
            let vbus = GpioInput::new(p.PIN_24, Pull::None);
            // Keep the Pico's own LED off.
            let _led = Output::new(p.PIN_25, Level::Low);
            (Power::new(adc, Some(vsys), Some(vbus)), NetHandle::new(false, store.config()))
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
            let net = NetHandle::new(has_radio, store.config());
            (Power::new(adc, None, None), net)
        }
    };
    // Splash screen: fade the backlight in over about 300 ms, hold, let the
    // owl blink once, hold again. Close to three seconds in all.
    let mut dma_ok = true;
    ui::splash::draw(fb);
    push_frame(display, fb, &mut dma_ok).await;
    fb.take_dirty();
    for level in (0..=255u8).step_by(5) {
        backlight.set(level);
        Timer::after_millis(6).await;
    }
    backlight.set(255);
    Timer::after_millis(800).await;
    ui::splash::blink(fb, true);
    push_frame(display, fb, &mut dma_ok).await;
    Timer::after_millis(140).await;
    ui::splash::blink(fb, false);
    push_frame(display, fb, &mut dma_ok).await;
    fb.take_dirty();
    Timer::after_millis(1_600).await;

    let mut hw = Hardware {
        module,
        backlight,
        led_left,
        led_right,
        power,
        display_dma: dma_ok,
    };
    let mut shell = Shell::new(has_radio);
    // The OS agent keeps the device serviceable from the server whatever
    // app is on screen: heartbeat, warnings, commands, updates.
    #[cfg(feature = "wifi")]
    let mut agent = agent::Agent::new(module.name());

    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(WATCHDOG);
    info!("shell running, watchdog armed");

    let mut ticker = Ticker::every(Duration::from_millis(FRAME_MS));
    let mut frame_ms = 0u32;
    let mut boot_confirmed = false;
    let trial = ota::in_trial(flash);
    if trial {
        info!("ota: trial boot; confirming after the first heartbeat");
    }
    let mut power = hw.power.read();
    let mut next_power_ms = 0u32;
    let mut idle = IdleDimmer { last_input_ms: 0, dimmed: false, user_level: 255 };
    loop {
        let start = Instant::now();
        let now_ms = start.as_millis() as u32;
        if !boot_confirmed {
            // With a radio, the new firmware must reach the server first.
            #[cfg(feature = "wifi")]
            let ready = if has_radio { agent.heartbeat_ok() } else { now_ms >= CONFIRM_BOOT_MS };
            #[cfg(not(feature = "wifi"))]
            let ready = now_ms >= CONFIRM_BOOT_MS;
            if ready {
                boot_confirmed = true;
                ota::confirm_boot(flash);
            } else if trial && has_radio && now_ms >= TRIAL_LIMIT_MS {
                warn!("ota: no heartbeat in the trial window; reverting");
                Timer::after_millis(300).await;
                cortex_m::peripheral::SCB::sys_reset();
            }
        }
        if now_ms.wrapping_sub(next_power_ms) < u32::MAX / 2 {
            power = hw.power.read();
            // A Pico W cannot see VBUS itself; the radio chip can.
            if !power.usb_known && let Some(usb) = net.usb_power() {
                power.usb = usb;
                power.usb_known = true;
            }
            next_power_ms = now_ms.wrapping_add(POWER_POLL_MS);
        }
        let saver = saver_active(store.config().power_mode, &power);
        input.poll(now_ms);
        idle.update(&mut input, &mut hw, saver, now_ms);
        #[cfg(feature = "wifi")]
        agent.update(&mut net, &mut store, now_ms, shell.current_app(), saver);
        {
            let mut ctx = Ctx {
                fb: &mut *fb,
                input: &input,
                hw: &mut hw,
                net: &mut net,
                store: &mut store,
                power,
                saver,
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

//! Kernel panic and hard fault handlers.
//!
//! When the OS panics or the CPU faults, the reason goes on the display,
//! both white LEDs blink forever, and pressing L reboots into USB flash
//! mode. The handlers use raw register writes and the blocking display
//! path, so they work no matter which task was running. Before the display
//! is armed only the LEDs blink.

use core::panic::PanicInfo;
use core::ptr::{null_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicPtr, Ordering};

use cortex_m_rt::{ExceptionFrame, exception};
use sprig_gfx::{CELL_HEIGHT, CELL_WIDTH, Framebuffer, HEIGHT, Rgb565, WIDTH};

use crate::board::pins::{BTN_L, LED_LEFT, LED_RIGHT, TFT_BACKLIGHT};
use crate::hw::Display;
use crate::ui::text::{StrBuf, format};

const IO_BANK0_BASE: usize = 0x4001_4000;
const PADS_BANK0_BASE: usize = 0x4001_c000;
const SIO_BASE: usize = 0xd000_0000;
const GPIO_IN: usize = SIO_BASE + 0x04;
const GPIO_OUT_SET: usize = SIO_BASE + 0x14;
const GPIO_OUT_XOR: usize = SIO_BASE + 0x1c;
const GPIO_OE_SET: usize = SIO_BASE + 0x24;
const WATCHDOG_CTRL_CLR: usize = 0x4005_8000 + 0x3000;
const WATCHDOG_ENABLE: u32 = 1 << 30;
/// GPIO function select value for software-controlled I/O.
const FUNCSEL_SIO: u32 = 5;
/// Pad config: input enabled, pull-up, Schmitt trigger, 4 mA drive.
const PAD_INPUT_PULLUP: u32 = 0x5A;
/// About 125 ms at the default 125 MHz system clock.
const HALF_PERIOD_CYCLES: u32 = 15_600_000;
/// Characters that fit on one line of the fault screen.
const COLS: usize = (WIDTH / CELL_WIDTH) as usize;

const BG: Rgb565 = Rgb565::hex(0x40_10_14);
const DIM: Rgb565 = Rgb565::hex(0xE8_A0_A6);

static DISPLAY: AtomicPtr<Display> = AtomicPtr::new(null_mut());
static FRAME: AtomicPtr<Framebuffer> = AtomicPtr::new(null_mut());

/// Let the fault handlers draw on the display. Call once the display works.
///
/// The handlers keep raw pointers to objects that `main` goes on using.
/// That is sound only because they run with interrupts disabled and never
/// return, so `main` can never touch the objects again.
pub fn arm(display: &mut Display, fb: &mut Framebuffer) {
    DISPLAY.store(display, Ordering::Release);
    FRAME.store(fb, Ordering::Release);
}

unsafe fn gpio_funcsel_sio(pin: u8) {
    let ctrl = (IO_BANK0_BASE + 8 * pin as usize + 4) as *mut u32;
    unsafe { write_volatile(ctrl, FUNCSEL_SIO) };
}

unsafe fn gpio_output_high(pin: u8) {
    unsafe {
        gpio_funcsel_sio(pin);
        write_volatile(GPIO_OE_SET as *mut u32, 1 << pin);
        write_volatile(GPIO_OUT_SET as *mut u32, 1 << pin);
    }
}

fn button_l_pressed() -> bool {
    // SAFETY: fixed RP2040 register address, read only.
    unsafe { read_volatile(GPIO_IN as *const u32) & (1 << BTN_L) == 0 }
}

fn draw_wrapped(fb: &mut Framebuffer, mut y: i32, text: &str, color: Rgb565, max_lines: usize) -> i32 {
    for chunk in text.as_bytes().chunks(COLS).take(max_lines) {
        let line = core::str::from_utf8(chunk).unwrap_or("?");
        fb.draw_text(2, y, line, color, None);
        y += CELL_HEIGHT;
    }
    y
}

/// Title bar and footer shared by both screens. Returns the first free row.
fn draw_frame(fb: &mut Framebuffer, title: &str) -> i32 {
    fb.clear(BG);
    fb.draw_text_centered(3, title, Rgb565::WHITE, None, 1);
    fb.hline(0, 13, WIDTH, DIM);
    fb.draw_text(2, HEIGHT - CELL_HEIGHT - 2, "L: USB flash mode", DIM, None);
    18
}

fn draw_panic(fb: &mut Framebuffer, info: &PanicInfo) {
    let mut y = draw_frame(fb, "KERNEL PANIC");
    if let Some(loc) = info.location() {
        let where_: StrBuf<128> = format(format_args!("{}:{}", loc.file(), loc.line()));
        y = draw_wrapped(fb, y, where_.as_str(), DIM, 5);
    }
    y += 4;
    let what: StrBuf<200> = format(format_args!("{}", info.message()));
    draw_wrapped(fb, y, what.as_str(), Rgb565::WHITE, 7);
}

fn draw_fault(fb: &mut Framebuffer, ef: &ExceptionFrame) {
    let mut y = draw_frame(fb, "HARD FAULT");
    let regs: [(&str, u32); 6] = [
        ("pc  ", ef.pc()),
        ("lr  ", ef.lr()),
        ("xpsr", ef.xpsr()),
        ("r0  ", ef.r0()),
        ("r1  ", ef.r1()),
        ("r2  ", ef.r2()),
    ];
    for (name, value) in regs {
        let line: StrBuf<24> = format(format_args!("{name} {value:08x}"));
        fb.draw_text(2, y, line.as_str(), Rgb565::WHITE, None);
        y += CELL_HEIGHT;
    }
    y += 4;
    draw_wrapped(fb, y, "CPU fault: bad address or unaligned access", DIM, 2);
}

/// Show a screen if the display is armed, then blink until L is pressed.
fn halt(draw: impl FnOnce(&mut Framebuffer)) -> ! {
    cortex_m::interrupt::disable();
    // SAFETY: fixed RP2040 register addresses; we never return, so nothing
    // else uses these peripherals again.
    unsafe {
        // Keep the watchdog from rebooting us: the message must stay up.
        write_volatile(WATCHDOG_CTRL_CLR as *mut u32, WATCHDOG_ENABLE);
        // Read L through a pull-up, whatever state main left the pad in.
        write_volatile((PADS_BANK0_BASE + 4 + 4 * BTN_L as usize) as *mut u32, PAD_INPUT_PULLUP);
        gpio_funcsel_sio(BTN_L);
    }

    let display = DISPLAY.load(Ordering::Acquire);
    let frame = FRAME.load(Ordering::Acquire);
    if !display.is_null() && !frame.is_null() {
        // SAFETY: see `arm`.
        let (display, fb) = unsafe { (&mut *display, &mut *frame) };
        draw(fb);
        display.write_frame(fb.as_bytes());
        // Full backlight, in case the fault came before the fade-in.
        unsafe { gpio_output_high(TFT_BACKLIGHT) };
    }

    let mask = (1u32 << LED_LEFT) | (1u32 << LED_RIGHT);
    unsafe {
        gpio_funcsel_sio(LED_LEFT);
        gpio_funcsel_sio(LED_RIGHT);
        write_volatile(GPIO_OE_SET as *mut u32, mask);
        loop {
            write_volatile(GPIO_OUT_XOR as *mut u32, mask);
            cortex_m::asm::delay(HALF_PERIOD_CYCLES);
            if button_l_pressed() {
                embassy_rp::rom_data::reset_to_usb_boot(0, 0);
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    halt(|fb| draw_panic(fb, info))
}

#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    halt(|fb| draw_fault(fb, ef))
}

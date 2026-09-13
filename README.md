# Sprig OS

A small operating system for the Hack Club Sprig, written in Rust on the
Embassy async runtime. It replaces the stock Spade firmware. It shares no
code with it.

The Sprig is a handheld game console. It has a Raspberry Pi Pico (RP2040),
a 160x128 colour display, eight buttons, two white LEDs and a speaker.

## Status

| Part | State |
| --- | --- |
| Boot, clocks, watchdog, Embassy executor | Done |
| Display driver (ST7735, full-frame SPI) | Done, verified on hardware |
| Framebuffer, font, drawing primitives | Done, unit-tested on the host |
| Buttons with debounce and key repeat | Done, verified on hardware |
| Backlight and LED dimming (PWM) | Done |
| USB and battery voltage readout | Done |
| Home menu with four built-in apps | Done |
| Reboot to USB flash mode from the menu | Done |
| Pico vs Pico W detection at boot | Done, verified on hardware |
| Flash storage: config record and 64 KiB blob slots | Done, untested on hardware |
| Wi-Fi, DHCP, DNS and HTTP client (Pico W, `wifi` feature) | Done, untested on hardware |
| Photo frame app | Done, untested on hardware |
| Wi-Fi setup on the device | Not started. Credentials come from `secrets.toml` |
| Kernel panic indicator (both LEDs blink) | Done |
| Audio (I2S) | Not started |
| USB serial console | Not started |
| Multi-tasking, app loading, storage | Not started |

## Hardware map

All numbers are RP2040 GPIO numbers. They come from the stock firmware.

| Function | GPIO | Notes |
| --- | --- | --- |
| Display SCK / MOSI | 18 / 19 | SPI0 |
| Display CS / DC / RST | 20 / 22 / 26 | |
| Display backlight | 17 | PWM slice 0 B |
| Buttons W A S D | 5 6 7 8 | Active low, pull-up |
| Buttons I J K L | 12 13 14 15 | Active low, pull-up |
| LED left / right | 28 / 4 | PWM slice 6 A / 2 A |
| I2S DIN / BCLK / LRCLK | 9 / 10 / 11 | Not used yet |
| USB power detect | 24 | High on USB. Plain Pico only |
| VSYS sense | 29 | ADC 3, reads VSYS / 3 |

Some Pico pins mean something else on a Pico W. Sprig OS checks which
module it runs on at boot and adapts.

| GPIO | Plain Pico | Pico W |
| --- | --- | --- |
| 23 | Regulator power-save mode | WL_ON, wireless chip power |
| 24 | USB power detect | WL_D, wireless SPI data |
| 25 | On-board LED | WL_CS, wireless SPI chip select |
| 29 | VSYS / 3 | VSYS / 3, shared with wireless SPI clock |

## Build

1. Install Rust with `rustup`. The `rust-toolchain.toml` file adds the
   Cortex-M0+ target for you.
2. Install the flasher: `brew install picotool`.
3. Optional: copy `os/secrets.example.toml` to `os/secrets.toml` and fill
   in your Wi-Fi and photo server. The file is git-ignored.
4. Build one of the two variants.

| Command | For | Flash image |
| --- | --- | --- |
| `cargo build --release` | Plain Pico | 106.8 KiB |
| `cargo build-w` | Pico W, adds the Wi-Fi stack and the network apps | 459.5 KiB |

The Wi-Fi stack is a Cargo feature, `wifi`. Leaving it out saves about
350 KiB of flash and the RAM the network stack would pin down. Apps that
declare `needs_network` (Photo frame, Network) are compiled only into
`wifi` builds. A `wifi` build on a plain Pico still works: it reports
"No wifi" and hides those apps from the menu.

Host tests for the graphics crate:

```sh
cargo host-test
cargo host-test -- font_sheet --nocapture   # prints the font as ASCII art
```

The `host-test` alias targets Apple Silicon. Edit `.cargo/config.toml` for
another host.

## Flash

1. Hold the BOOTSEL button on the Pico. Connect USB. Release the button.
2. Run `cargo run --release`, or `cargo run-w` for a Pico W. This calls
   `tools/flash.sh`, which makes picotool write the program over USB,
   verify it and start it.
   The manual equivalent is:

   ```sh
   picotool load -v -x target/thumbv6m-none-eabi/release/sprig-os -t elf
   ```
3. The Sprig boots into Sprig OS.

After the first flash you no longer need the button. Choose
"Reboot to USB" in the menu instead.

Do not copy a UF2 file to the `RPI-RP2` drive on macOS 15 or later. The
system's FSKit FAT driver can hang on that drive. If a copy has already
hung, unplug the Sprig. The stuck command then exits on its own.

## Controls

| Where | Button | Action |
| --- | --- | --- |
| Menu | W or I / S or K | Move up / down |
| Menu | L or D | Open the selected item |
| Any app | J | Back to the menu |
| Input test | Hold J for 1 s | Back to the menu |
| LEDs app | W/S, I/K, A/D | Left LED, right LED, backlight |
| Display test | Any key | Next pattern |
| Photo frame | Hold L for 2 s | Forget the cached photo time and fetch again |
| Network (Pico W only) | L or D | Connect to Wi-Fi |
| Menu: Clear photo cache | L | Erase both photo slots and the stored timestamp |
| Menu: Reboot | L | Normal reset |
| Menu: Reboot to USB | L | Reset into the USB flash mode |

## First boot checklist

1. The splash shows "Sprig OS" the right way up. If it is upside down,
   change `TFT_MADCTL` in `os/src/board.rs`.
2. In "Display test" the bar marked R is red and B is blue. If they are
   swapped, add `MADCTL_BGR` to `TFT_MADCTL`.
3. In "Display test" the one-pixel border is visible on all four sides.
4. In "Input test" every key lights up when pressed.
5. "About" names the right module: Pico or Pico W. A Pico W has a metal
   can and a small antenna area at the end of the module.

## Photo frame

The photo frame app is the Sprig version of the ESP32 frame in
`~/Code/Hardware/photo-frame`. It talks to the same `frame-server`.

1. Start the server. It now has a route `GET /frame/<name>.rgb565`. It
   renders the photo and message at 160x128 and returns 40,960 bytes of
   raw RGB565. It honours `If-Modified-Since` and answers 304 when nothing
   changed.
2. Put the server URL, the frame name and your Wi-Fi in `os/secrets.toml`.
3. Build with `cargo build-w`, flash a Pico W Sprig, open "Photo frame".

What it does:

- Shows the last photo from flash within a second of power-on.
- Connects, then polls every 15 s. A new photo streams straight into a
  flash slot, 4 KiB at a time, and is shown from there. Two slots
  alternate, so the old photo stays valid until the new one is complete.
- Once a photo is on screen, nothing draws over it. Trouble shows on the
  left LED: two pulses when the network is down, three when the server
  cannot be reached.
- On a plain Pico it still shows a cached photo, and says it needs a
  Pico W for the rest.

## Debug output

The firmware logs over USB as a serial port, from the first line of boot.
Plug the Sprig in and open the port:

```sh
screen /dev/tty.usbmodem* 115200      # leave with Ctrl-A then K
```

or just `cat /dev/tty.usbmodem*`. Boot, app changes, the radio bring-up
and every HTTP request are logged. Add your own lines with `log::info!`.
Messages logged before the port is opened wait in a 2 KiB buffer.

A panic or a CPU fault shows a red screen with the reason, and pressing L
there reboots into USB flash mode.

## Layout

| Path | Contents |
| --- | --- |
| `gfx/` | `sprig-gfx`: framebuffer, RGB565 colour, 5x7 font. No hardware code. |
| `proto/` | `sprig-proto`: URL and HTTP parsing, CRC-32, config record. No hardware code. |
| `os/src/main.rs` | Boot sequence and the shell task (the frame loop) |
| `os/src/board.rs` | Pin map and display constants |
| `os/src/drivers/` | ST7735, buttons, PWM dimmer, power monitor, module detection |
| `os/src/storage/` | Config store and blob slots in flash |
| `os/src/net/` | Network handle for apps, HTTP client, Wi-Fi task |
| `os/src/ui/` | Theme, text formatting, splash, shell |
| `os/src/apps/` | The `App` trait, the app template, and the built-in apps |
| `os/firmware/cyw43/` | Radio firmware blobs (Infineon permissive binary license) |
| `os/secrets.example.toml` | Template for build-time defaults |
| `os/src/panic.rs` | Panic handler |
| `os/memory.x` | Flash and RAM layout for the linker |
| `tools/flash.sh` | Cargo runner: flash over USB with picotool |

## Flash map

| Offset | Size | Use |
| --- | --- | --- |
| 0x000000 | 1 MiB | Firmware |
| 0x100000 | 512 KiB | Reserved for app slots (WASM) |
| 0x180000 | 448 KiB | 7 blob slots of 64 KiB. Photos use slots 0 and 1 |
| 0x1F0000 | 8 KiB | Config record, two sectors written alternately |
| 0x1F2000 | 56 KiB | Free |

## Writing an app

Every app has the same shape. Read the module docs at the top of
`os/src/apps/mod.rs`. In short: an `INFO` constant, a struct for state,
and an `App` implementation with `on_enter`, `update` and `on_exit`.
`update` runs each frame and must not block. Long work goes to a service
and is polled the next frame. Register the app in the registry in
`os/src/ui/shell.rs`. Set `needs_network: true` if the app cannot work
without Wi-Fi: the build then leaves it out of plain-Pico images, and the
menu hides it when there is no radio. The photo frame app is the
reference example.

## Design notes

- One frame buffer of 40 KiB lives in RAM. Each frame is sent to the panel in
  one SPI burst at 31.25 MHz. That takes about 11 ms.
- The firmware runs on Embassy. The shell is one async task that runs at
  about 60 frames per second. It polls the buttons, updates the current
  app, sends the frame and feeds the watchdog, then sleeps until the next
  tick. Networking, USB and apps will be further tasks beside it.
- Drivers talk to `embedded-hal` traits, not to Embassy directly. Only
  `main.rs`, `hw.rs` and the module and power code name Embassy types.
- The PWM dimmers keep the whole slice handle. Dropping an Embassy `Pwm`
  handle disables its slice, even after `split()`.
- A frame goes to the display only when something was drawn, by DMA.
  An idle screen costs no CPU and no SPI time.
- Apps talk to the network through a request and poll API, never through
  sockets. WASM apps will get the same API as host functions later.
- Reads from flash use the memory-mapped window directly. Only erase and
  program go through the driver, behind a mutex shared with the Wi-Fi task.
- Apps implement the `App` trait. The shell owns them and runs one at a time.
- Nothing allocates. Text is formatted into fixed-size stack buffers.
- Module detection: with GPIO25 low, ADC channel 3 reads near zero on a
  Pico W because the wireless chip holds the shared line down. A plain
  Pico always reads VSYS / 3 there. On a Pico W the OS keeps GPIO25 high
  afterwards so battery readings work, and infers USB power from VSYS
  above 3.6 V because GPIO24 is not the USB sense line on that module.

## Restore the stock firmware

Build or download the Spade firmware from `github.com/hackclub/sprig`.
Flash its UF2 the same way as above.

## Next steps

1. Test the photo frame on a Pico W and fix what the hardware reveals.
2. Wi-Fi setup on the device: an on-screen keyboard or a setup hotspot,
   so credentials no longer come from the build.
3. USB serial console for logging (`embassy-usb`).
4. WASM app runtime (`wasmi`) with the network and storage API as host
   functions. Measure flash, RAM and speed first.
5. Firmware and app updates over the air with `embassy-boot`.
6. I2S audio.

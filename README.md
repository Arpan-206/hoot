# Hoot

A small operating system for the Hack Club Sprig, written in Rust on the
Embassy async runtime. It replaces the stock Spade firmware. It shares no
code with it.

Hoot is named for the barn owl on the splash screen. The look is a night
palette: amber and cream on deep indigo, like a lamp left on for someone. It is the mascot now and
the digital pet later: it will live in the background, sleep at night,
hoot the alarm, and get excited when a note arrives. Only the hardware is
still called a Sprig, so pins, the board and the `X-Sprig-*` heartbeat
headers keep that name.

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
| Home menu with grouped apps (Frame, Fun, Tools) | Done |
| Speaker: I2S tones from PIO1 and DMA, volume setting | Done, verified on hardware |
| Clock from the server heartbeat, daily alarm | Done, verified on hardware |
| Live photos: motion frames streamed from the server | Done, untested on hardware |
| Goals renamed from the web page, kept offline | Done, untested on hardware |
| Hoot the owl: hatching, check-in, goals, breathing, adventures, outfits, the week, hugs | Done, untested on hardware |
| Slideshow of the last uploads, cached in flash | Done, untested on hardware |
| Reboot to USB flash mode from the menu | Done |
| Pico vs Pico W detection at boot | Done, verified on hardware |
| Flash storage: config record and 44 KiB blob slots | Done, verified on hardware |
| Wi-Fi, DHCP, DNS and HTTP client (Pico W, `wifi` feature) | Done, untested on hardware |
| Photo frame app | Done, untested on hardware |
| Wi-Fi setup on the device: hotspot, QR code, captive portal | Done, verified with a phone |
| Boot loader with two firmware partitions and rollback | Done, boots on hardware |
| Firmware updates over the air from the photo server | Done |
| Heartbeat, remote warnings and server commands | Done, verified |
| Battery saver: idle dimming, slower polling | Done |
| Messages app with unread badge and LED cue | Done |
| Pomodoro, Fireplace, Aquarium | Done |
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

Some Pico pins mean something else on a Pico W. Hoot checks which
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

The display holds 26 characters across at the small size. Run
`python3 tools/check_text.py` to find any literal that would run off the
edge: footers, hints and centred lines. The release script runs it first
and refuses to publish while anything overflows.

## Flash

The OS runs behind a small boot loader, so a Sprig is set up in three
parts: boot loader, radio firmware (Pico W only) and the OS. The first two
never change; later OS updates arrive over the air or with one command.

First time, or after changing the flash layout:

1. Hold the BOOTSEL button on the Pico. Connect USB. Release the button.
2. Run `tools/flash-all.sh wifi` (or `tools/flash-all.sh plain` for a
   plain Pico). It flashes the boot loader, writes the radio firmware to
   its partition, flashes the OS and reboots into it.

OS only, later:

1. Choose "Reboot to USB" in the menu, or hold BOOTSEL while plugging in.
2. Run `cargo run --release`, or `cargo run-w` for a Pico W. This calls
   `tools/flash.sh`, which makes picotool write the OS over USB, verify
   it and reboot the board.

Do not copy a UF2 file to the `RPI-RP2` drive on macOS 15 or later. The
system's FSKit FAT driver can hang on that drive. If a copy has already
hung, unplug the Sprig. The stuck command then exits on its own.

## Controls

The menu has two levels. Apps are grouped: the top level shows one entry
per group, then About, Settings and Developer. An app never sits on the
top level by itself. Each app names its group, and the shell builds the
submenus from that. A group with nothing usable on the board is hidden.

| Menu | Entries |
| --- | --- |
| Top | Hoot, Frame, Fun, Tools, About, Settings, Developer |
| Frame | Photo frame, Messages, Slideshow |
| Fun | Fireplace, Aquarium, Sounds |
| Tools | Pomodoro, Stopwatch, Clock, Alarm |
| Settings | Network, Battery saver, Volume, Clear photo cache, Reboot, Reboot to USB |
| Developer | Input test, LEDs & backlight, Display test, Speaker test |

Frame and its apps exist only in Wi-Fi builds. The unread count for
Messages also shows next to Frame on the top menu.

| Where | Button | Action |
| --- | --- | --- |
| Any menu | W or I / S or K | Move up / down |
| Any menu | L or D | Open the selected item |
| Submenu | J or A | Back to the top menu |
| Any app | J | Back to the menu |
| Input test | Hold J for 1 s | Back to the menu |
| LEDs app | W/S, I/K, A/D | Left LED, right LED, backlight |
| Display test | Any key | Next pattern |
| Speaker test | L, K | Full-scale three-tone sweep, chime at the set level |
| Photo frame | Hold L for 2 s | Forget the cached photo time and fetch again |
| Messages | W/S, L, K | Move, mark the selected message seen, refresh |
| Pomodoro | L, K | Start or pause, stop. W/S and A/D set the lengths while ready |
| Hoot | W/S, L, K, I, D | Pick a goal, do it or take it back, fly when full, the week, the next outfit |
| Hoot: check in | A/D, L, J | Pick a face, confirm, or skip |
| Stopwatch | L, K | Start or stop. Lap while running, reset while stopped |
| Clock | W/S, A/D, L | Hour, minute, zero the seconds. Sets the clock by hand |
| Alarm | W/S, A/D, L, K | Hour, minute, on or off, next tone. While ringing: L stops, K snoozes 5 min |
| Sounds | W A S D I K L | One sound per key |
| Slideshow | A/D, W/S | Previous or next photo, dwell time down or up in 5 s steps |
| Fireplace | W/S | More or less fuel |
| Network (Pico W only) | L or D | Connect to Wi-Fi |
| Network (Pico W only) | K | Open the setup hotspot |
| Settings: Clear photo cache | L | Erase both photo slots and the stored timestamp |
| Settings: Battery saver | L | Cycle auto, on, off |
| Settings: Volume | A/D or W/S, L | Set the level from 0 to 10, play the chime |
| Settings: Reboot | L | Normal reset |
| Settings: Reboot to USB | L | Reset into the USB flash mode |

## First boot checklist

1. The splash shows the owl on its branch and "Hoot" the right way up. If it is upside down,
   change `TFT_MADCTL` in `os/src/board.rs`.
2. In "Display test" the bar marked R is red and B is blue. If they are
   swapped, add `MADCTL_BGR` to `TFT_MADCTL`.
3. In "Display test" the one-pixel border is visible on all four sides.
4. In "Input test" every key lights up when pressed.
5. "About" names the right module: Pico or Pico W. A Pico W has a metal
   can and a small antenna area at the end of the module.

## Updates over the air

Frames update themselves from the photo server, like the ESP32 frame.

1. Bump `version` under `[workspace.package]` in `Cargo.toml`.
2. Run `tools/release.sh ~/Code/Hardware/frame-server`. It builds the
   Wi-Fi OS, extracts the flat image with `tools/mkbin.py`, and writes
   `firmware/hoot.bin` and `firmware/version.txt` in the server folder.
   It also writes `firmware/sprig-os.bin`, the name devices on 0.1.x look for.
3. Each Sprig checks `/firmware/version.txt` a minute after boot and
   every six hours after that, whatever is on screen. If the published
   version differs from its own, it streams the image into the update
   partition, marks it and reboots. The "check for update" button on the
   web page makes it check on its next poll instead.

The boot loader swaps the new image in, which takes a few seconds with a
dark screen. The OS confirms the boot after running for 20 seconds. If it
never does, for example because the new image crashes, the next reset
swaps the old image back. The boot loader itself is never updated over
the air.

## Heartbeat, remote warnings and commands

An OS agent runs beside the shell, whatever app is on screen. Every 30 s
it polls `<server>/device/<name>` with `X-Sprig-*` headers: version,
uptime, module, the app on screen, failure count and the last warning.
The server stores them and the web page shows them in the Device card.
Warnings logged on the Sprig are posted to `/log/<name>` as they happen,
at most once a minute, and the card shows the last lines.

The server can hand the Sprig one command per heartbeat in the reply
header `X-Sprig-Command`. The Device card queues them: refetch photo,
clear cache, check for update, reboot, open Wi-Fi setup. Nothing connects
to the Sprig from outside; it all rides on the heartbeat. The agent also
runs the firmware update check. Apps only fetch their own content.

The agent's requests travel on a separate lane of the network service, so
an app can never block them.

## Battery saver

The Sprig runs from two AAA cells or from USB. Battery saver stretches the
cells; it changes nothing that a frame on a desk would notice.

| Setting | Meaning |
| --- | --- |
| Auto | Saver on while on battery, off on USB. A plain Pico reads its USB sense pin; a Pico W asks its radio chip, which holds that pin there. Until the radio is up, auto means off |
| On | Saver always on |
| Off | Saver always off |

Cycle it with "Battery saver" under Settings, or from the web page. About
shows the current state. A "z" in the title bar means saver is active.

What saver does:

- After 30 s without a key press the backlight drops to 30 % of its
  setting. The first key press restores it, and that press does nothing
  else.
- The photo frame polls every 60 s instead of 15 s.
- The heartbeat goes every 5 min instead of 30 s.

The radio already sleeps between packets in every mode, and the display
is only written when something changed, so the rest of the system idles
by itself.

## Hoot, the owl

Hoot is a companion in the spirit of Finch. It is not a mouth to feed: it
grows when you look after yourself, and nothing is lost for a missed day.
It is the first entry of the menu and the screen the menu comes back to
after a quiet minute. It runs in the background whatever is on screen.

The first time, it is an egg. L helps it crack, and Hoot introduces
itself in three lines. After that it is awake on its screen with a life of
its own: it blinks, glances about, flutters its wings now and then, hops
when you tick a goal, and hoots softly when you come to see it. It sleeps
from 22:00 to 07:00 by the clock and snores on the menu.

Each day has a check-in and six small goals. Every one done gives Hoot 20
energy. At 100 the bar is full and K sends Hoot on an adventure: it flies
off, comes back with a discovery, and the count of adventures is what
makes it grow and what earns it things to wear.

| Goal | How it is done |
| --- | --- |
| Check in | Pick one of five faces for how you feel. Once a day. Hoot answers. Stays on the device |
| Drink water, Move a little, See daylight, Wind down | Tick with L. L again takes it back |
| Focus session | Counted for you when a Pomodoro work session ends. L also ticks it |
| Breathe with Hoot | A minute of box breathing: in, hold, out, hold, four rounds. A ring grows and shrinks, the LEDs breathe with it. J stops it early without credit |

| Adventures | Stage | Unlocks |
| --- | --- | --- |
| 1 | | a scarf |
| 3 | owlet | a bow |
| 5 | | a party hat |
| 10 | fledgling | headphones |
| 15 | | a crown |
| 25 | owl | |
| 60 | wise owl, with spectacles | |

D on Hoot's screen cycles through what it has earned, and tells you what
comes next. I shows the week: six days back and today, a face for each
check-in and a bar for the goals done, with the total in the corner.

Hoot has something to say on its screen: a greeting and a question until
you have checked in, a note that a message from home is waiting, praise
after three goals, and otherwise one gentle line a day from a list of
twenty-one.

Family can send a hug from the web page, with their name: it arrives as
the server command `hug:Mum`, gives 10 energy, and Hoot says who it was
from. The five goals besides Check in and Breathe can be renamed per frame
on the web page, in a "Hoot's goals" card. The heartbeat carries a stamp
for them; when it changes the device fetches the names and stores them in
the config record, so they work offline too. The heartbeat carries Hoot's
stage, energy and goals for the day in `X-Sprig-Pet`. The check-in never
leaves the device. Hoot's state lives in the config record, saved after
each change and every ten minutes.

## Clock and alarm

The board has no clock chip. On a Pico W the server sends its time and the
frame's zone offset with every heartbeat, so the clock is right within a
minute of joining Wi-Fi. Set a frame's zone on the web page (an IANA name
such as `Europe/London`); it defaults to the server's own zone. Any board
can have the time set by hand in the Clock app. The time is lost at
power-off.

The Alarm app keeps one daily alarm in the config record: time, on or
off, and a tone. It fires through the `background` hook, so the shell
opens the alarm screen from whatever is showing, plays the tone on repeat
with both LEDs flashing, and gives up after two minutes. L stops it, K
snoozes for five minutes.

## Slideshow

Every photo upload also goes into an album on the server, at frame size
and without the caption. The server keeps the last eight. The web page shows them in an Album
card with a remove button each, and the photo form has an "album only"
box for photos that should join the show without replacing the frame's
picture. The Slideshow app lists them, fetches the newest six it does not yet have into blob
slots 2 to 7, and cycles through them newest first. Photos survive
reboots, so the show runs from flash when the network is down. The list is
refreshed every five minutes.

## Live photos

A Live Photo is a still and a short clip. Send both from the web page: the
picture as usual, and the clip with "add the motion clip". A Pixel motion
photo, which hides its clip inside the JPEG, is found on its own. The
server pulls ten frames out of the clip with ffmpeg, framed like the
still, and answers the frame poll with `X-Sprig-Live: 10`.

While online, the photo frame plays the frames every 45 seconds: each one
is fetched straight into a spare screen buffer in RAM and shown, then the
still comes back from flash. The pace is the network's, a few frames a
second. Offline, or in battery saver, the still is all there is. A plain
upload without a clip clears the motion.

## Sounds

The Sprig has a small speaker on a MAX98357A amplifier. The OS drives it
over I2S from PIO1 and one DMA channel, at 24 kHz and 16 bits. Apps ask
for a named sound and carry on: a tick for a key press, a rising chime
when a Pomodoro work session ends, a falling one when the break ends.
Settings has a Volume screen: A/D move the level from 0 to 10 with a
tick at each step, and L plays the chime. The level is stored in the
config record. The Sounds app under Fun puts seven sounds on seven keys;
the alarm can use four of them as its tone. Timers fire
their chime from the menu too, through the `background` hook that runs
every frame for every app that is not on screen.

## Messages

Two separate channels come from the web page. "Send a message" writes the
caption baked into the photo, as it always did. "Send a note" goes to the
Messages app on the Sprig and never touches the picture. The server keeps
the notes per frame. The agent's heartbeat brings back the unread count;
the menu shows it as a badge on Messages, and the right LED pulses softly
while anything is unread. Marking a note seen in the app gives the sender
the double tick on the web page.

## Wi-Fi setup on the device

The Sprig sets itself up the way the ESP32 frame does, without a rebuild.

1. The setup hotspot opens by itself when no network is saved, when the
   saved network refuses three joins in a row, or when you press K in the
   Network app under Settings. The screen shows a QR code and three steps.
2. Scan the code with a phone, or join the open Wi-Fi `Hoot-Setup`.
3. A page opens by itself. If it does not, open `http://192.168.4.1`.
4. Pick your network from the list, type the password, check the photo
   server and frame name, tap Save.

The Sprig stores the settings, closes the hotspot and joins your network.
The hotspot gives up after five minutes and retries the saved network.

How it works: while still a station the Sprig scans for networks. It then
starts an open access point with a fixed address, and runs three small
servers. DHCP hands the phone an address. DNS answers every name with the
Sprig's address, so the phone's connectivity check lands on the page.
The web server serves the page, and redirects every other path to it.
The packet formats live in `proto/` and are unit-tested on the host.

`os/secrets.toml` is now only a convenience for development builds.

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
| `gfx/` | `hoot-gfx`: framebuffer, RGB565 colour, 5x7 font. No hardware code. |
| `proto/` | `hoot-proto`: URL, HTTP, form, DHCP and DNS codecs, CRC-32, config record. No hardware code. |
| `os/src/main.rs` | Boot sequence and the shell task (the frame loop) |
| `os/src/board.rs` | Pin map and display constants |
| `os/src/drivers/` | ST7735, buttons, PWM dimmer, power monitor, module detection |
| `os/src/storage/` | Config store and blob slots in flash |
| `os/src/net/` | Network handle for apps, HTTP client, Wi-Fi task, setup portal with DHCP and DNS servers |
| `os/src/ota.rs` | Firmware updater: streams images into the update partition, confirms boots |
| `os/src/agent.rs` | OS agent: heartbeat, warnings, server commands, update check |
| `boot/` | `hoot-boot`: the boot loader |
| `os/src/ui/` | Theme, text formatting, splash, shell |
| `os/src/apps/` | The `App` trait, the app template, and the built-in apps |
| `os/firmware/cyw43/` | Radio firmware blobs (Infineon permissive binary license) |
| `os/secrets.example.toml` | Template for build-time defaults |
| `os/src/panic.rs` | Panic handler |
| `os/memory.x` | Flash and RAM layout for the linker |
| `tools/flash.sh` | Cargo runner: flash the OS over USB with picotool |
| `tools/flash-all.sh` | First-time flash: boot loader, radio partition, OS |
| `tools/release.sh` | Publish an over-the-air update to the server |
| `tools/mkbin.py`, `tools/mkradio.py` | Build the flat OS image and the radio partition image |

## Flash map

| Offset | Size | Use |
| --- | --- | --- |
| 0x000000 | 24 KiB | Boot2 and the boot loader (`boot/`) |
| 0x006000 | 4 KiB | Boot loader state |
| 0x007000 | 640 KiB | Active firmware, where the OS runs |
| 0x0A7000 | 644 KiB | Update partition, written over the air |
| 0x148000 | 288 KiB | Radio firmware for the Pico W, written once |
| 0x190000 | 352 KiB | 8 blob slots of 44 KiB. The photo frame uses slots 0 and 1, the slideshow 2 to 7 |
| 0x1E8000 | 32 KiB | Free |
| 0x1F0000 | 8 KiB | Config record, two sectors written alternately |
| 0x1F2000 | 56 KiB | Free |

The boot loader, `os/memory.x` and `os/src/board.rs` all state this map
and must agree.

## Writing an app

Every app has the same shape. Read the module docs at the top of
`os/src/apps/mod.rs`. In short: an `INFO` constant, a struct for state,
and an `App` implementation with `on_enter`, `update` and `on_exit`.
`update` runs each frame and must not block. Long work goes to a service
and is polled the next frame. Register the app in the registry in
`os/src/ui/shell.rs`. Give it a `group`, so the launcher files it under the right submenu; add a group in `apps/mod.rs` when none fits. Set `needs_network: true` if the app cannot work
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

1. Resume the last app after a power cut.
2. Seen button and a message cue in the photo frame.
3. More sounds: an alarm app, a metronome, key clicks in the menu.

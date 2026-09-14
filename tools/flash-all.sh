#!/bin/sh
# First-time flash of a Sprig over USB: boot loader, radio firmware, OS.
# Usage: tools/flash-all.sh [plain|wifi]      (default: wifi)
# Put the Sprig in USB mode first (BOOTSEL, or "Reboot to USB" in the menu).
# Later OS updates need only `cargo run-w` (or `cargo run --release`), or
# arrive over the air.
set -eu
cd "$(dirname "$0")/.."
VARIANT="${1:-wifi}"

echo "-- boot loader"
cargo build --release -p hoot-boot
picotool load -v target/thumbv6m-none-eabi/release/hoot-boot -t elf

if [ "$VARIANT" = "wifi" ]; then
  echo "-- radio firmware partition"
  python3 tools/mkradio.py target/radio.bin
  picotool load -v target/radio.bin -t bin -o 0x10148000
  echo "-- Hoot (wifi)"
  cargo build --release -p hoot --features wifi
else
  echo "-- Hoot (plain)"
  cargo build --release -p hoot
fi
picotool load -v target/thumbv6m-none-eabi/release/hoot -t elf
picotool reboot
echo "done: the Sprig is rebooting into Hoot"

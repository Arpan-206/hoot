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
cargo build --release -p sprig-boot
picotool load -v target/thumbv6m-none-eabi/release/sprig-boot -t elf

if [ "$VARIANT" = "wifi" ]; then
  echo "-- radio firmware partition"
  python3 tools/mkradio.py target/radio.bin
  picotool load -v target/radio.bin -t bin -o 0x10148000
  echo "-- Sprig OS (wifi)"
  cargo build --release -p sprig-os --features wifi
else
  echo "-- Sprig OS (plain)"
  cargo build --release -p sprig-os
fi
picotool load -v target/thumbv6m-none-eabi/release/sprig-os -t elf
picotool reboot
echo "done: the Sprig is rebooting into Sprig OS"

#!/bin/sh
# Flash the OS ELF to a Sprig in USB mode, verify it, then reboot into it.
# Used as the cargo runner, so `cargo run --release` flashes the board.
# The OS lives behind the boot loader, so picotool's own "execute" flag
# cannot be used; a plain reboot starts the boot loader, which starts the OS.
set -eu
picotool load -v "$1" -t elf
picotool reboot

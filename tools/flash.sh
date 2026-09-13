#!/bin/sh
# Flash an ELF to a Sprig in BOOTSEL mode over USB, verify it, then run it.
# Used as the cargo runner, so `cargo run --release` flashes the board.
# picotool needs `-t elf` after the file name, which cargo cannot do itself.
set -eu
exec picotool load -v -x "$1" -t elf

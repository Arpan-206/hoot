#!/bin/sh
# Build the Wi-Fi firmware and publish it for over-the-air update.
# Usage: tools/release.sh <frame-server directory>
# Frames compare /firmware/version.txt with their own version and pull
# /firmware/sprig-os.bin when it differs. Bump the version in Cargo.toml
# ([workspace.package] version) before you release.
set -eu
DIR="${1:?usage: tools/release.sh <frame-server directory>}"
cd "$(dirname "$0")/.."
cargo build --release -p sprig-os --features wifi
python3 tools/mkbin.py target/thumbv6m-none-eabi/release/sprig-os target/sprig-os.bin
V=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
mkdir -p "$DIR/firmware"
cp target/sprig-os.bin "$DIR/firmware/sprig-os.bin"
printf '%s\n' "$V" > "$DIR/firmware/version.txt"
echo "published Sprig OS $V to $DIR/firmware/"

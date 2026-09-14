#!/bin/sh
# Build the Wi-Fi firmware and publish it for over-the-air update.
# Usage: tools/release.sh <frame-server directory>
# Frames compare /firmware/version.txt with their own version and pull
# /firmware/hoot.bin when it differs. Bump the version in Cargo.toml
# ([workspace.package] version) before you release.
set -eu
DIR="${1:?usage: tools/release.sh <frame-server directory>}"
cd "$(dirname "$0")/.."
cargo build --release -p hoot --features wifi
python3 tools/mkbin.py target/thumbv6m-none-eabi/release/hoot target/hoot.bin
V=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
mkdir -p "$DIR/firmware"
cp target/hoot.bin "$DIR/firmware/hoot.bin"
# Devices still on 0.1.x look for the old name. Drop this once none are left.
cp target/hoot.bin "$DIR/firmware/sprig-os.bin"
printf '%s\n' "$V" > "$DIR/firmware/version.txt"
echo "published Hoot $V to $DIR/firmware/"

#!/usr/bin/env python3
"""Build the radio firmware partition image for the Pico W.

Layout (offsets inside the partition):
  0x000  header: magic b"SRAD", version u32 = 1, fw_len u32, clm_len u32,
         nvram_len u32, then zero padding
  0x100  firmware blob, then CLM, then NVRAM, each padded to 4 bytes
Flash it once with:
  picotool load target/radio.bin -t bin -o 0x10148000
"""
import struct, sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "os" / "firmware" / "cyw43"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("target/radio.bin")
PART_SIZE = 0x48000


def pad4(b: bytes) -> bytes:
    return b + b"\0" * (-len(b) % 4)


fw = SRC.joinpath("43439A0.bin").read_bytes()
clm = SRC.joinpath("43439A0_clm.bin").read_bytes()
nvram = SRC.joinpath("nvram_rp2040.bin").read_bytes()
header = struct.pack("<4sIIII", b"SRAD", 1, len(fw), len(clm), len(nvram))
image = header.ljust(0x100, b"\0") + pad4(fw) + pad4(clm) + pad4(nvram)
if len(image) > PART_SIZE:
    sys.exit(f"radio image {len(image)} bytes exceeds partition {PART_SIZE}")
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_bytes(image)
print(f"{OUT}: {len(image)} bytes (fw {len(fw)}, clm {len(clm)}, nvram {len(nvram)})")

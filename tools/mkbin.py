#!/usr/bin/env python3
"""Extract the flat firmware image from the OS ELF for over-the-air updates.

Takes every loadable segment whose load address lies in the active
partition and writes them, in place, as one binary starting at the
partition start. The boot loader copies this byte for byte.
"""
import struct, sys
from pathlib import Path

ACTIVE = 0x10007000
ACTIVE_SIZE = 0xA0000

elf = Path(sys.argv[1]).read_bytes()
out = Path(sys.argv[2])
assert elf[:4] == b"\x7fELF" and elf[4] == 1, "expected a 32-bit ELF"
e_phoff, = struct.unpack_from("<I", elf, 0x1C)
e_phentsize, e_phnum = struct.unpack_from("<HH", elf, 0x2A)
image = bytearray()
for i in range(e_phnum):
    p_type, p_offset, p_vaddr, p_paddr, p_filesz = struct.unpack_from("<IIIII", elf, e_phoff + i * e_phentsize)
    if p_type != 1 or p_filesz == 0:
        continue
    if not (ACTIVE <= p_paddr < ACTIVE + ACTIVE_SIZE):
        if 0x10000000 <= p_paddr < 0x10007000:
            sys.exit(f"segment at {p_paddr:#x} lies in the boot loader area; is boot2-none set?")
        continue
    start = p_paddr - ACTIVE
    end = start + p_filesz
    if len(image) < end:
        image.extend(b"\xff" * (end - len(image)))
    image[start:end] = elf[p_offset:p_offset + p_filesz]
if not image:
    sys.exit("no segments in the active partition")
# whole flash pages, so the updater's writes stay aligned
image.extend(b"\xff" * (-len(image) % 256))
out.parent.mkdir(parents=True, exist_ok=True)
out.write_bytes(image)
print(f"{out}: {len(image)} bytes ({len(image)/1024:.1f} KiB of {ACTIVE_SIZE//1024} KiB)")

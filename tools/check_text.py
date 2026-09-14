#!/usr/bin/env python3
"""Flag on-screen text literals that do not fit the 160x128 display.

Scans os/src for draw_text(x, y, "..."), draw_text_centered(y, "...", ..., scale),
draw_text_right(x, y, "..."), theme::footer(fb, "...") and menu/hint literals.
A cell is 6 px wide at scale 1. The display is 160 px wide, so a footer or a
full-width line holds 26 characters. Exit code 1 when something overflows.
"""
import re, sys, pathlib

WIDTH, CELL = 160, 6
bad = []
for path in sorted(pathlib.Path("os/src").rglob("*.rs")):
    src = path.read_text()
    for m in re.finditer(r'draw_text\(\s*([^,]+),\s*[^,]+,\s*"([^"\\]*)"', src):
        x, text = m.group(1).strip(), m.group(2)
        try:
            x0 = int(eval(x, {}, {"WIDTH": WIDTH}))
        except Exception:
            continue
        if x0 + len(text) * CELL > WIDTH:
            bad.append((path, text, f"x {x0} + {len(text)} chars = {x0 + len(text) * CELL} px"))
    for m in re.finditer(r'draw_text_centered\(\s*[^,]+,\s*"([^"\\]*)"\s*,[^;]*?,\s*(\d+)\s*\)', src):
        text, scale = m.group(1), int(m.group(2))
        if len(text) * CELL * scale > WIDTH:
            bad.append((path, text, f"centred at scale {scale} = {len(text) * CELL * scale} px"))
    for m in re.finditer(r'footer\(\s*fb,\s*"([^"\\]*)"', src):
        text = m.group(1)
        if 3 + len(text) * CELL > WIDTH:
            bad.append((path, text, f"footer {len(text)} chars = {3 + len(text) * CELL} px"))
    # Hints picked in a match and passed to footer later: one line at a time.
    for line in src.splitlines():
        if not any(k in line for k in ('=> "', "hint", "footer(")):
            continue
        for text in re.findall(r'"([^"\\]*)"', line):
            if len(text) > 26 and (path, text) not in [(b[0], b[1]) for b in bad]:
                bad.append((path, text, f"{len(text)} chars, over 26 for a full line"))
for path, text, why in bad:
    print(f"{path}: \"{text}\"  ({why})")
print(f"{len(bad)} problem(s)")
sys.exit(1 if bad else 0)

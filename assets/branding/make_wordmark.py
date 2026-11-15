#!/usr/bin/env python3
"""basso wordmark: the icon with "basso" in a bold pixel font to its right, on the icon's 32px pixel grid.
Writes wordmark.png: teal letters with a navy pixel outline, readable on light and dark backgrounds. Stdlib only.

    python3 -I make_wordmark.py basso-icon.png
"""
import struct
import sys
import zlib
from pathlib import Path

SCALE = 16  # one grid pixel = 16 px in basso-icon.png and in the output
GLYPHS = {
    "b": ["XX.....",
          "XX.....",
          "XX.....",
          "XX.....",
          "XXXXXX.",
          "XXXXXXX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XXXXXXX",
          "XXXXXX."],
    "a": ["XXXXXX.",
          "XXXXXXX",
          ".....XX",
          ".XXXXXX",
          "XXXXXXX",
          "XX...XX",
          "XX...XX",
          "XXXXXXX",
          ".XXXXXX"],
    "s": [".XXXXXX",
          "XXXXXXX",
          "XX.....",
          "XXXXXX.",
          ".XXXXXX",
          ".....XX",
          ".....XX",
          "XXXXXXX",
          "XXXXXX."],
    "o": [".XXXXX.",
          "XXXXXXX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XX...XX",
          "XXXXXXX",
          ".XXXXX."],
}
GAP_ICON, GAP_LETTER, CAP = 6, 2, 13  # grid pixels


def read_png(path):
    data = Path(path).read_bytes()
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n = struct.unpack(">I", data[pos:pos + 4])[0]
        kind, body = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h = struct.unpack(">II", body[:8])
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw, stride = zlib.decompress(idat), w * 4 + 1
    # basso-icon.png is written with filter 0 on every row
    return [[tuple(raw[y * stride + 1 + x * 4: y * stride + 5 + x * 4]) for x in range(w)] for y in range(h)]


def write_png(path, rows):
    h, w = len(rows), len(rows[0])
    raw = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    Path(path).write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def wordmark(icon, colour, outline=None):
    word = "basso"
    text_w = sum(len(GLYPHS[ch][0]) for ch in word) + GAP_LETTER * (len(word) - 1)
    gw, gh = 32 + GAP_ICON + text_w + 2, 32
    grid = [[(0, 0, 0, 0)] * gw for _ in range(gh)]
    top = (gh - CAP) // 2  # text block centred on the icon
    x = 32 + GAP_ICON
    for ch in word:
        rows = GLYPHS[ch]
        y0 = top + CAP - len(rows)  # sit every letter on the same baseline
        for j, row in enumerate(rows):
            for i, px in enumerate(row):
                if px == "X":
                    grid[y0 + j][x + i] = colour
        x += len(rows[0]) + GAP_LETTER
    if outline:
        fill = {(gx, gy) for gy in range(gh) for gx in range(gw) if grid[gy][gx] == colour}
        for (gx, gy) in fill:
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    if (gx + dx, gy + dy) not in fill and 32 <= gx + dx < gw and 0 <= gy + dy < gh:
                        grid[gy + dy][gx + dx] = outline
        # close the 1px holes left between two letters' outlines
        for gy in range(gh):
            for gx in range(33, gw - 1):
                if grid[gy][gx][3] == 0 and grid[gy][gx - 1] == outline and grid[gy][gx + 1] == outline:
                    grid[gy][gx] = outline
    out = []
    for gy in range(gh * SCALE):
        line = []
        for gx in range(gw * SCALE):
            line.append(icon[gy][gx] if gx < 32 * SCALE else grid[gy // SCALE][gx // SCALE])
        out.append(line)
    return out


if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    icon = read_png(sys.argv[1] if len(sys.argv) > 1 else here / "basso-icon.png")
    write_png(here / "wordmark.png", wordmark(icon, (0x86, 0xF0, 0xDF, 255), (0x0A, 0x10, 0x30, 255)))

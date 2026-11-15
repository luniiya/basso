#!/usr/bin/env python3
"""basso icon v7: dark nav-app style in pixel art. Near-black map, every road drawn the same way (3px grey),
the road basso takes highlighted in blue, a white-ringed blue disc with a white arrow, a light "b" top-left.
32x32 grid -> PNG, stdlib only."""
import math
import struct
import sys
import zlib

N = 32


def c(h):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), 255)


CLEAR = (0, 0, 0, 0)
BG = c("#18191d")
BG2 = c("#22252b")
ROAD = c("#3e424c")
ROUTE = c("#2f86f6")
ROUTE_HI = c("#5ea3ff")
RING = c("#eceef2")
RING_SH = c("#c3c7cf")
DISC = c("#2f86f6")
DISC_SH = c("#2672dc")
ARROW = c("#ffffff")
LINE = c("#0f1013")
RIM = c("#565b66")
LETTER = c("#e6e8ec")

CX = int(sys.argv[4]) if len(sys.argv) > 4 else 12
CY = 20
DIAG = int(sys.argv[5]) if len(sys.argv) > 5 else 1

ARROW_PX = ["...X...",
            "..XXX..",
            "..XXX..",
            ".XXXXX.",
            ".XXXXX.",
            "XXX.XXX",
            "XX...XX"]
B = ["XX....",
     "XX....",
     "XX....",
     "XXXXX.",
     "XXXXXX",
     "XX..XX",
     "XX..XX",
     "XXXXXX",
     "XXXXX."]
import os
for _k, _v in (kv.split("=") for kv in os.environ.get("PAL", "").split(",") if kv):
    globals()[_k] = c(_v)
CORNER = [4, 2, 1, 1]
# 17px pixel circle: outer line; ring and fill come from erosion
CIRCLE = [8, 8, 8, 7, 7, 6, 5, 4, 2]


def erode(m):
    return {(x, y) for (x, y) in m if all((x + dx, y + dy) in m for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))}


def band(points_fn, width=3):
    """Road mask: for each x (or y), `width` pixels centred on the line."""
    out = set()
    for (x, y) in points_fn():
        for k in range(-(width // 2), width - width // 2):
            out.add((x, y + k))
    return out


def draw():
    g = [[BG] * N for _ in range(N)]
    # every road is the same: 5px grey. A vertical road through the disc, a diagonal one crossing it at 2:1,
    # and a curved one in the top-right corner.
    roads = set()
    for y in range(N):
        for x in range(CX - 2, CX + 3):
            roads.add((x, y))
    for x in range(N):
        yc = CY + DIAG - (x - CX) // 2
        for k in range(-2, 3):
            roads.add((x, yc + k))
    for (x, y) in roads:
        if 0 <= x < N and 0 <= y < N:
            g[y][x] = ROAD
    # the road basso takes: a blue line in the middle of the vertical road, from the top edge to the disc
    for y in range(0, CY + 1):
        for x in range(CX - 1, CX + 2):
            g[y][x] = ROUTE
    # disc: dark line, 2px white ring (shaded at the bottom), blue fill, white arrow
    outer = {(CX + dx, CY + dy) for dy in range(-8, 9) for dx in range(-CIRCLE[abs(dy)], CIRCLE[abs(dy)] + 1)}
    ring = erode(outer)
    fill = erode(erode(ring))
    for (x, y) in outer:
        g[y][x] = LINE
    for (x, y) in ring:
        g[y][x] = RING
    for (x, y) in fill:
        g[y][x] = DISC
    for j, row in enumerate(ARROW_PX):
        for i, ch in enumerate(row):
            if ch == "X":
                g[CY - 3 + j][CX - 3 + i] = ARROW
    # mirror the map left-right (the arrow is symmetric), then draw the "b" unmirrored in the top-right
    g = [row[::-1] for row in g]
    bx, by = int(sys.argv[2]) if len(sys.argv) > 2 else 4, int(sys.argv[3]) if len(sys.argv) > 3 else 2
    for j, row in enumerate(B):
        for i, ch in enumerate(row):
            if ch == "X":
                g[by + j][bx + i] = LETTER

    def inside(x, y):
        if not (0 <= x < N and 0 <= y < N):
            return False
        ry, rx = min(y, N - 1 - y), min(x, N - 1 - x)
        return not (ry < len(CORNER) and rx < CORNER[ry])

    for y in range(N):
        for x in range(N):
            if not inside(x, y):
                g[y][x] = CLEAR
            elif any(not inside(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                g[y][x] = RIM
    return g


def write_png(path, grid, scale):
    w = h = N * scale
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        row = grid[y // scale]
        for x in range(w):
            raw.extend(row[x // scale])

    def chunk(kind, data):
        cc = kind + data
        return struct.pack(">I", len(data)) + cc + struct.pack(">I", zlib.crc32(cc) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(png)


if __name__ == "__main__":
    write_png(sys.argv[1], draw(), 16)

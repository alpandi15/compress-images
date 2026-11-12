#!/usr/bin/env python3
"""Renders the 1024x1024 source app icon (app-icon.png) with no third-party dependencies.

Regenerate all platform icons afterwards (from app/):
    python3 src-tauri/icons/generate_icon.py
    cargo tauri icon src-tauri/icons/app-icon.png
"""

import math
import os
import struct
import zlib

N = 1024


def mix(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(len(a)))


def sd_round_rect(x, y, x0, y0, x1, y1, r):
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    hx, hy = (x1 - x0) / 2 - r, (y1 - y0) / 2 - r
    dx, dy = abs(x - cx) - hx, abs(y - cy) - hy
    outside = math.hypot(max(dx, 0), max(dy, 0))
    inside = min(max(dx, dy), 0)
    return outside + inside - r


def sd_circle(x, y, cx, cy, r):
    return math.hypot(x - cx, y - cy) - r


def sd_polygon(x, y, pts):
    d = float("inf")
    sign = 1
    n = len(pts)
    for i in range(n):
        ax, ay = pts[i]
        bx, by = pts[(i + 1) % n]
        ex, ey = bx - ax, by - ay
        wx, wy = x - ax, y - ay
        t = max(0.0, min(1.0, (wx * ex + wy * ey) / (ex * ex + ey * ey)))
        d = min(d, math.hypot(wx - ex * t, wy - ey * t))
        c1 = ay <= y < by
        c2 = by <= y < ay
        if (c1 or c2) and x < ax + (y - ay) * ex / ey:
            sign = -sign
    return sign * d


def cover(d):
    """Signed distance → antialiased coverage."""
    return max(0.0, min(1.0, 0.5 - d))


TEAL = (20, 184, 166)
INDIGO = (79, 70, 229)
WHITE = (255, 255, 255)
SUN = (251, 191, 36)
CARD = (290, 330, 734, 694)
MOUNTAINS = [
    [(330, 654), (470, 470), (610, 654)],
    [(500, 654), (600, 540), (700, 654)],
]
ARROWS = [
    [(150, 452), (240, 512), (150, 572)],
    [(874, 452), (784, 512), (874, 572)],
]


def pixel(x, y):
    bg = cover(sd_round_rect(x, y, 100, 100, 924, 924, 185))
    if bg == 0:
        return (0, 0, 0, 0)
    t = ((x - 100) + (y - 100)) / (2 * 824)
    col = mix(TEAL, INDIGO, t)

    card = cover(sd_round_rect(x, y, *CARD, 36))
    col = mix(col, WHITE, card)
    if card > 0:
        inner = mix(TEAL, INDIGO, (y - CARD[1]) / (CARD[3] - CARD[1]))
        for m in MOUNTAINS:
            col = mix(col, inner, cover(sd_polygon(x, y, m)) * card)
        col = mix(col, SUN, cover(sd_circle(x, y, 640, 420, 42)) * card)

    for a in ARROWS:
        col = mix(col, WHITE, cover(sd_polygon(x, y, a) - 10))
    return (*col, bg * 255)


def main():
    rows = []
    for y in range(N):
        row = bytearray(b"\x00")
        for x in range(N):
            row.extend(max(0, min(255, round(c))) for c in pixel(x + 0.5, y + 0.5))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app-icon.png")
    with open(out, "wb") as f:
        f.write(png)
    print("wrote", out)


if __name__ == "__main__":
    main()

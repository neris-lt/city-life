#!/usr/bin/env python3
"""Bakes real terrain heights for Vilnius into data/relief.bin (one byte per 40 m grid point).
Source: SRTM 1-arc-second tile N54E025.hgt.gz from https://s3.amazonaws.com/elevation-tiles-prod/skadi/N54/
placed in data/dem/."""
import gzip, math, os, struct

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LAT0, LON0 = 54.715, 25.245
MX = math.cos(math.radians(LAT0)) * 111320.0
MZ = 110574.0
raw = gzip.open(os.path.join(ROOT, "data", "dem", "N54E025.hgt.gz")).read()
N = 3601
h = struct.unpack(f">{N * N}h", raw)


def height(lat, lon):
    fx = min(max((lon - 25.0) * (N - 1), 0), N - 1.001)
    fy = min(max((55.0 - lat) * (N - 1), 0), N - 1.001)
    x, y = int(fx), int(fy)
    tx, ty = fx - x, fy - y
    a, b, c, d = h[y * N + x], h[y * N + x + 1], h[(y + 1) * N + x], h[(y + 1) * N + x + 1]
    return a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty


STEP = 40.0
X0, X1 = (25.03 - LON0) * MX, (25.47 - LON0) * MX
Z0, Z1 = -(54.835 - LAT0) * MZ, -(54.565 - LAT0) * MZ
W, H = int((X1 - X0) / STEP) + 1, int((Z1 - Z0) / STEP) + 1
vals = []
for j in range(H):
    lat = LAT0 - (Z0 + j * STEP) / MZ
    for i in range(W):
        vals.append(height(lat, LON0 + (X0 + i * STEP) / MX))
base = math.floor(min(vals))
out = b"VRL1" + struct.pack("<fffHHf", X0, Z0, STEP, W, H, base) + bytes(max(0, min(255, int(round(v - base)))) for v in vals)
open(os.path.join(ROOT, "data", "relief.bin"), "wb").write(out)
print(f"{W}x{H} grid, heights {min(vals):.0f}..{max(vals):.0f} m, {len(out) / 1e6:.2f} MB")

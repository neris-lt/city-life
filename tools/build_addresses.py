#!/usr/bin/env python3
"""Builds data/addresses.bin (embedded in the game) for the address search on the city map.

Input: data/osm/addr_*.csv, one address per line as `lat|lon|street|house number`, fetched from
OpenStreetMap with Overpass (run for each quarter of the city's bounding box 54.58,25.05,54.82,25.44):

    [out:csv(::lat,::lon,"addr:street","addr:housenumber";false;"|")][timeout:160];
    nwr["addr:housenumber"]["addr:street"](S,W,N,E);out center;

Output format "VLA1": u16 street count; per street: u8 name length, name (ASCII, upper case,
diacritics dropped like the street names in core.bin), u16 address count; per address:
u8 length, house number, i16 x, i16 z (metres, same projection as tools/build_map.py).
Map data (c) OpenStreetMap contributors, ODbL.
"""
import glob, math, os, re, struct, unicodedata
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LAT0, LON0 = 54.715, 25.245
MX = math.cos(math.radians(LAT0)) * 111320.0
MZ = 110574.0


def ascii_name(s):
    s = unicodedata.normalize("NFKD", s)
    return "".join(ch for ch in s if ord(ch) < 128).upper().strip()


def natural(n):
    m = re.match(r"(\d+)(.*)", n)
    return (int(m.group(1)), m.group(2)) if m else (10 ** 9, n)


streets = defaultdict(dict)
for path in sorted(glob.glob(os.path.join(ROOT, "data", "osm", "addr_*.csv"))):
    for line in open(path, encoding="utf-8"):
        parts = line.rstrip("\n").split("|")
        if len(parts) != 4:
            continue
        try:
            lat, lon = float(parts[0]), float(parts[1])
        except ValueError:
            continue
        street, num = ascii_name(parts[2])[:60], ascii_name(parts[3]).replace(" ", "")
        if not street or not num or len(num) > 8:
            continue
        x, z = round((lon - LON0) * MX), round(-(lat - LAT0) * MZ)
        if abs(x) < 32000 and abs(z) < 32000:
            streets[street].setdefault(num, (x, z))

out = bytearray(b"VLA1")
names = sorted(streets)
out += struct.pack("<H", len(names))
total = 0
for name in names:
    addrs = sorted(streets[name].items(), key=lambda kv: natural(kv[0]))[:65535]
    out += struct.pack("<B", len(name)) + name.encode("ascii") + struct.pack("<H", len(addrs))
    for num, (x, z) in addrs:
        out += struct.pack("<B", len(num)) + num.encode("ascii") + struct.pack("<hh", x, z)
    total += len(addrs)
open(os.path.join(ROOT, "data", "addresses.bin"), "wb").write(out)
print(f"{len(names)} streets, {total} addresses -> data/addresses.bin ({len(out) / 1e6:.2f} MB)")

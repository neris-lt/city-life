#!/usr/bin/env python3
"""Builds data/extras.bin (embedded in the game) from data/landmarks_osm.json:

  * Vilnius airport: runway, taxiways, apron, aircraft stands (OpenStreetMap `aeroway` features,
    which the main map pipeline does not import) and a few parked airliners
  * footprints and heights of famous buildings, which the game draws as hand-modelled landmarks

The source file is an Overpass `out geom` reply; see the queries at the bottom of this file.
Map data (c) OpenStreetMap contributors, ODbL.
"""
import json, math, os, struct

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LAT0, LON0 = 54.715, 25.245  # same origin and projection as tools/build_map.py
MX = math.cos(math.radians(LAT0)) * 111320.0
MZ = 110574.0

# OSM id -> (landmark kind, height in metres as drawn). The game keeps everything under about
# 55 m so the overhead camera never ends up inside a building; taller ones are scaled down.
WAYS = {
    4870323: (1, 20.0),    # Cathedral Basilica
    24569542: (2, 20.0),   # Gediminas' Tower
    81304730: (3, 40.0),   # Cathedral bell tower (57 m)
    90827111: (4, 40.0),   # Europa Tower (129 m)
    111866535: (5, 14.0),  # Town Hall
    387669997: (6, 13.0),  # St. Anne's Church
    88534413: (7, 18.0),   # St. Peter and St. Paul's Church
    112746030: (8, 14.0),  # Gate of Dawn
    24569635: (9, 22.0),   # National Opera and Ballet Theatre
    52710382: (10, 18.0),  # Palace of the Grand Dukes
}
# free-standing monuments without a footprint: (kind, lat, lon)
POINTS = [(11, 54.6871468, 25.214721),   # TV Tower (326 m)
          (12, 54.6867363, 25.2975462)]  # Three Crosses


def proj(g):
    return ((g["lon"] - LON0) * MX, -(g["lat"] - LAT0) * MZ)


def area2(p):
    return sum(p[i][0] * p[(i + 1) % len(p)][1] - p[(i + 1) % len(p)][0] * p[i][1] for i in range(len(p)))


def orient_neg(p):
    return p if area2(p) < 0 else p[::-1]


def earclip(pts):
    idx, tris = list(range(len(pts))), []
    cross = lambda a, b, c: (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    guard = 0
    while len(idx) > 3 and guard < 5000:
        guard += 1
        m, found = len(idx), False
        for k in range(m):
            i0, i1, i2 = idx[(k - 1) % m], idx[k], idx[(k + 1) % m]
            a, b, c = pts[i0], pts[i1], pts[i2]
            if cross(a, b, c) >= -1e-9:
                continue
            if all(j in (i0, i1, i2) or not (cross(a, b, pts[j]) <= 0 and cross(b, c, pts[j]) <= 0 and cross(c, a, pts[j]) <= 0) for j in idx):
                tris.append((i0, i1, i2))
                idx.pop(k)
                found = True
                break
        if not found:
            break
    for k in range(1, len(idx) - 1):
        tris.append((idx[0], idx[k], idx[k + 1]))
    return tris


def ring(geom):
    p = [proj(g) for g in geom]
    if len(p) > 1 and math.dist(p[0], p[-1]) < 0.05:
        p = p[:-1]
    out = []
    for q in p:  # drop repeated points
        if not out or math.dist(out[-1], q) > 0.05:
            out.append(q)
    return orient_neg(out)


def stitch(parts):
    """Joins the outer ways of a multipolygon into closed rings."""
    parts = [[proj(g) for g in p] for p in parts]
    rings = []
    while parts:
        cur = parts.pop(0)
        while math.dist(cur[0], cur[-1]) > 0.5:
            for i, p in enumerate(parts):
                if math.dist(p[0], cur[-1]) < 0.5:
                    cur += parts.pop(i)[1:]
                    break
                if math.dist(p[-1], cur[-1]) < 0.5:
                    cur += parts.pop(i)[::-1][1:]
                    break
            else:
                break
        if len(cur) >= 4 and math.dist(cur[0], cur[-1]) <= 0.5:
            rings.append(cur)
    return rings


els = json.load(open(os.path.join(ROOT, "data", "landmarks_osm.json")))["elements"]
strips, polys, marks, stands = [], [], [], []
for e in els:
    t = e["tags"]
    a = t.get("aeroway")
    if e["type"] == "way" and a in ("runway", "taxiway", "parking_position"):
        pts = [proj(g) for g in e["geometry"]]
        kind = {"runway": 0, "taxiway": 1, "parking_position": 2}[a]
        width = float(t.get("width", {0: 50, 1: 18, 2: 0.3}[kind]))
        strips.append((kind, width, pts))
        if kind == 2:
            stands.append(pts)
    elif e["type"] == "relation" and a == "apron":
        for r in stitch([m["geometry"] for m in e["members"] if m["role"] == "outer"]):
            r = ring([{"lon": p[0] / MX + LON0, "lat": LAT0 - p[1] / MZ} for p in r])
            if len(r) >= 3:
                polys.append((r, earclip(r)))
    elif e["type"] == "way" and e["id"] in WAYS:
        kind, h = WAYS[e["id"]]
        r = ring(e["geometry"])
        cx, cz = sum(p[0] for p in r) / len(r), sum(p[1] for p in r) / len(r)
        marks.append((kind, h, (cx, cz), r, earclip(r)))
for kind, lat, lon in POINTS:
    marks.append((kind, 0.0, proj({"lat": lat, "lon": lon}), [], []))

# parked airliners: on the longer stands, nose at the end of the lead-in line, well apart
planes = []
for pts in sorted(stands, key=lambda p: -math.dist(p[0], p[-1])):
    a, b = pts[-2], pts[-1]
    l = math.dist(a, b)
    if math.dist(pts[0], pts[-1]) < 30 or l < 1:
        continue
    d = ((b[0] - a[0]) / l, (b[1] - a[1]) / l)
    c = (b[0] - d[0] * 17.0, b[1] - d[1] * 17.0)
    if all(math.dist(c, p[0]) > 48 for p in planes):
        planes.append((c, math.atan2(d[1], d[0])))
    if len(planes) >= 8:
        break

out = bytearray(b"VLX1")
out += struct.pack("<H", len(strips))
for kind, width, pts in strips:
    out += struct.pack("<BfH", kind, width, len(pts))
    for p in pts:
        out += struct.pack("<ff", *p)
out += struct.pack("<H", len(polys))
for pts, tris in polys:
    out += struct.pack("<H", len(pts))
    for p in pts:
        out += struct.pack("<ff", *p)
    out += struct.pack("<H", len(tris) * 3)
    for t in tris:
        out += struct.pack("<HHH", *t)
out += struct.pack("<H", len(marks))
for kind, h, c, pts, tris in marks:
    out += struct.pack("<BfffH", kind, h, c[0], c[1], len(pts))
    for p in pts:
        out += struct.pack("<ff", *p)
    out += struct.pack("<H", len(tris) * 3)
    for t in tris:
        out += struct.pack("<HHH", *t)
out += struct.pack("<H", len(planes))
for c, hd in planes:
    out += struct.pack("<fff", c[0], c[1], hd)
open(os.path.join(ROOT, "data", "extras.bin"), "wb").write(out)
print(f"{len(strips)} strips, {len(polys)} apron polygons, {len(marks)} landmarks, {len(planes)} aircraft -> data/extras.bin ({len(out)} bytes)")
for kind, h, c, pts, _ in marks:
    print(f"  landmark {kind}: ({c[0]:.0f}, {c[1]:.0f}) {len(pts)} points")

# Overpass queries used to make data/landmarks_osm.json:
#   (way["aeroway"](54.615,25.26,54.66,25.31);relation["aeroway"](54.615,25.26,54.66,25.31););out geom;
#   (way(id:4870323,24569542,81304730,90827111,111866535,387669997,88534413,112746030,24569635,52710382););out geom;

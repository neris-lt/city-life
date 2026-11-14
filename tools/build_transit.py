#!/usr/bin/env python3
"""Converts the Vilnius public transport GTFS feed (https://www.stops.lt/vilnius/vilnius/gtfs.zip,
unzipped into data/gtfs/) into data/transit.bin: for every route and direction, the real path,
its stops, and how many departures there are in each hour of a working day."""
import csv, math, os, struct, sys
from collections import defaultdict, Counter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
G = os.path.join(ROOT, "data", "gtfs")
LAT0, LON0 = 54.715, 25.245
MX = math.cos(math.radians(LAT0)) * 111320.0
MZ = 110574.0


def rows(name):
    with open(os.path.join(G, name), encoding="utf-8-sig", newline="") as f:
        yield from csv.DictReader(f)


def proj(lat, lon):
    return ((float(lon) - LON0) * MX, -(float(lat) - LAT0) * MZ)


def simplify(pts, eps=1.5):
    if len(pts) < 3:
        return pts
    keep = [False] * len(pts)
    keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        a, b = stack.pop()
        (ax, az), (bx, bz) = pts[a], pts[b]
        dx, dz = bx - ax, bz - az
        L = math.hypot(dx, dz) or 1e-9
        best, bi = 0.0, -1
        for i in range(a + 1, b):
            d = abs((pts[i][0] - ax) * dz - (pts[i][1] - az) * dx) / L
            if d > best:
                best, bi = d, i
        if best > eps:
            keep[bi] = True
            stack += [(a, bi), (bi, b)]
    return [p for p, k in zip(pts, keep) if k]


weekday = {r["service_id"] for r in rows("calendar.txt") if r["wednesday"] == "1"}
routes = {r["route_id"]: r for r in rows("routes.txt")}
stops = {r["stop_id"]: proj(r["stop_lat"], r["stop_lon"]) for r in rows("stops.txt")}
trips = {}
by_key = defaultdict(list)
for t in rows("trips.txt"):
    # night buses only run at weekends; include them so the small hours are not empty
    if (t["service_id"] in weekday or "night" in t["route_id"]) and t["shape_id"]:
        trips[t["trip_id"]] = t
        by_key[(t["route_id"], t["direction_id"])].append(t)
first = {}
stop_seq = defaultdict(list)
# the most-used shape of each route+direction represents it
rep_shape = {k: Counter(t["shape_id"] for t in v).most_common(1)[0][0] for k, v in by_key.items()}
rep_trip = {}
for k, v in by_key.items():
    rep_trip[k] = next(t["trip_id"] for t in v if t["shape_id"] == rep_shape[k])
rep_ids = set(rep_trip.values())
for s in rows("stop_times.txt"):
    tid = s["trip_id"]
    if tid not in trips:
        continue
    seq = int(s["stop_sequence"])
    if tid not in first or seq < first[tid][0]:
        first[tid] = (seq, s["departure_time"])
    if tid in rep_ids:
        stop_seq[tid].append((seq, s["stop_id"]))
shapes = defaultdict(list)
need = set(rep_shape.values())
for s in rows("shapes.txt"):
    if s["shape_id"] in need:
        shapes[s["shape_id"]].append((int(s["shape_pt_sequence"]), proj(s["shape_pt_lat"], s["shape_pt_lon"])))

out = bytearray(b"VTR1")
items = []
for (rid, d), tl in sorted(by_key.items()):
    r = routes[rid]
    hours = [0] * 24
    for t in tl:
        if t["trip_id"] in first:
            hours[int(first[t["trip_id"]][1].split(":")[0]) % 24] += 1
    pts = simplify([p for _, p in sorted(shapes[rep_shape[(rid, d)]])])
    if len(pts) < 2 or sum(hours) == 0:
        continue
    st = [stops[sid] for _, sid in sorted(stop_seq[rep_trip[(rid, d)]]) if sid in stops]
    name = r["route_short_name"].strip('"').encode("ascii", "ignore")[:6]
    kind = 1 if "trol" in rid else 2 if "night" in rid else 0
    items.append((name, kind, hours, st, pts))
out += struct.pack("<H", len(items))
for name, kind, hours, st, pts in items:
    out += struct.pack("<B", len(name)) + name + struct.pack("<B", kind)
    out += bytes(min(255, h) for h in hours)
    out += struct.pack("<H", len(st)) + struct.pack(f"<{len(st) * 2}f", *[c for p in st for c in p])
    out += struct.pack("<I", len(pts)) + struct.pack(f"<{len(pts) * 2}f", *[c for p in pts for c in p])
open(os.path.join(ROOT, "data", "transit.bin"), "wb").write(out)
kinds = Counter(k for _, k, *_ in items)
print(f"{len(items)} route directions (bus {kinds[0]}, trolleybus {kinds[1]}, night {kinds[2]}), {len(out) / 1e6:.2f} MB")
ex = [i for i in items if i[0] in (b"3G", b"2", b"N1")][:3]
for name, kind, hours, st, pts in ex:
    print(name.decode(), "kind", kind, "stops", len(st), "pts", len(pts), "departures/hour", hours)

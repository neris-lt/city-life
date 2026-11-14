#!/usr/bin/env python3
"""Cuts Vilnius out of an OpenStreetMap .osm.pbf (e.g. Geofabrik's lithuania-latest.osm.pbf)
and writes data/osm/chunk_vilnius.json in the same shape as an Overpass `out geom` reply,
which tools/build_map.py consumes.

    python3 -m venv .venv && .venv/bin/pip install osmium
    curl -L -o data/osm/lithuania.osm.pbf https://download.geofabrik.de/europe/lithuania-latest.osm.pbf
    .venv/bin/python tools/extract_pbf.py
"""
import json, os, re, sys
import osmium

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "data", "osm", "lithuania.osm.pbf")
OUT = os.path.join(ROOT, "data", "osm", "chunk_vilnius.json")
S, W, N, E = 54.58, 25.05, 54.82, 25.44  # the whole city municipality, roughly 25 x 27 km

LANDUSE = {"grass", "forest", "meadow", "cemetery", "recreation_ground", "village_green"}
LEISURE = {"park", "pitch", "playground", "garden"}
NATURAL = {"wood", "water", "scrub", "grassland"}
KEEP = ("highway", "building", "building:levels", "name", "oneway", "junction", "lanes", "service", "footway",
        "crossing", "tunnel", "access", "landuse", "leisure", "natural", "amenity", "waterway", "shop", "type",
        "bridge", "layer", "railway", "maxspeed")


def wanted(t):
    return ("highway" in t or "building" in t or "waterway" in t or t.get("railway") in ("rail", "light_rail") or t.get("landuse") in LANDUSE
            or t.get("leisure") in LEISURE or t.get("natural") in NATURAL or t.get("amenity") == "parking")


class Rels(osmium.SimpleHandler):
    def __init__(self):
        super().__init__()
        self.rels, self.members = [], set()

    def relation(self, r):
        t = r.tags
        if t.get("type") != "multipolygon":
            return
        if "building" in t or t.get("natural") in ("water", "wood") or t.get("landuse") == "forest":
            outer = [m.ref for m in r.members if m.type == "w" and m.role == "outer"]
            if outer:
                self.rels.append((r.id, {k: v for k, v in t if k in KEEP}, outer))
                self.members.update(outer)


class Ways(osmium.SimpleHandler):
    def __init__(self, members):
        super().__init__()
        self.members, self.out, self.geom, self.points = members, [], {}, []

    def node(self, n):
        t = n.tags
        if len(t) == 0:
            return
        kind = "signal" if t.get("highway") == "traffic_signals" else "stop" if t.get("highway") == "bus_stop" else \
            "camera" if t.get("highway") == "speed_camera" else "tree" if t.get("natural") == "tree" else None
        if kind and n.location.valid() and S <= n.location.lat <= N and W <= n.location.lon <= E:
            self.points.append({"type": "node", "id": n.id, "lat": round(n.location.lat, 7),
                                "lon": round(n.location.lon, 7), "kind": kind})

    def way(self, w):
        member = w.id in self.members
        if not member and not wanted(w.tags):
            return
        try:
            geom = [(n.lat, n.lon) for n in w.nodes]
        except osmium.InvalidLocationError:
            return
        if len(geom) < 2 or not any(S <= la <= N and W <= lo <= E for la, lo in geom):
            return
        g = [{"lat": round(la, 7), "lon": round(lo, 7)} for la, lo in geom]
        if member:
            self.geom[w.id] = g
        if wanted(w.tags):
            self.out.append({"type": "way", "id": w.id, "nodes": [n.ref for n in w.nodes], "geometry": g,
                             "tags": {k: v for k, v in w.tags if k in KEEP}})


rels = Rels()
rels.apply_file(SRC)
ways = Ways(rels.members)
ways.apply_file(SRC, locations=True)
elements = ways.out + ways.points
for rid, tags, outer in rels.rels:
    members = [{"type": "way", "ref": o, "role": "outer", "geometry": ways.geom[o]} for o in outer if o in ways.geom]
    if members:
        elements.append({"type": "relation", "id": rid, "tags": tags, "members": members})
json.dump({"elements": elements}, open(OUT, "w"), separators=(",", ":"))
print(f"{len(ways.out)} ways, {len(ways.points)} points, {len(rels.rels)} relations -> {OUT} ({os.path.getsize(OUT) / 1e6:.0f} MB)")

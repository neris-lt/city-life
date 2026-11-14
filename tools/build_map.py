#!/usr/bin/env python3
"""Converts OpenStreetMap extracts (Overpass JSON, `out geom`) into the game's map:

  data/core.bin                 road graph, street names, tile index (embedded in the binary)
  web/assets/map/<tx>_<tz>.bin  512m vector tiles streamed by the browser while playing

Input: every data/osm/chunk_*.json (see tools/fetch_osm.sh). If none exist, falls back to
data/seskine_osm.json. Map data (c) OpenStreetMap contributors, ODbL.
"""
import glob, json, math, os, struct, sys, unicodedata, shutil
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_CORE = os.path.join(ROOT, "data", "core.bin")
OUT_TILES = os.path.join(ROOT, "web", "assets", "map")
LAT0, LON0 = 54.715, 25.245  # world origin: Seskine
TILE = 512.0
MX = math.cos(math.radians(LAT0)) * 111320.0
MZ = 110574.0


def proj(g):
    return ((g["lon"] - LON0) * MX, -(g["lat"] - LAT0) * MZ)


def tile_of(p):
    return (int(math.floor(p[0] / TILE)), int(math.floor(p[1] / TILE)))


def area2(pts):
    s = 0.0
    for i in range(len(pts)):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        s += a[0] * b[1] - b[0] * a[1]
    return s


def orient_neg(pts):
    return pts if area2(pts) < 0 else pts[::-1]


def earclip(pts):
    """pts oriented with negative shoelace area. Returns index triples in the same orientation."""
    idx = list(range(len(pts)))
    tris = []

    def cross(a, b, c):
        return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])

    guard = 0
    while len(idx) > 3 and guard < 5000:
        guard += 1
        found = False
        m = len(idx)
        for k in range(m):
            i0, i1, i2 = idx[(k - 1) % m], idx[k], idx[(k + 1) % m]
            a, b, c = pts[i0], pts[i1], pts[i2]
            if cross(a, b, c) >= -1e-9:
                continue
            ok = True
            for j in idx:
                if j == i0 or j == i1 or j == i2:
                    continue
                p = pts[j]
                if cross(a, b, p) <= 0 and cross(b, c, p) <= 0 and cross(c, a, p) <= 0:
                    ok = False
                    break
            if ok:
                tris.append((i0, i1, i2))
                idx.pop(k)
                found = True
                break
        if not found:
            break
    for k in range(1, len(idx) - 1):
        tris.append((idx[0], idx[k], idx[k + 1]))
    return tris


def clip_rect(pts, x0, z0, x1, z1):
    """Sutherland-Hodgman clip of a polygon against an axis-aligned rectangle."""
    def clip(poly, inside, inter):
        out = []
        for i in range(len(poly)):
            a, b = poly[i], poly[(i + 1) % len(poly)]
            ia, ib = inside(a), inside(b)
            if ia:
                out.append(a)
            if ia != ib:
                out.append(inter(a, b))
        return out

    def ix(x):
        return lambda a, b: (x, a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0]))

    def iz(z):
        return lambda a, b: (a[0] + (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]), z)

    for inside, inter in ((lambda p: p[0] >= x0, ix(x0)), (lambda p: p[0] <= x1, ix(x1)),
                          (lambda p: p[1] >= z0, iz(z0)), (lambda p: p[1] <= z1, iz(z1))):
        pts = clip(pts, inside, inter)
        if len(pts) < 3:
            return []
    return pts


def ascii_name(s):
    s = unicodedata.normalize("NFKD", s)
    return "".join(ch for ch in s if ord(ch) < 128).upper()[:60]


def d2(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def split_len(pts, maxlen=350.0, step=120.0):
    """Split a polyline into pieces no longer than ~maxlen (subdividing long segments).
    Points may carry extra components (elevation), which are interpolated."""
    dense = [pts[0]]
    for a, b in zip(pts, pts[1:]):
        n = max(1, int(math.ceil(d2(a, b) / step)))
        for k in range(1, n + 1):
            dense.append(tuple(a[i] + (b[i] - a[i]) * k / n for i in range(len(a))))
    out, cur, acc = [], [dense[0]], 0.0
    for a, b in zip(dense, dense[1:]):
        acc += d2(a, b)
        cur.append(b)
        if acc > maxlen:
            out.append(cur)
            cur, acc = [b], 0.0
    if len(cur) > 1:
        out.append(cur)
    return out


# ---- load ------------------------------------------------------------------
files = sorted(glob.glob(os.path.join(ROOT, "data", "osm", "chunk_*.json")))
files = [f for f in files if os.path.getsize(f) > 10000]
if not files:
    files = [os.path.join(ROOT, "data", "seskine_osm.json")]
ways, rels, points, seen = [], [], [], set()
for f in files:
    for e in json.load(open(f))["elements"]:
        key = (e["type"], e["id"])
        if key in seen:
            continue
        seen.add(key)
        if e["type"] == "way" and "geometry" in e:
            ways.append(e)
        elif e["type"] == "relation":
            rels.append(e)
        elif e["type"] == "node":
            points.append(e)
print(f"{len(files)} extract(s): {len(ways)} ways, {len(rels)} relations", flush=True)

DRIVE = {
    "motorway": (2, 0), "motorway_link": (1, 0),
    "trunk": (3, 0), "trunk_link": (1, 0), "primary": (2, 0), "primary_link": (1, 0),
    "secondary": (2, 1), "secondary_link": (1, 1), "tertiary": (2, 1), "tertiary_link": (1, 1),
    "residential": (2, 2), "unclassified": (2, 2), "living_street": (2, 2),
}
WALK = {"footway": 2.2, "path": 1.4, "pedestrian": 4.0, "steps": 2.0, "cycleway": 2.0, "track": 2.5}

tiles = defaultdict(lambda: {"ways": [], "blds": [], "areas": [], "pts": []})
names, name_ix = [], {}


def name_id(s):
    if not s:
        return 0xFFFF
    s = ascii_name(s)
    if s not in name_ix:
        if len(names) >= 0xFFF0:
            return 0xFFFF
        name_ix[s] = len(names)
        names.append(s)
    return name_ix[s]


def add_way(kind, pts, width, flags=0, lanes=1, lclass=3, mark=0, presplit=False, elev=None, step=120.0):
    """elev: per-point deck height in metres (bridges, ramps)."""
    if elev and max(abs(e) for e in elev) > 0.15:
        pts = [(p[0], p[1], e) for p, e in zip(pts, elev)]
        flags |= 8
    for piece in ([pts] if presplit and len(pts) < 60 else split_len(pts, step=step)):
        if len(piece) >= 2:
            tiles[tile_of(piece[0])]["ways"].append((kind, width, flags, lanes, lclass, mark, piece))


def is_bridge(t):
    return t.get("bridge") not in (None, "no")


def speed_limit(t, hw):
    """Posted limit in km/h, falling back to the Lithuanian defaults."""
    v = t.get("maxspeed", "")
    try:
        return max(10, min(130, int(v.split()[0])))
    except (ValueError, IndexError):
        return 110 if hw.startswith("motorway") else 20 if hw == "living_street" else 50


drive_ways, walk_ways = [], []
for w in ways:
    t = w.get("tags", {})
    hw = t.get("highway")
    if not hw or hw in ("proposed", "construction", "platform", "bus_stop", "elevator", "corridor", "razed", "abandoned"):
        continue
    if t.get("tunnel") == "building_passage" and hw not in DRIVE:
        continue
    tunnel = t.get("tunnel") in ("yes", "culvert") or t.get("covered") == "yes" and t.get("layer", "0").startswith("-")
    if t.get("access") in ("private", "no") and hw not in DRIVE:
        continue
    pts = [proj(g) for g in w["geometry"]]
    ids = w["nodes"]
    if hw in DRIVE:
        dl, lclass = DRIVE[hw]
        oneway = t.get("oneway") in ("yes", "1", "true") or t.get("junction") == "roundabout" or hw.startswith("motorway")
        try:
            lanes = int(t.get("lanes", "").split(";")[0])
        except ValueError:
            lanes = dl
            if oneway and hw in ("primary", "secondary", "tertiary"):
                lanes = 2
        lanes = max(1, min(6, lanes))
        width = max(4.5 if oneway else 6.0, lanes * 3.5)
        nm = name_id(t.get("name"))
        if oneway:
            mark = 3 if lanes >= 2 else 0
        else:
            mark = 2 if lanes >= 4 else 1 if (lclass <= 1 or hw == "residential") else 0
        try:
            layer = max(1, min(2, int(t.get("layer", "1"))))
        except ValueError:
            layer = 1
        drive_ways.append((pts, ids, oneway, width, lanes, nm, lclass, mark,
                           6.0 * layer if is_bridge(t) else 0.0, t.get("junction") == "roundabout",
                           6.0 if tunnel else 0.0, speed_limit(t, hw)))
    elif hw == "service":
        add_way(1, pts, 5.0 if t.get("service") == "parking_aisle" else 4.2)
    elif hw in WALK:
        crossing = t.get("footway") == "crossing" or t.get("crossing") in ("marked", "zebra", "uncontrolled", "traffic_signals")
        walk_ways.append((pts, ids, 2 if tunnel else 3 if crossing else 2, WALK[hw],
                          -1 if tunnel else 1 if (is_bridge(t) and not crossing) else 0))

# ---- elevation: bridges sit on a deck, and the roads leading to them ramp up -------------
adj = defaultdict(list)
for d in drive_ways:
    pts, ids = d[0], d[1]
    for i in range(1, len(ids)):
        L = d2(pts[i - 1], pts[i])
        adj[ids[i - 1]].append((ids[i], L))
        adj[ids[i]].append((ids[i - 1], L))
import heapq
SLOPE = 0.05


def spread(col):
    """Heights at structure nodes, fading out along connected roads as approach ramps."""
    h_at = defaultdict(float)
    for d in drive_ways:
        if d[col]:
            for i in d[1]:
                h_at[i] = max(h_at[i], d[col])
    heap = [(-h, n) for n, h in h_at.items()]
    heapq.heapify(heap)
    while heap:
        h, n = heapq.heappop(heap)
        h = -h
        if h < h_at[n] - 1e-6:
            continue
        for m, L in adj[n]:
            hm = h - SLOPE * L
            if hm > 0.15 and hm > h_at[m] + 1e-6:
                h_at[m] = hm
                heapq.heappush(heap, (-hm, m))
    return h_at


up, down = spread(8), spread(10)
nbridge = ntunnel = 0
for pts, ids, oneway, width, lanes, nm, lclass, mark, deck, rb, dep, _ms in drive_ways:
    elev = [up.get(i, 0.0) - down.get(i, 0.0) for i in ids]
    nbridge += 1 if deck else 0
    ntunnel += 1 if dep else 0
    sloped = max(abs(e) for e in elev) > 0.15
    add_way(0, pts, width, (1 if oneway else 0) | (4 if deck else 0) | (16 if dep else 0), lanes, lclass, mark,
            elev=elev, step=10.0 if sloped else 120.0)
print(f"tunnels {ntunnel}", flush=True)
print(f"bridges {nbridge}", flush=True)

# roundabouts: find each ring and give it a landscaped island
parent = {}


def find(x):
    while parent.setdefault(x, x) != x:
        parent[x] = parent[parent[x]]
        x = parent[x]
    return x


for d in drive_ways:
    if d[9]:
        for i in d[1][1:]:
            parent[find(i)] = find(d[1][0])
rings = defaultdict(list)
for d in drive_ways:
    if d[9]:
        rings[find(d[1][0])].append(d)
nround = 0
for parts in rings.values():
    allp = [p for d in parts for p in d[0]]
    c = (sum(p[0] for p in allp) / len(allp), sum(p[1] for p in allp) / len(allp))
    r = sum(d2(p, c) for p in allp) / len(allp) - max(d[3] for d in parts) / 2 - 0.9
    if 1.5 < r < 60:
        tiles[tile_of(c)]["areas"].append((5, [c, (c[0] + r, c[1])], []))
        nround += 1
print(f"roundabouts {nround}", flush=True)

rivers = []
for w in ways:
    t = w.get("tags", {})
    if t.get("railway") in ("rail", "light_rail") and t.get("tunnel") != "yes" and "highway" not in t:
        add_way(5, [proj(g) for g in w["geometry"]], 3.4)
for w in ways:
    t = w.get("tags", {})
    ww = t.get("waterway")
    if ww in ("river", "stream", "ditch", "canal", "drain") and "highway" not in t and t.get("tunnel") != "culvert":
        nm = ascii_name(t.get("name", ""))
        width = 75.0 if "NERIS" in nm else 11.0 if ww == "river" else 2.5
        add_way(4, [proj(g) for g in w["geometry"]], width)
        if ww == "river":
            rivers.append((width, [proj(g) for g in w["geometry"]]))


def split_junctions(wlist):
    use = defaultdict(int)
    for item in wlist:
        ids = item[1]
        for i in ids:
            use[i] += 1
        use[ids[0]] += 1
        use[ids[-1]] += 1
    for item in wlist:
        pts, ids = item[0], item[1]
        start = 0
        for i in range(1, len(ids)):
            if use[ids[i]] > 1 or i == len(ids) - 1:
                yield item, ids[start], ids[i], pts[start:i + 1]
                start = i


# footpaths: split at junctions so the game can stitch a walkable graph from tile data
for item, _, _, seg in split_junctions(walk_ways):
    if len(seg) < 2:
        continue
    if item[4]:
        # footbridge or underpass: densify, then arch it up or dip it down
        dense = split_len(seg, maxlen=1e9, step=5.0)[0]
        cum = [0.0]
        for a, b in zip(dense, dense[1:]):
            cum.append(cum[-1] + d2(a, b))
        L = cum[-1]
        ramp = max(4.0, min(28.0, L * 0.35))
        hgt = 5.5 if item[4] > 0 else -3.6
        elev = [hgt * min(1.0, c / ramp, (L - c) / ramp) for c in cum]
        add_way(item[2], dense, item[3], flags=2 | (4 if item[4] > 0 else 16), presplit=True, elev=elev)
    else:
        add_way(item[2], seg, item[3], flags=2, presplit=True)

# drivable graph (global)
d_nodes, d_nix, d_edges = [], {}, []
for item, na, nb, seg in split_junctions(drive_ways):
    if len(seg) < 2 or sum(math.dist(a, b) for a, b in zip(seg, seg[1:])) < 0.5:
        continue
    for n_, p in ((na, seg[0]), (nb, seg[-1])):
        if n_ not in d_nix:
            d_nix[n_] = len(d_nodes)
            d_nodes.append(p)
    d_edges.append((d_nix[na], d_nix[nb], item[2], item[3], item[4], item[5], item[6], seg, item[11]))

# ---- buildings -------------------------------------------------------------
SMALL = {"garage", "garages", "shed", "hut", "kiosk", "service", "roof", "carport", "transformer_tower", "toilets", "greenhouse", "allotment_house"}
COMM = {"retail", "commercial", "supermarket", "office", "hotel", "mall", "kiosk"}
RESI = {"apartments", "residential", "dormitory", "house", "detached", "semidetached_house", "terrace"}
nb = 0


def add_building(geom, t):
    global nb
    pts = [proj(g) for g in geom]
    if len(pts) < 4 or math.dist(pts[0], pts[-1]) > 0.5:
        return
    out = []
    for p in pts[:-1]:
        if not out or math.dist(p, out[-1]) > 0.3:
            out.append(p)
    pts = out
    if len(pts) < 3 or len(pts) > 300:
        return
    pts = orient_neg(pts)
    a = abs(area2(pts)) / 2
    xs = [p[0] for p in pts]
    zs = [p[1] for p in pts]
    if a < 8 or max(xs) - min(xs) > 480 or max(zs) - min(zs) > 480:
        return
    b = t.get("building", "yes")
    try:
        lv = int(float(t.get("building:levels", "")))
    except ValueError:
        lv = 1 if (b in SMALL or a < 70) else 5 if b in RESI and a > 300 else 2
    lv = max(1, min(40, lv))
    kind = 2 if (b in SMALL or (a < 70 and lv <= 1)) else 1 if (b in COMM or t.get("shop")) else 0 if (b in RESI or lv >= 4) else 3
    cen = (sum(xs) / len(xs), sum(zs) / len(zs))
    tiles[tile_of(cen)]["blds"].append((lv, kind, pts, earclip(pts)))
    nb += 1


for w in ways:
    if "building" in w.get("tags", {}):
        add_building(w["geometry"], w["tags"])
for r in rels:
    if "building" in r.get("tags", {}):
        for m in r.get("members", []):
            if m.get("role") == "outer" and "geometry" in m:
                add_building(m["geometry"], r["tags"])
print(f"buildings {nb}", flush=True)

# ---- areas -----------------------------------------------------------------
def add_area(pts, t):
    if len(pts) < 4 or math.dist(pts[0], pts[-1]) > 0.5:
        return
    pts = orient_neg(pts[:-1])
    xs = [p[0] for p in pts]
    zs = [p[1] for p in pts]
    kind = None
    if t.get("natural") == "water":
        kind = 0
    elif t.get("amenity") == "parking":
        kind = 1
    elif t.get("leisure") == "pitch":
        kind = 2
    if kind is not None:
        if len(pts) <= 350 and max(xs) - min(xs) < 480 and max(zs) - min(zs) < 480:
            cen = (sum(xs) / len(xs), sum(zs) / len(zs))
            tiles[tile_of(cen)]["areas"].append((kind, pts, earclip(pts)))
        return
    if t.get("landuse") == "forest" or t.get("natural") in ("wood", "scrub"):
        kind = 3
    elif t.get("leisure") in ("park", "garden") or t.get("landuse") in ("cemetery", "village_green", "recreation_ground"):
        kind = 4
    if kind is None:
        return
    for tx in range(int(min(xs) // TILE), int(max(xs) // TILE) + 1):
        for tz in range(int(min(zs) // TILE), int(max(zs) // TILE) + 1):
            c = clip_rect(pts, tx * TILE, tz * TILE, (tx + 1) * TILE, (tz + 1) * TILE)
            if len(c) >= 3 and abs(area2(c)) > 40:
                tiles[(tx, tz)]["areas"].append((kind, c[:2000], []))


for w in ways:
    t = w.get("tags", {})
    if "highway" in t or "building" in t or "waterway" in t:
        continue
    add_area([proj(g) for g in w["geometry"]], t)
for r in rels:
    t = r.get("tags", {})
    if t.get("natural") == "water" or t.get("landuse") == "forest" or t.get("natural") == "wood":
        for m in r.get("members", []):
            if m.get("role") == "outer" and "geometry" in m:
                add_area([proj(g) for g in m["geometry"]], t)

# ---- point features --------------------------------------------------------
PK = {"signal": 0, "stop": 1, "tree": 2}
# speed cameras take the limit of the road they watch
cam_grid = defaultdict(list)
for d in drive_ways:
    for p in d[0]:
        cam_grid[(int(p[0] // 60), int(p[1] // 60))].append((p, d[11]))
ncam = 0
for e in points:
    p = proj(e)
    if e["kind"] == "camera":
        near = [(d2(p, q), ms) for dx in (-1, 0, 1) for dz in (-1, 0, 1)
                for q, ms in cam_grid.get((int(p[0] // 60) + dx, int(p[1] // 60) + dz), ())]
        if not near:
            continue
        tiles[tile_of(p)]["pts"].append((10 + min(near)[1] // 10, p))
        ncam += 1
    else:
        tiles[tile_of(p)]["pts"].append((PK[e["kind"]], p))
print(f"speed cameras {ncam}", flush=True)
print(f"points {len(points)}", flush=True)

# ---- spawn -----------------------------------------------------------------
sesk = [d[0] for d in drive_ways if d[5] != 0xFFFF and names[d[5]] == "SESKINES G."]
target = sesk[len(sesk) // 2][len(sesk[len(sesk) // 2]) // 2] if sesk else (0.0, 0.0)
spawn = min((p for w in walk_ways for p in w[0]), key=lambda p: math.dist(p, target), default=target)

# ---- write -----------------------------------------------------------------
if os.path.isdir(OUT_TILES):
    shutil.rmtree(OUT_TILES)
os.makedirs(OUT_TILES)
txs = [k[0] for k in tiles]
tzs = [k[1] for k in tiles]
tx0, tz0, tw, th = min(txs), min(tzs), max(txs) - min(txs) + 1, max(tzs) - min(tzs) + 1


def q(v):
    return max(-32767, min(32767, int(round(v * 10.0))))


def ease(h):
    """Ramps computed as straight lines get an S-shaped profile so they meet the deck and the ground smoothly."""
    s = abs(h)
    if s < 6.0:
        t = s / 6.0
        s = 6.0 * t * t * (3.0 - 2.0 * t)
    return s if h >= 0 else -s


total = 0
for (tx, tz), t in tiles.items():
    ox, oz = tx * TILE, tz * TILE
    out = bytearray(b"VLT1")
    W = lambda fmt, *v: out.extend(struct.pack("<" + fmt, *v))
    pv = lambda pts: out.extend(struct.pack(f"<{len(pts) * 2}h", *[q(c) for p in pts for c in (p[0] - ox, p[1] - oz)]))
    out[3] = ord("2")
    W("H", min(65535, len(t["ways"])))
    for kind, width, flags, lanes, lclass, mark, pts in t["ways"][:65535]:
        W("BHBBBBH", kind, min(65535, int(width * 10)), flags, lanes, lclass, mark, len(pts))
        pv(pts)
        if flags & 8:
            out.extend(struct.pack(f"<{len(pts)}b", *[max(-127, min(127, int(round(ease(p[2]) * 10)))) for p in pts]))
    W("H", min(65535, len(t["blds"])))
    for lv, kind, pts, tris in t["blds"][:65535]:
        W("BBH", lv, kind, len(pts))
        pv(pts)
        W("H", len(tris) * 3)
        out.extend(struct.pack(f"<{len(tris) * 3}H", *[i for tr in tris for i in tr]))
    W("H", min(65535, len(t["areas"])))
    for kind, pts, tris in t["areas"][:65535]:
        W("BH", kind, len(pts))
        pv(pts)
        W("H", len(tris) * 3)
        out.extend(struct.pack(f"<{len(tris) * 3}H", *[i for tr in tris for i in tr]))
    W("H", min(65535, len(t["pts"])))
    for kind, p in t["pts"][:65535]:
        W("Bhh", kind, q(p[0] - ox), q(p[1] - oz))
    open(os.path.join(OUT_TILES, f"{tx}_{tz}.bin"), "wb").write(out)
    total += len(out)

core = bytearray(b"VLN4")
W = lambda fmt, *v: core.extend(struct.pack("<" + fmt, *v))
W("ff", spawn[0], spawn[1])
W("H", len(names))
for n in names:
    b = n.encode("ascii")
    W("B", len(b))
    core += b
W("iiHH", tx0, tz0, tw, th)
for z in range(th):
    for x in range(tw):
        W("B", 1 if (tx0 + x, tz0 + z) in tiles else 0)
W("I", len(d_nodes))
W("I", len(d_edges))
for a, b, oneway, width, lanes, nm, lclass, seg, ms in d_edges:
    W("IIBBBBBHH", a, b, 1 if oneway else 0, min(255, int(width * 10)), lanes, lclass, ms, nm, len(seg))
    core.extend(struct.pack(f"<{len(seg) * 2}f", *[c for p in seg for c in p]))
W("I", len(rivers))
for width, seg in rivers:
    W("HH", int(width * 10), min(65535, len(seg)))
    core.extend(struct.pack(f"<{min(65535, len(seg)) * 2}f", *[c for p in seg[:65535] for c in p]))
open(OUT_CORE, "wb").write(core)
print(f"tiles {len(tiles)} ({tw}x{th} grid, {tw * TILE / 1000:.1f} x {th * TILE / 1000:.1f} km), {total / 1e6:.1f} MB")
print(f"core {len(core) / 1e6:.2f} MB: drive edges {len(d_edges)}, names {len(names)}, spawn {spawn}")

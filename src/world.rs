//! The city: a 1:1 scale reconstruction of Vilnius built from OpenStreetMap
//! data (see `tools/build_map.py`). The drivable road graph is embedded in the
//! binary; everything else arrives as 512m vector tiles fetched on demand, and
//! geometry is meshed in 64m cells around the camera.
//! Map data © OpenStreetMap contributors (ODbL).

use crate::Game;
use crate::art::*;
use bevy::asset::{AssetLoader, LoadContext, LoadState, io::Reader};
use bevy::light::NotShadowCaster;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::reflect::TypePath;

pub const CELL: f32 = 64.0;
pub const TILE: f32 = 512.0;
pub const LOAD_R: i32 = 3;
const TILE_R: i32 = 2;
static CORE: &[u8] = include_bytes!("../data/core.bin");
static RELIEF: &[u8] = include_bytes!("../data/relief.bin");

/// Real terrain height in metres above sea level (SRTM, 40 m grid), bilinear.
pub fn relief_height(p: Vec2) -> f32 {
    let f = |o: usize| f32::from_le_bytes(RELIEF[o..o + 4].try_into().unwrap());
    let (x0, z0, step) = (f(4), f(8), f(12));
    let w = u16::from_le_bytes([RELIEF[16], RELIEF[17]]) as usize;
    let h = u16::from_le_bytes([RELIEF[18], RELIEF[19]]) as usize;
    let base = f(20);
    let fx = ((p.x - x0) / step).clamp(0.0, w as f32 - 1.001);
    let fz = ((p.y - z0) / step).clamp(0.0, h as f32 - 1.001);
    let (ix, iz) = (fx as usize, fz as usize);
    let (tx, tz) = (fx - ix as f32, fz - iz as f32);
    let at = |x: usize, z: usize| RELIEF[24 + z * w + x] as f32;
    let (a, b, c, d) = (at(ix, iz), at(ix + 1, iz), at(ix, iz + 1), at(ix + 1, iz + 1));
    base + a + (b - a) * tx + (c - a) * tz + (a - b - c + d) * tx * tz
}

/// Height of the ground relative to the spawn area, in metres. Smooth (Catmull-Rom over the
/// 40 m grid) so roads, kerbs and grass laid on it never fight each other.
pub fn terrain(p: Vec2) -> f32 {
    let f = |o: usize| f32::from_le_bytes(RELIEF[o..o + 4].try_into().unwrap());
    let (x0, z0, step) = (f(4), f(8), f(12));
    let w = u16::from_le_bytes([RELIEF[16], RELIEF[17]]) as i32;
    let h = u16::from_le_bytes([RELIEF[18], RELIEF[19]]) as i32;
    let fx = ((p.x - x0) / step).clamp(1.0, w as f32 - 3.0);
    let fz = ((p.y - z0) / step).clamp(1.0, h as f32 - 3.0);
    let (ix, iz) = (fx.floor() as i32, fz.floor() as i32);
    let (tx, tz) = (fx - ix as f32, fz - iz as f32);
    let at = |x: i32, z: i32| RELIEF[24 + (z * w + x) as usize] as f32;
    let cr = |a: f32, b: f32, c: f32, d: f32, t: f32| {
        b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
    };
    let row = |z: i32| cr(at(ix - 1, z), at(ix, z), at(ix + 1, z), at(ix + 2, z), tx);
    cr(row(iz - 1), row(iz), row(iz + 1), row(iz + 2), tz) + f(20) - 165.0
}

/// Hillshade factor for the ground at `p`: slopes facing the north-west light are brighter,
/// slopes facing away are darker, and high ground is slightly lighter than valleys.
pub fn relief_shade(p: Vec2) -> f32 {
    let e = 45.0;
    let dx = (relief_height(p + Vec2::X * e) - relief_height(p - Vec2::X * e)) / (2.0 * e);
    let dz = (relief_height(p + Vec2::Y * e) - relief_height(p - Vec2::Y * e)) / (2.0 * e);
    let lit = (dx + dz) * 0.707; // downhill direction dotted with the direction to the light
    let alt = (relief_height(p) - 140.0) / 100.0;
    (1.0 + lit * 5.5 + alt * 0.16).clamp(0.55, 1.5)
}

// ---- tile asset -----------------------------------------------------------
#[derive(Asset, TypePath)]
pub struct TileBlob(pub Vec<u8>);

#[derive(Default, TypePath)]
pub struct TileLoader;

impl AssetLoader for TileLoader {
    type Asset = TileBlob;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<TileBlob, std::io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(TileBlob(bytes))
    }
    fn extensions(&self) -> &[&str] {
        &["bin"]
    }
}

// ---- data -----------------------------------------------------------------
pub struct Bld {
    pub lv: u8,
    pub kind: u8, // 0 apartments, 1 commercial, 2 garage/shed, 3 other
    pub pts: Vec<Vec2>,
    pub tris: Vec<u16>,
    pub min: Vec2,
    pub max: Vec2,
}

impl Bld {
    pub fn height(&self) -> f32 {
        (self.lv as f32 * 2.7).clamp(3.0, 36.0)
    }
}

struct Area {
    kind: u8, // 0 water, 1 parking, 2 pitch, 3 forest, 4 park
    pts: Vec<Vec2>,
    min: Vec2,
    max: Vec2,
}

pub struct Edge {
    pub a: u32,
    pub b: u32,
    pub oneway: bool,
    pub width: f32,
    pub lanes: u8,
    pub name: u16,
    /// 0 = arterial, 1 = main street, 2 = local street.
    pub class: u8,
    /// Posted speed limit in km/h.
    pub limit: u8,
    pub pts: Vec<Vec2>,
}

impl Edge {
    /// k-th point in travel order.
    pub fn pt(&self, fwd: bool, k: usize) -> Vec2 {
        if fwd { self.pts[k] } else { self.pts[self.pts.len() - 1 - k] }
    }
    pub fn end(&self, fwd: bool) -> u32 {
        if fwd { self.b } else { self.a }
    }
}

#[derive(Default)]
pub struct Graph {
    pub edges: Vec<Edge>,
    pub adj: Vec<Vec<u32>>,
    pub node_pos: Vec<Vec2>,
}

impl Graph {
    /// Edges that can be taken when arriving at `node`, as (edge, forward).
    pub fn exits(&self, node: u32, from: u32) -> Vec<(u32, bool)> {
        let mut out = vec![];
        for &e in &self.adj[node as usize] {
            let ed = &self.edges[e as usize];
            if ed.a == node {
                out.push((e, true));
            }
            if ed.b == node && !ed.oneway && ed.a != ed.b {
                out.push((e, false));
            }
        }
        // never pick an immediate U-turn if there is another way
        if out.iter().any(|&(e, _)| e != from) {
            out.retain(|&(e, _)| e != from);
        }
        out
    }
}

type Quad3 = [Vec3; 4];

#[derive(Default)]
pub struct CellSrc {
    tris: [Vec<[Vec2; 3]>; 4],
    /// Elevated road / footbridge surfaces: [asphalt, paving].
    deck: [Vec<[Vec3; 3]>; 2],
    quads: Vec<(Quad3, u8, u8, u32)>,
    /// Carriageways for sorting out road paint: a, b, half width, way id, height.
    roads: Vec<(Vec2, Vec2, f32, u32, f32)>,
    /// Extra textured quads (bridge fascias, barriers, retaining walls).
    equads: Vec<(Quad3, u32, [f32; 4])>,
    boxes: Vec<(Vec3, Vec3, u32, [f32; 4])>,
    /// Contact-shadow quads: corners and darkness.
    shades: Vec<(Quad3, f32)>,
    /// Outer edges of raised / sunken roads: p, q (ordered so the outside is on the right), heights,
    /// kind (0 bridge, 1 embankment, 2 open cutting, 3 tunnel) and whether to paint an edge line.
    edges: Vec<(Vec2, Vec2, f32, f32, u8, bool)>,
    own: Vec<u32>,
    col: Vec<u32>,
    pub segs: Vec<(Vec2, Vec2, f32)>,
    /// Drivable deck segments: a, b, half width, height at a, height at b.
    decks: Vec<(Vec2, Vec2, f32, f32, f32)>,
    /// Open cuttings (tunnel approach ramps): the ground is removed over these.
    holes: Vec<(Vec2, Vec2, f32)>,
    lights: Vec<(Vec2, u8, f32)>,
    areas: Vec<u32>,
    trees: Vec<(Vec2, u8)>,
    rounds: Vec<(Vec2, f32)>,
    islands: Vec<(Vec2, f32)>,
    props: Vec<(Vec2, u8)>,
    /// Speed cameras: position and the limit (km/h) they enforce.
    pub cams: Vec<(Vec2, u32)>,
    parked: Vec<(Vec2, f32)>,
    scattered: bool,
    pub walk: Vec<u32>,
    pub poi: Option<Vec2>,
}

#[derive(Clone)]
pub struct LightSite {
    pub pos: Vec3,
    pub color: [f32; 3],
    pub power: f32,
}

pub struct Loaded {
    pub lights: Vec<LightSite>,
    /// Everything at or above ground level; hidden while the player is in a tunnel.
    pub entity: Entity,
    /// Below-ground geometry (tunnels, cuttings).
    pub under: Option<Entity>,
}

enum TileState {
    Requested(Handle<TileBlob>),
    Ready,
}

#[derive(Resource)]
pub struct City {
    pub spawn: Vec2,
    names: Vec<String>,
    t0: IVec2,
    tsize: IVec2,
    mask: Vec<u8>,
    tiles: HashMap<IVec2, TileState>,
    pub drive: Graph,
    drive_cells: HashMap<IVec2, Vec<u32>>,
    pub walk: Graph,
    walk_nodes: HashMap<(i32, i32), u32>,
    pub blds: Vec<Bld>,
    pub rivers: Vec<(f32, Vec<Vec2>)>,
    areas: Vec<Area>,
    src: HashMap<IVec2, CellSrc>,
    pub cells: HashMap<IVec2, Loaded>,
    pub collected: HashSet<IVec2>,
    /// Id of the way currently being ingested (tags its road paint).
    cur_way: u32,
}

pub fn cell_of(p: Vec2) -> IVec2 {
    IVec2::new((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}
fn tile_of_cell(c: IVec2) -> IVec2 {
    IVec2::new(c.x.div_euclid(8), c.y.div_euclid(8))
}

// ---- binary reader --------------------------------------------------------
struct Rd<'a> {
    d: &'a [u8],
    o: usize,
    origin: Vec2,
}
impl Rd<'_> {
    fn ok(&self, n: usize) -> bool {
        self.o + n <= self.d.len()
    }
    fn u8(&mut self) -> u8 {
        self.o += 1;
        self.d[self.o - 1]
    }
    fn u16(&mut self) -> u16 {
        self.o += 2;
        u16::from_le_bytes([self.d[self.o - 2], self.d[self.o - 1]])
    }
    fn u32(&mut self) -> u32 {
        self.o += 4;
        u32::from_le_bytes(self.d[self.o - 4..self.o].try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    /// Tile-relative point stored as decimetres.
    fn v(&mut self) -> Vec2 {
        let x = self.u16() as i16 as f32 * 0.1;
        let z = self.u16() as i16 as f32 * 0.1;
        self.origin + Vec2::new(x, z)
    }
    fn poly(&mut self) -> Vec<Vec2> {
        let n = self.u16() as usize;
        if !self.ok(n * 4) {
            return vec![];
        }
        (0..n).map(|_| self.v()).collect()
    }
    fn idx(&mut self) -> Vec<u16> {
        let n = self.u16() as usize;
        if !self.ok(n * 2) {
            return vec![];
        }
        (0..n).map(|_| self.u16()).collect()
    }
}

fn up_quad(mut q: Quad3) -> Quad3 {
    let mut a = 0.0;
    for i in 0..4 {
        let (p, n) = (q[i], q[(i + 1) % 4]);
        a += p.x * n.z - n.x * p.z;
    }
    if a > 0.0 {
        q.swap(1, 3);
    }
    q
}

fn bounds(pts: &[Vec2]) -> (Vec2, Vec2) {
    (pts.iter().fold(Vec2::MAX, |a, p| a.min(*p)), pts.iter().fold(Vec2::MIN, |a, p| a.max(*p)))
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Nearest boundary point, squared distance to it, and whether `p` is inside.
fn nearest_on_poly(p: Vec2, pts: &[Vec2]) -> (Vec2, f32, bool) {
    let mut best = (pts[0], f32::MAX);
    let mut inside = false;
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let q = a + ab * t;
        let d2 = q.distance_squared(p);
        if d2 < best.1 {
            best = (q, d2);
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    (best.0, best.1, inside)
}

impl City {
    pub fn load() -> City {
        let mut r = Rd { d: CORE, o: 4, origin: Vec2::ZERO };
        let spawn = Vec2::new(r.f32(), r.f32());
        let nn = r.u16();
        let mut names = vec![];
        for _ in 0..nn {
            let n = r.u8() as usize;
            r.o += n;
            names.push(String::from_utf8_lossy(&r.d[r.o - n..r.o]).into_owned());
        }
        let t0x = r.u32() as i32;
        let t0 = IVec2::new(t0x, r.u32() as i32);
        let tw = r.u16() as i32;
        let tsize = IVec2::new(tw, r.u16() as i32);
        let mask = r.d[r.o..r.o + (tsize.x * tsize.y) as usize].to_vec();
        r.o += mask.len();
        let nodes = r.u32() as usize;
        let mut drive = Graph { edges: vec![], adj: vec![vec![]; nodes], node_pos: vec![Vec2::ZERO; nodes] };
        let mut drive_cells: HashMap<IVec2, Vec<u32>> = HashMap::default();
        let ne = r.u32();
        for i in 0..ne {
            let a = r.u32();
            let b = r.u32();
            let oneway = r.u8() != 0;
            let width = r.u8() as f32 * 0.1;
            let lanes = r.u8();
            let lclass = r.u8();
            let limit = r.u8();
            let name = r.u16();
            let np = r.u16();
            let pts: Vec<Vec2> = (0..np)
                .map(|_| {
                    let x = r.f32();
                    Vec2::new(x, r.f32())
                })
                .collect();
            drive.adj[a as usize].push(i);
            if b != a {
                drive.adj[b as usize].push(i);
            }
            drive.node_pos[a as usize] = pts[0];
            drive.node_pos[b as usize] = pts[pts.len() - 1];
            let class = lclass.min(2);
            let mut last = IVec2::MAX;
            for w in pts.windows(2) {
                let n = (w[0].distance(w[1]) / 20.0).ceil().max(1.0) as i32;
                for k in 0..=n {
                    let c = cell_of(w[0].lerp(w[1], k as f32 / n as f32));
                    if c != last {
                        last = c;
                        let list = drive_cells.entry(c).or_default();
                        if list.last() != Some(&i) {
                            list.push(i);
                        }
                    }
                }
            }
            drive.edges.push(Edge { a, b, oneway, width, lanes, name, class, limit, pts });
        }
        let mut rivers = vec![];
        let nr = r.u32();
        for _ in 0..nr {
            let w = r.u16() as f32 * 0.1;
            let np = r.u16();
            let pts = (0..np)
                .map(|_| {
                    let x = r.f32();
                    Vec2::new(x, r.f32())
                })
                .collect();
            rivers.push((w, pts));
        }
        City {
            spawn,
            names,
            t0,
            tsize,
            mask,
            tiles: HashMap::default(),
            drive,
            drive_cells,
            walk: Graph::default(),
            walk_nodes: HashMap::default(),
            blds: vec![],
            rivers,
            areas: vec![],
            src: HashMap::default(),
            cells: HashMap::default(),
            collected: HashSet::default(),
            cur_way: 0,
        }
    }

    pub fn src(&self, c: IVec2) -> Option<&CellSrc> {
        self.src.get(&c)
    }
    pub fn drive_in(&self, c: IVec2) -> &[u32] {
        self.drive_cells.get(&c).map_or(&[], |v| v.as_slice())
    }
    pub fn has_job(&self, c: IVec2) -> bool {
        hash2(c.x, c.y, 77) % 5 == 0 && self.src(c).is_some_and(|s| s.poi.is_some())
    }

    /// Name of the nearest named street.
    pub fn street(&self, p: Vec2) -> &str {
        let c = cell_of(p);
        let mut best: Option<(f32, u16)> = None;
        for dz in -1..=1 {
            for dx in -1..=1 {
                for &e in self.drive_in(c + IVec2::new(dx, dz)) {
                    let ed = &self.drive.edges[e as usize];
                    if (ed.name as usize) < self.names.len() {
                        for w in ed.pts.windows(2) {
                            let d = seg_dist(p, w[0], w[1]);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, ed.name));
                            }
                        }
                    }
                }
            }
        }
        best.map_or("", |b| self.names[b.1 as usize].as_str())
    }

    fn tile_exists(&self, t: IVec2) -> bool {
        let l = t - self.t0;
        l.x >= 0 && l.y >= 0 && l.x < self.tsize.x && l.y < self.tsize.y && self.mask[(l.y * self.tsize.x + l.x) as usize] != 0
    }
    fn tile_ready(&self, t: IVec2) -> bool {
        !self.tile_exists(t) || matches!(self.tiles.get(&t), Some(TileState::Ready))
    }
    /// True once every tile that can contribute geometry to this cell has arrived.
    fn cell_ready(&self, c: IVec2) -> bool {
        let t = tile_of_cell(c);
        (-1..=1).all(|dz| (-1..=1).all(|dx| self.tile_ready(t + IVec2::new(dx, dz))))
    }
    pub fn area_ready(&self, p: Vec2) -> bool {
        self.cells.contains_key(&cell_of(p))
    }

    /// Height of the surface under `p` for something currently at height `cur`:
    /// a bridge deck or ramp if one is within stepping range, otherwise the ground.
    pub fn ground_y(&self, p: Vec2, cur: f32) -> f32 {
        let Some(c) = self.src(cell_of(p)) else { return 0.0 };
        let (mut best, mut low) = (0.0f32, 0.0f32);
        for (a, b, hw, ya, yb) in &c.decks {
            let ab = *b - *a;
            let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            if (*a + ab * t).distance_squared(p) <= hw * hw {
                let h = ya + (yb - ya) * t;
                if (h - cur).abs() < 2.2 {
                    best = best.max(h);
                    low = low.min(h);
                }
            }
        }
        // a cutting or tunnel floor within reach wins over the ground it is dug into
        if low < -0.05 { low } else { best }
    }

    /// Height to place a vehicle heading `dir` at `p`: the deck running its way, if any.
    pub fn surface_for(&self, p: Vec2, dir: Vec2) -> f32 {
        let Some(c) = self.src(cell_of(p)) else { return 0.0 };
        let mut best: Option<(f32, f32)> = None;
        for (a, b, hw, ya, yb) in &c.decks {
            let ab = *b - *a;
            let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let d = (*a + ab * t).distance(p);
            if d <= *hw && ab.normalize_or_zero().dot(dir).abs() > 0.9 && best.is_none_or(|x| d < x.0) {
                best = Some((d, ya + (yb - ya) * t));
            }
        }
        best.map_or(0.0, |x| x.1)
    }

    /// Posted limit (km/h) of the road nearest to `p`.
    pub fn limit_at(&self, p: Vec2) -> u32 {
        self.nearest_drive(p).map_or(50, |(e, _)| self.drive.edges[e as usize].limit as u32)
    }

    pub fn own_blds(&self, c: IVec2) -> &[u32] {
        self.src(c).map_or(&[], |s| s.own.as_slice())
    }

    /// Closest point on the drivable network (edge id, point).
    pub fn nearest_drive(&self, p: Vec2) -> Option<(u32, Vec2)> {
        let c = cell_of(p);
        for ring in [1, 3, 7] {
            let mut best: Option<(f32, u32, Vec2)> = None;
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    for &e in self.drive_in(c + IVec2::new(dx, dz)) {
                        for w in self.drive.edges[e as usize].pts.windows(2) {
                            let ab = w[1] - w[0];
                            let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                            let q = w[0] + ab * t;
                            let d = q.distance_squared(p);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, e, q));
                            }
                        }
                    }
                }
            }
            if let Some(b) = best {
                return Some((b.1, b.2));
            }
        }
        None
    }

    /// Fastest legal driving route between two points, as a polyline.
    pub fn route(&self, from: Vec2, to: Vec2) -> Vec<Vec2> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let (Some((e0, _)), Some((e1, q1))) = (self.nearest_drive(from), self.nearest_drive(to)) else {
            return vec![];
        };
        let g = &self.drive;
        let speed = |e: &Edge| [21.0, 15.0, 9.0][e.class as usize];
        let n = g.adj.len();
        let mut dist = vec![f32::MAX; n];
        let mut prev: Vec<(u32, bool)> = vec![(u32::MAX, true); n];
        let mut heap: BinaryHeap<(Reverse<u32>, u32)> = BinaryHeap::new();
        let key = |d: f32| Reverse((d * 100.0) as u32);
        let s = &g.edges[e0 as usize];
        let mut seed = |node: u32, heap: &mut BinaryHeap<(Reverse<u32>, u32)>, dist: &mut Vec<f32>| {
            let d = g.node_pos[node as usize].distance(from) / 9.0;
            if d < dist[node as usize] {
                dist[node as usize] = d;
                heap.push((key(d), node));
            }
        };
        seed(s.b, &mut heap, &mut dist);
        if !s.oneway {
            seed(s.a, &mut heap, &mut dist);
        }
        let t = &g.edges[e1 as usize];
        let goals = if t.oneway { vec![t.a] } else { vec![t.a, t.b] };
        let mut goal = u32::MAX;
        while let Some((Reverse(k), node)) = heap.pop() {
            if k as f32 > dist[node as usize] * 100.0 + 2.0 {
                continue;
            }
            if goals.contains(&node) {
                goal = node;
                break;
            }
            for &ei in &g.adj[node as usize] {
                let ed = &g.edges[ei as usize];
                let mut len = 0.0;
                for w in ed.pts.windows(2) {
                    len += w[0].distance(w[1]);
                }
                let cost = len / speed(ed);
                for (ok, next, fwd) in [(ed.a == node, ed.b, true), (ed.b == node && !ed.oneway, ed.a, false)] {
                    if ok && dist[node as usize] + cost < dist[next as usize] {
                        dist[next as usize] = dist[node as usize] + cost;
                        prev[next as usize] = (ei, fwd);
                        heap.push((key(dist[next as usize]), next));
                    }
                }
            }
        }
        if goal == u32::MAX {
            return vec![from, to];
        }
        // walk back from the goal collecting edge polylines
        let mut chain: Vec<(u32, bool)> = vec![];
        let mut node = goal;
        while prev[node as usize].0 != u32::MAX && chain.len() < 4000 {
            let (ei, fwd) = prev[node as usize];
            chain.push((ei, fwd));
            let ed = &g.edges[ei as usize];
            node = if fwd { ed.a } else { ed.b };
        }
        let mut out = vec![from];
        for (ei, fwd) in chain.into_iter().rev() {
            let ed = &g.edges[ei as usize];
            for k in 0..ed.pts.len() {
                out.push(ed.pt(fwd, k));
            }
        }
        out.push(q1);
        out.push(to);
        out.dedup_by(|a, b| a.distance_squared(*b) < 0.01);
        out
    }

    /// Push a circle out of buildings and trees. Returns the corrected position and whether it hit.
    pub fn push_out(&self, p: Vec2, r: f32) -> (Vec2, bool) {
        let lo = self.t0.as_vec2() * TILE + 4.0;
        let hi = (self.t0 + self.tsize).as_vec2() * TILE - 4.0;
        let mut p = p.clamp(lo, hi);
        let mut hit = false;
        let Some(cell) = self.src(cell_of(p)) else { return (p, false) };
        for &bi in &cell.col {
            let b = &self.blds[bi as usize];
            if p.x < b.min.x - r || p.x > b.max.x + r || p.y < b.min.y - r || p.y > b.max.y + r {
                continue;
            }
            let (near, d2, inside) = nearest_on_poly(p, &b.pts);
            if inside {
                let dir = (near - p).normalize_or(Vec2::X);
                p = near + dir * r;
                hit = true;
            } else if d2 < r * r {
                let d = d2.sqrt().max(1e-4);
                p = near + (p - near) / d * r;
                hit = true;
            }
        }
        for (t, _) in &cell.trees {
            let d = p - *t;
            let rr = r + 0.3;
            if d.length_squared() < rr * rr {
                p = *t + d.normalize_or(Vec2::X) * rr;
                hit = true;
            }
        }
        for (t, ir) in &cell.rounds {
            let d = p - *t;
            let rr = r + ir;
            if d.length_squared() < rr * rr {
                p = *t + d.normalize_or(Vec2::X) * rr;
                hit = true;
            }
        }
        (p, hit)
    }

    pub fn solid(&self, p: Vec2) -> bool {
        self.in_building(p, 0.0)
    }

    fn in_building(&self, p: Vec2, margin: f32) -> bool {
        self.src(cell_of(p)).is_some_and(|c| {
            c.col.iter().any(|&bi| {
                let b = &self.blds[bi as usize];
                if p.x < b.min.x - margin || p.x > b.max.x + margin || p.y < b.min.y - margin || p.y > b.max.y + margin {
                    return false;
                }
                let (_, d2, inside) = nearest_on_poly(p, &b.pts);
                inside || d2 < margin * margin
            })
        })
    }

    fn near_way(&self, p: Vec2, margin: f32) -> bool {
        self.src(cell_of(p))
            .is_some_and(|c| c.segs.iter().any(|(a, b, hw)| seg_dist(p, *a, *b) < hw + margin))
    }

    // ---- tile ingestion ---------------------------------------------------
    fn add_tri(&mut self, m: usize, t: [Vec2; 3]) {
        // big triangles are split so the surface can bend over hills
        let l = [t[0].distance(t[1]), t[1].distance(t[2]), t[2].distance(t[0])];
        let k = if l[0] >= l[1] && l[0] >= l[2] { 0 } else if l[1] >= l[2] { 1 } else { 2 };
        if l[k] > 14.0 {
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            let mid = (a + b) * 0.5;
            self.add_tri(m, [a, mid, c]);
            self.add_tri(m, [mid, b, c]);
            return;
        }
        let c = cell_of((t[0] + t[1] + t[2]) / 3.0);
        let t = if (t[1] - t[0]).perp_dot(t[2] - t[0]) > 0.0 { [t[0], t[2], t[1]] } else { t };
        self.src.entry(c).or_default().tris[m].push(t);
    }

    fn strip(&mut self, m: usize, pts: &[Vec2], width: f32) {
        let hw = width * 0.5;
        let n = pts.len();
        for i in 0..n - 1 {
            let (a, b) = (pts[i], pts[i + 1]);
            let d = (b - a).normalize_or_zero();
            if d == Vec2::ZERO {
                continue;
            }
            let nn = Vec2::new(-d.y, d.x) * hw;
            let pieces = (a.distance(b) / 8.0).ceil().max(1.0) as i32;
            for k in 0..pieces {
                let (p0, p1) = (a.lerp(b, k as f32 / pieces as f32), a.lerp(b, (k + 1) as f32 / pieces as f32));
                if hw > 4.0 {
                    self.add_tri(m, [p0 + nn, p0, p1]);
                    self.add_tri(m, [p0 + nn, p1, p1 + nn]);
                    self.add_tri(m, [p0, p0 - nn, p1 - nn]);
                    self.add_tri(m, [p0, p1 - nn, p1]);
                } else {
                    self.add_tri(m, [p0 + nn, p0 - nn, p1 - nn]);
                    self.add_tri(m, [p0 + nn, p1 - nn, p1 + nn]);
                }
            }
            // round off ends and noticeable bends
            let bend = i + 2 < n && d.dot((pts[i + 2] - b).normalize_or_zero()) < 0.995;
            for (p, cap) in [(a, i == 0), (b, i == n - 2 || bend)] {
                if cap {
                    let ring: Vec<Vec2> = (0..8).map(|k| p + Vec2::from_angle(k as f32 * 0.7854) * hw).collect();
                    for k in 1..7 {
                        self.add_tri(m, [ring[0], ring[k], ring[k + 1]]);
                    }
                }
            }
        }
    }

    fn add_quad(&mut self, q: Quad3, tile: u8, shade: u8) {
        let c = cell_of(((q[0] + q[1] + q[2] + q[3]) * 0.25).xz());
        let id = self.cur_way;
        self.src.entry(c).or_default().quads.push((up_quad(q), tile, shade, id));
    }

    fn add_equad(&mut self, q: Quad3, tile: u32, col: [f32; 4]) {
        let c = cell_of(((q[0] + q[1] + q[2] + q[3]) * 0.25).xz());
        self.src.entry(c).or_default().equads.push((q, tile, col));
    }

    /// Road surface that follows per-point heights: bridge decks and their approach ramps.
    /// Road surface that follows per-point heights: bridge decks, ramps, cuttings and tunnels.
    /// Side structures are only recorded here; they are built per cell once the neighbouring
    /// roads are known, so nothing gets walled off where two raised roads merge.
    fn strip_elev(&mut self, m: usize, pts: &[Vec2], ys: &[f32], width: f32, bridge: bool, tunnel: bool) {
        let hw = width * 0.5;
        let n = pts.len();
        let up = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
        let mut run = 0.0;
        for i in 0..n - 1 {
            let (a, b) = (pts[i], pts[i + 1]);
            let (ya, yb) = (ys[i], ys[i + 1]);
            let len = a.distance(b);
            if len < 0.01 {
                continue;
            }
            let d = (b - a) / len;
            let nn = Vec2::new(-d.y, d.x) * hw;
            let sy = 0.3;
            let cen = cell_of((a + b) * 0.5);
            let s = self.src.entry(cen).or_default();
            // wound so the surface faces up (same convention as the flat ground layers)
            s.deck[m].push([up(a + nn, ya + sy), up(b - nn, yb + sy), up(a - nn, ya + sy)]);
            s.deck[m].push([up(a + nn, ya + sy), up(b + nn, yb + sy), up(b - nn, yb + sy)]);
            let ring: Vec<Vec3> = (0..8).map(|k| up(b + Vec2::from_angle(-(k as f32) * 0.7854) * hw, yb + sy)).collect();
            for k in 1..7 {
                s.deck[m].push([ring[0], ring[k], ring[k + 1]]);
            }
            // physics: the deck can be driven on
            let (lo, hi) = (cell_of(a.min(b) - (hw + 2.0)), cell_of(a.max(b) + (hw + 2.0)));
            for cz in lo.y..=hi.y.min(lo.y + 4) {
                for cx in lo.x..=hi.x.min(lo.x + 4) {
                    self.src.entry(IVec2::new(cx, cz)).or_default().decks.push((a, b, hw + 0.4, ya, yb));
                }
            }
            let (top, low) = (ya.max(yb), ya.min(yb));
            if top < 0.3 && low > -0.25 {
                continue;
            }
            let covered = tunnel && top < -3.0;
            let kind = if low < -0.25 { if covered { 3 } else { 2 } } else if bridge { 0 } else { 1 };
            for side in [1.0f32, -1.0] {
                let (ea, eb) = (a + nn * side, b + nn * side);
                let e = if side > 0.0 { (ea, eb, ya, yb, kind, m == 0) } else { (eb, ea, yb, ya, kind, m == 0) };
                self.src.entry(cen).or_default().edges.push(e);
            }
            match kind {
                0 if low > 1.5 => {
                    // the deck darkens whatever lies beneath it, with a softer band spilling past the edges
                    for (wide, alpha, sy2) in [(hw + 0.4, 0.5, 0.2), (hw + 3.2, 0.25, 0.215)] {
                        let w2 = d.perp() * wide;
                        let v = |p: Vec2| Vec3::new(p.x, sy2, p.y);
                        let q = up_quad([v(a + w2), v(a - w2), v(b - w2), v(b + w2)]);
                        self.src.entry(cen).or_default().shades.push((q, alpha));
                    }
                    run += len;
                    if run > 24.0 && low > 3.0 {
                        run = 0.0;
                        let c = (a + b) * 0.5;
                        let y = (ya + yb) * 0.5 - 0.9;
                        for side in [1.0f32, -1.0] {
                            let pc = c + nn * side * 0.8;
                            let s = self.src.entry(cell_of(pc)).or_default();
                            s.boxes.push((Vec3::new(pc.x - 0.5, 0.0, pc.y - 0.5), Vec3::new(pc.x + 0.5, y, pc.y + 0.5), T_METAL, [0.3, 0.3, 0.36, 1.0]));
                        }
                    }
                }
                2 => {
                    // concrete apron around the cutting hides the stepped edge of the hole in the ground
                    for side in [1.0f32, -1.0] {
                        let (ea, eb) = (a + nn * side, b + nn * side);
                        let (o0, o1) = (a + nn * side * (1.0 + 4.0 / hw), b + nn * side * (1.0 + 4.0 / hw));
                        self.add_tri(M_CONCRETE, [ea, o0, o1]);
                        self.add_tri(M_CONCRETE, [ea, o1, eb]);
                    }
                    let (lo, hi) = (cell_of(a.min(b) - (hw + 4.0)), cell_of(a.max(b) + (hw + 4.0)));
                    for cz in lo.y..=hi.y.min(lo.y + 4) {
                        for cx in lo.x..=hi.x.min(lo.x + 4) {
                            self.src.entry(IVec2::new(cx, cz)).or_default().holes.push((a, b, hw));
                        }
                    }
                }
                3 => {
                    run += len;
                    if run > 16.0 {
                        run = 0.0;
                        let c = (a + b) * 0.5;
                        self.src.entry(cell_of(c)).or_default().lights.push((c, 4, (ya + yb) * 0.5));
                    }
                }
                _ => {}
            }
        }
    }

    /// Continuous painted line following a road at a sideways offset.
    fn line(&mut self, pts: &[Vec2], ys: Option<&[f32]>, off: f32, width: f32, shade: u8) {
        for (i, w) in pts.windows(2).enumerate() {
            let l = w[0].distance(w[1]);
            if l < 0.05 {
                continue;
            }
            let d = (w[1] - w[0]) / l;
            let nn = Vec2::new(-d.y, d.x);
            let (ya, yb) = ys.map_or((0.0, 0.0), |y| (y[i], y[i + 1]));
            let h = nn * width * 0.5;
            let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
            // short pieces (slightly overlapped) so the line follows bends and slopes
            let pieces = (l / 8.0).ceil().max(1.0) as i32;
            for k in 0..pieces {
                let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
                let (p, e) = (w[0].lerp(w[1], t0) - d * 0.12 + nn * off, w[0].lerp(w[1], t1) + d * 0.12 + nn * off);
                let (y0, y1) = (ya + (yb - ya) * t0, ya + (yb - ya) * t1);
                self.add_quad([v(p + h, y0), v(p - h, y0), v(e - h, y1), v(e + h, y1)], 0, shade);
            }
        }
    }

    fn dashes(&mut self, pts: &[Vec2], ys: Option<&[f32]>, off: f32, period: f32, len: f32, width: f32, shade: u8) {
        let mut t = 0.0;
        for (i, w) in pts.windows(2).enumerate() {
            let l = w[0].distance(w[1]);
            if l < 0.1 {
                continue;
            }
            let d = (w[1] - w[0]) / l;
            let nn = Vec2::new(-d.y, d.x);
            let y_at = |s: f32| ys.map_or(0.0, |y| y[i] + (y[i + 1] - y[i]) * (s / l).clamp(0.0, 1.0));
            while t + len <= l {
                let p = w[0] + d * t + nn * off;
                let e = p + d * len;
                let h = nn * width * 0.5;
                let (y0, y1) = (y_at(t), y_at(t + len));
                self.add_quad(
                    [
                        Vec3::new((p + h).x, y0, (p + h).y),
                        Vec3::new((p - h).x, y0, (p - h).y),
                        Vec3::new((e - h).x, y1, (e - h).y),
                        Vec3::new((e + h).x, y1, (e + h).y),
                    ],
                    0,
                    shade,
                );
                t += period;
            }
            t -= l;
        }
    }

    fn lights_along(&mut self, pts: &[Vec2], ys: Option<&[f32]>, spacing: f32, off: f32, kind: u8) {
        let seed = hash2((pts[0].x * 10.0) as i32, (pts[0].y * 10.0) as i32, 3);
        let mut t = (seed % 1000) as f32 / 1000.0 * spacing;
        let mut side = if seed & 1024 == 0 { 1.0 } else { -1.0 };
        for (i, w) in pts.windows(2).enumerate() {
            let l = w[0].distance(w[1]);
            if l < 0.1 {
                continue;
            }
            let d = (w[1] - w[0]) / l;
            while t <= l {
                let y = ys.map_or(0.0, |y| y[i] + (y[i + 1] - y[i]) * (t / l));
                // on a deck the lamp stands on the edge of the slab, not in mid-air beside it
                let o = if y > 0.4 { off - 1.5 } else { off };
                let p = w[0] + d * t + Vec2::new(-d.y, d.x) * o * side;
                self.src.entry(cell_of(p)).or_default().lights.push((p, kind, y));
                side = -side;
                t += spacing;
            }
            t -= l;
        }
    }

    fn register_segs(&mut self, pts: &[Vec2], hw: f32) {
        for w in pts.windows(2) {
            let lo = cell_of(w[0].min(w[1]) - (hw + 4.0));
            let hi = cell_of(w[0].max(w[1]) + (hw + 4.0));
            for cz in lo.y..=hi.y.min(lo.y + 6) {
                for cx in lo.x..=hi.x.min(lo.x + 6) {
                    self.src.entry(IVec2::new(cx, cz)).or_default().segs.push((w[0], w[1], hw));
                }
            }
        }
    }

    fn walk_node(&mut self, p: Vec2) -> u32 {
        let key = ((p.x * 4.0).round() as i32, (p.y * 4.0).round() as i32);
        if let Some(&n) = self.walk_nodes.get(&key) {
            return n;
        }
        let n = self.walk.adj.len() as u32;
        self.walk.adj.push(vec![]);
        self.walk_nodes.insert(key, n);
        n
    }

    fn ingest(&mut self, tile: IVec2, data: &[u8]) {
        if data.len() < 6 || &data[0..4] != b"VLT2" {
            return;
        }
        let mut r = Rd { d: data, o: 4, origin: tile.as_vec2() * TILE };
        for _ in 0..r.u16() {
            if !r.ok(9) {
                return;
            }
            let kind = r.u8();
            let width = r.u16() as f32 * 0.1;
            let (flags, lanes, lclass, mark) = (r.u8(), r.u8().max(1), r.u8(), r.u8());
            let pts = r.poly();
            let ys: Vec<f32> = if flags & 8 != 0 && r.ok(pts.len()) {
                (0..pts.len()).map(|_| r.u8() as i8 as f32 * 0.1).collect()
            } else {
                vec![]
            };
            if pts.len() < 2 {
                continue;
            }
            let elevated = ys.len() == pts.len() && ys.iter().any(|y| y.abs() > 0.15);
            self.cur_way += 1;
            if kind == 0 {
                // remember the carriageway so paint from other roads can be kept off it
                let id = self.cur_way;
                for (i, w) in pts.windows(2).enumerate() {
                    let y = if elevated { (ys[i] + ys[i + 1]) * 0.5 } else { 0.0 };
                    let (lo, hi) = (cell_of(w[0].min(w[1]) - width), cell_of(w[0].max(w[1]) + width));
                    for cz in lo.y..=hi.y.min(lo.y + 6) {
                        for cx in lo.x..=hi.x.min(lo.x + 6) {
                            self.src.entry(IVec2::new(cx, cz)).or_default().roads.push((w[0], w[1], width * 0.5, id, y));
                        }
                    }
                }
            }
            let tunnel = flags & 16 != 0;
            let ysl = elevated.then_some(ys.as_slice());
            let bridge = flags & 4 != 0;
            match kind {
                0 => {
                    if elevated {
                        self.strip_elev(0, &pts, &ys, width + 1.6, bridge, tunnel);
                        // approach ramps still need ground underneath their low end
                        if ys.iter().any(|y| *y < 0.5) && ys.iter().all(|y| *y > -0.1) {
                            self.strip(M_PAVING, &pts, width + 2.2);
                        }
                    } else {
                        self.strip(M_PAVING, &pts, width + 2.2); // kerb line under the carriageway
                        self.strip(M_ASPHALT, &pts, width + 0.6);
                    }
                    if !bridge && !tunnel {
                        self.register_segs(&pts, width * 0.5 + 1.0);
                    }
                    let lw = width / lanes as f32;
                    match mark {
                        1 => self.dashes(&pts, ysl, 0.0, 9.0, 3.0, 0.2, 120),
                        2 => {
                            self.line(&pts, ysl, 0.16, 0.13, 150);
                            self.line(&pts, ysl, -0.16, 0.13, 150);
                            self.dashes(&pts, ysl, width / 4.0, 9.0, 3.0, 0.2, 120);
                            self.dashes(&pts, ysl, -width / 4.0, 9.0, 3.0, 0.2, 120);
                        }
                        3 => {
                            for l in 1..lanes {
                                self.dashes(&pts, ysl, -width / 2.0 + l as f32 * lw, 9.0, 3.0, 0.2, 120);
                            }
                        }
                        _ => {}
                    }
                    if lclass <= 1 && !elevated {
                        // solid edge lines on main roads
                        self.line(&pts, None, width / 2.0 - 0.1, 0.14, 110);
                        self.line(&pts, None, -width / 2.0 + 0.1, 0.14, 110);
                    }
                    if !tunnel {
                        self.lights_along(&pts, ysl, 34.0, width / 2.0 + 1.9, lclass.min(2));
                    }
                }
                1 => {
                    self.strip(M_ASPHALT, &pts, width);
                    self.register_segs(&pts, width * 0.5);
                }
                4 => {
                    self.strip(M_WATER, &pts, width);
                    self.register_segs(&pts, width * 0.5 + 1.0);
                }
                5 => {
                    // railway: ballast, sleepers, two rails
                    self.strip(M_CONCRETE, &pts, width);
                    self.register_segs(&pts, width * 0.5 + 1.5);
                    self.dashes(&pts, None, 0.0, 1.5, 0.32, 2.5, 55);
                    self.dashes(&pts, None, 0.72, 4.0, 4.0, 0.14, 230);
                    self.dashes(&pts, None, -0.72, 4.0, 4.0, 0.14, 230);
                }
                _ => {
                    if kind == 3 {
                        for w in pts.windows(2) {
                            let l = w[0].distance(w[1]);
                            let n = (l / 4.0).round().max(1.0) as i32;
                            let d = (w[1] - w[0]) / l.max(0.01);
                            let nn = Vec2::new(-d.y, d.x) * 1.6;
                            for k in 0..n {
                                let p0 = w[0] + d * (l * k as f32 / n as f32);
                                let p1 = w[0] + d * (l * (k + 1) as f32 / n as f32);
                                let v = |p: Vec2| Vec3::new(p.x, 0.0, p.y);
                                self.add_quad([v(p0 + nn), v(p0 - nn), v(p1 - nn), v(p1 + nn)], 3, 255);
                            }
                        }
                    } else if elevated {
                        self.strip_elev(1, &pts, &ys, width + 0.6, !tunnel, tunnel);
                    } else {
                        self.strip(M_PAVING, &pts, width);
                        self.register_segs(&pts, width * 0.5);
                        self.lights_along(&pts, None, 80.0, width / 2.0 + 0.7, 3);
                    }
                    if flags & 2 != 0 {
                        let (a, b) = (self.walk_node(pts[0]), self.walk_node(pts[pts.len() - 1]));
                        let id = self.walk.edges.len() as u32;
                        self.walk.adj[a as usize].push(id);
                        if a != b {
                            self.walk.adj[b as usize].push(id);
                        }
                        let mut last = IVec2::MAX;
                        for p in &pts {
                            let c = cell_of(*p);
                            if c != last {
                                last = c;
                                let s = self.src.entry(c).or_default();
                                s.walk.push(id);
                                if s.poi.is_none() && kind == 2 && !elevated {
                                    s.poi = Some(*p);
                                }
                            }
                        }
                        self.walk.edges.push(Edge { a, b, oneway: false, width, lanes: 1, name: u16::MAX, class: 2, limit: 0, pts });
                    }
                }
            }
        }
        for _ in 0..r.u16() {
            if !r.ok(4) {
                return;
            }
            let (lv, kind) = (r.u8(), r.u8());
            let pts = r.poly();
            let tris = r.idx();
            if pts.len() < 3 {
                continue;
            }
            let (min, max) = bounds(&pts);
            let id = self.blds.len() as u32;
            let cen = pts.iter().sum::<Vec2>() / pts.len() as f32;
            self.src.entry(cell_of(cen)).or_default().own.push(id);
            let (lo, hi) = (cell_of(min - 3.0), cell_of(max + 3.0));
            for cz in lo.y..=hi.y {
                for cx in lo.x..=hi.x {
                    self.src.entry(IVec2::new(cx, cz)).or_default().col.push(id);
                }
            }
            self.blds.push(Bld { lv, kind, pts, tris, min, max });
        }
        for _ in 0..r.u16() {
            if !r.ok(3) {
                return;
            }
            let kind = r.u8();
            let pts = r.poly();
            let tris = r.idx();
            if kind == 5 && pts.len() == 2 {
                // roundabout island
                let (c, rad) = (pts[0], pts[0].distance(pts[1]));
                self.src.entry(cell_of(c)).or_default().islands.push((c, rad));
                let (lo, hi) = (cell_of(c - rad - 2.0), cell_of(c + rad + 2.0));
                for cz in lo.y..=hi.y {
                    for cx in lo.x..=hi.x {
                        self.src.entry(IVec2::new(cx, cz)).or_default().rounds.push((c, rad));
                    }
                }
                continue;
            }
            if pts.len() < 3 {
                continue;
            }
            if kind <= 2 {
                let m = if kind == 0 { M_WATER } else { M_CONCRETE };
                for t in tris.chunks_exact(3) {
                    self.add_tri(m, [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]]);
                }
            }
            let (min, max) = bounds(&pts);
            let id = self.areas.len() as u32;
            let (lo, hi) = (cell_of(min), cell_of(max));
            for cz in lo.y..=hi.y.min(lo.y + 9) {
                for cx in lo.x..=hi.x.min(lo.x + 9) {
                    self.src.entry(IVec2::new(cx, cz)).or_default().areas.push(id);
                }
            }
            self.areas.push(Area { kind, pts, min, max });
        }
        if !r.ok(2) {
            return;
        }
        for _ in 0..r.u16() {
            if !r.ok(5) {
                return;
            }
            let kind = r.u8();
            let p = r.v();
            let s = self.src.entry(cell_of(p)).or_default();
            if kind >= 10 {
                s.cams.push((p, (kind as u32 - 10) * 10));
            } else if kind == 2 {
                s.trees.push((p, (hash2((p.x * 7.0) as i32, (p.y * 7.0) as i32, 9) % 256) as u8));
            } else {
                s.props.push((p, kind));
            }
        }
    }

    /// Procedural dressing that needs the finished neighbourhood: trees and parked cars.
    fn scatter(&mut self, c: IVec2) {
        let mut rng = Rng::new(hash2(c.x, c.y, 1234));
        let o = c.as_vec2() * CELL;
        let area_ids = self.src(c).map(|s| s.areas.clone()).unwrap_or_default();
        let area_at = |city: &City, p: Vec2| -> Option<u8> {
            let mut best = None;
            for &ai in &area_ids {
                let a = &city.areas[ai as usize];
                if p.x >= a.min.x && p.x <= a.max.x && p.y >= a.min.y && p.y <= a.max.y && nearest_on_poly(p, &a.pts).2 {
                    if a.kind <= 2 {
                        return Some(a.kind);
                    }
                    best = Some(best.map_or(a.kind, |b: u8| b.min(a.kind)));
                }
            }
            best
        };
        let mut trees = vec![];
        for _ in 0..64 {
            let p = o + Vec2::new(rng.f(), rng.f()) * CELL;
            let keep = rng.f();
            let size = rng.int(256) as u8;
            let dens = match area_at(self, p) {
                Some(0..=2) => 0.0,
                Some(3) => 0.85,
                Some(_) => 0.3,
                None => 0.13,
            };
            if keep < dens && !self.near_way(p, 2.4) && !self.in_building(p, 2.6) {
                trees.push((p, size));
            }
        }
        let mut parked = vec![];
        for &ai in &area_ids {
            let a = &self.areas[ai as usize];
            if a.kind != 1 || cell_of((a.min + a.max) * 0.5) != c {
                continue;
            }
            let n = a.pts.len();
            let size = (a.max - a.min).x * (a.max - a.min).y;
            let want = ((size / 70.0).min(14.0) * rng.range(0.3, 0.8)) as usize;
            let long = (0..n).max_by(|&i, &j| {
                a.pts[i].distance(a.pts[(i + 1) % n]).total_cmp(&a.pts[j].distance(a.pts[(j + 1) % n]))
            });
            let e = long.map_or(Vec2::X, |i| a.pts[(i + 1) % n] - a.pts[i]);
            let ang = e.y.atan2(e.x) + std::f32::consts::FRAC_PI_2;
            let mut placed: Vec<Vec2> = vec![];
            for _ in 0..want * 8 {
                if placed.len() >= want {
                    break;
                }
                let p = a.min + Vec2::new(rng.f(), rng.f()) * (a.max - a.min);
                let (_, d2, inside) = nearest_on_poly(p, &a.pts);
                if !inside || d2 < 7.0 || placed.iter().any(|q| q.distance(p) < 5.6) {
                    continue;
                }
                if self.near_way(p, 1.4) || self.in_building(p, 3.0) {
                    continue;
                }
                placed.push(p);
                parked.push((p, ang + if rng.chance(0.5) { std::f32::consts::PI } else { 0.0 }));
            }
        }
        let s = self.src.entry(c).or_default();
        s.trees.extend(trees);
        s.parked = parked;
        s.scattered = true;
    }
}

// ---- building meshes ------------------------------------------------------
const LIGHT_COL: [[f32; 3]; 4] = [[0.5, 0.85, 1.0], [1.0, 0.42, 0.8], [1.0, 0.68, 0.38], [0.3, 1.0, 0.8]];

fn add_building(b: &Bld, id: u32, mb: &mut MB, roof: &mut Flat, glow: &mut MB, smb: &mut MB, lights: &mut Vec<LightSite>) {
    let mut rng = Rng::new(hash2(id as i32, (b.min.x * 3.0) as i32, 7));
    let h = b.height();
    // the building stands level at the height of the ground under its middle
    let ground = terrain(b.pts.iter().sum::<Vec2>() / b.pts.len() as f32);
    mb.lift = ground;
    let style = match (b.kind, b.lv) {
        (2, _) => 3,
        (1, _) => [0, 0, 4][rng.int(3) as usize],
        (0, 9..) => [1, 5, 4, 1, 5][rng.int(5) as usize],
        (0, 4..) => [5, 1, 2][rng.int(3) as usize],
        (0, _) => 2,
        _ => [1, 2, 5][rng.int(3) as usize],
    };
    let shops = b.kind == 1 || (b.kind == 0 && b.lv >= 5 && rng.chance(0.3));
    let shade = rng.range(0.75, 1.1);
    let col = [shade, shade, shade, 1.0];
    let rows = (h / 4.0).round().max(1.0) as i32;
    let rh = h / rows as f32;
    let n = b.pts.len();
    let parapet = b.kind != 2 && b.lv >= 2;
    let ph = if parapet { 0.7 } else { 0.0 };
    for i in 0..n {
        let (a, c) = (b.pts[i], b.pts[(i + 1) % n]);
        let len = a.distance(c);
        let cols = (len / 4.0).round().max(1.0) as i32;
        for k in 0..cols {
            let p0 = a.lerp(c, k as f32 / cols as f32);
            let p1 = a.lerp(c, (k + 1) as f32 / cols as f32);
            for row in 0..rows {
                let v = rng.int(4);
                let shop = row == 0 && shops && len > 5.0;
                let tile = if shop { T_SHOP + v } else { T_WALL + style * 4 + v };
                // walls darken toward the street, which gives tall blocks real depth
                let g = if shop { 1.0 } else { 0.55 + 0.45 * (row + 1) as f32 / rows as f32 };
                mb.wall(p0, p1, row as f32 * rh, (row + 1) as f32 * rh, tile, [col[0] * g, col[1] * g, col[2] * g, 1.0]);
            }
        }
        if len > 0.6 {
            let d = (c - a) / len;
            let out = Vec2::new(-d.y, d.x);
            // foundation skirt so no gap opens on the downhill side
            mb.wall(a, c, -9.0, 0.0, T_METAL, [0.2, 0.2, 0.24, 1.0]);
            // grounding shadow where the wall meets the street
            for (w, alpha, y) in [(1.3, 0.42, 0.19), (3.2, 0.2, 0.185)] {
                let v = |p: Vec2| Vec3::new(p.x, y, p.y);
                smb.quad(up_quad([v(a), v(a + out * w), v(c + out * w), v(c)]), T_WHITE, 0, [0.0, 0.0, 0.0, alpha]);
            }
            if shops && len > 5.0 {
                // light from the shop windows spilling over the pavement
                let k = rng.int(8) as usize;
                let warm = [1.0 * 0.7 + NEON[k][0] * 0.3, 0.8 * 0.7 + NEON[k][1] * 0.3, 0.55 * 0.7 + NEON[k][2] * 0.3];
                glow.flat_quad([a, a + out * 1.8, c + out * 1.8, c], 0.2, T_SOLID, rgb(warm, 0.04));
                glow.flat_quad([a, a + out * 3.6, c + out * 3.6, c], 0.21, T_SOLID, rgb(warm, 0.02));
            }
            if b.kind == 0 && b.lv >= 4 && len > 10.0 {
                // balcony ledges on every floor
                let lc = [0.5 * shade, 0.5 * shade, 0.58 * shade, 1.0];
                for row in 1..rows {
                    let y = row as f32 * rh;
                    let (la, lb) = (a + d * 0.8 + out * 0.9, c - d * 0.8 + out * 0.9);
                    mb.flat_quad([a + d * 0.8, la, lb, c - d * 0.8], y + 0.95, T_METAL, lc);
                    mb.wall(la, lb, y, y + 0.95, T_METAL, [lc[0] * 0.6, lc[1] * 0.6, lc[2] * 0.6, 1.0]);
                }
            }
        }
        if parapet && len > 0.4 {
            let d = (c - a) / len;
            let inw = Vec2::new(d.y, -d.x) * 0.4;
            let pc = [0.42 * shade, 0.43 * shade, 0.5 * shade, 1.0];
            mb.wall(a, c, h, h + ph, T_METAL, pc);
            mb.wall(c + inw, a + inw, h, h + ph, T_METAL, pc);
            mb.flat_quad([a, a + inw, c + inw, c], h + ph, T_METAL, [0.6, 0.62, 0.72, 1.0]);
        }
    }
    // roof surface, remembering triangles for prop placement
    let mut tris: Vec<([Vec2; 3], f32)> = vec![];
    let mut total = 0.0;
    // roofs vary: bitumen, rust-red tin, green patina, pale gravel
    const ROOFS: [[f32; 4]; 6] = [
        [1.0, 1.0, 1.0, 1.0],
        [1.5, 0.75, 0.7, 1.0],
        [0.75, 1.1, 1.0, 1.0],
        [1.35, 1.3, 1.2, 1.0],
        [0.7, 0.72, 0.85, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    roof.tint = Some(ROOFS[rng.int(6) as usize]);
    for t in b.tris.chunks_exact(3) {
        if t.iter().any(|&i| i as usize >= n) {
            continue;
        }
        let tri = [b.pts[t[0] as usize], b.pts[t[1] as usize], b.pts[t[2] as usize]];
        let area = ((tri[1] - tri[0]).perp_dot(tri[2] - tri[0])).abs() * 0.5;
        total += area;
        tris.push((tri, area));
        roof.tri(tri, ground + h, M_SPAN[M_ROOF]);
    }
    roof.tint = None;
    let sample = |rng: &mut Rng, margin: f32| -> Option<Vec2> {
        for _ in 0..6 {
            let mut pick = rng.f() * total;
            for (tri, area) in &tris {
                pick -= area;
                if pick <= 0.0 {
                    let (mut u, mut v) = (rng.f(), rng.f());
                    if u + v > 1.0 {
                        u = 1.0 - u;
                        v = 1.0 - v;
                    }
                    let p = tri[0] + (tri[1] - tri[0]) * u + (tri[2] - tri[0]) * v;
                    if nearest_on_poly(p, &b.pts).1 > margin * margin {
                        return Some(p);
                    }
                    break;
                }
            }
        }
        None
    };
    // rooftop clutter
    if b.kind != 2 {
        let props = ((total / 110.0) as u32).min(9);
        for _ in 0..props {
            let Some(p) = sample(&mut rng, 2.3) else { continue };
            match rng.int(7) {
                0 | 1 => {
                    let (hx, hz) = if rng.chance(0.5) { (0.9, 0.65) } else { (0.65, 0.9) };
                    mb.boxx(Vec3::new(p.x - hx, h, p.y - hz), Vec3::new(p.x + hx, h + 1.0, p.y + hz), T_METAL, T_ROOF_VENT, WHITE);
                }
                2 => mb.boxx(Vec3::new(p.x - 1.3, h, p.y - 0.9), Vec3::new(p.x + 1.3, h + 0.3, p.y + 0.9), T_METAL, T_ROOF_SKY, WHITE),
                3 => mb.prism(p, 1.1, 1.0, h, h + 1.9, 8, rng.f(), T_CONTAINER, T_METAL, [0.5, 0.55, 0.62, 1.0]),
                4 => {
                    mb.boxx(Vec3::new(p.x - 0.07, h, p.y - 0.07), Vec3::new(p.x + 0.07, h + 4.5, p.y + 0.07), T_METAL, T_METAL, WHITE);
                    mb.boxx(Vec3::new(p.x - 0.18, h + 4.5, p.y - 0.18), Vec3::new(p.x + 0.18, h + 4.8, p.y + 0.18), T_TAILLIGHT, T_TAILLIGHT, WHITE);
                }
                5 => mb.boxx(Vec3::new(p.x - 1.6, h, p.y - 1.0), Vec3::new(p.x + 1.6, h + 0.25, p.y + 1.0), T_METAL, T_ROOF_PANEL, WHITE),
                _ => mb.boxx(Vec3::new(p.x - 1.5, h, p.y - 1.5), Vec3::new(p.x + 1.5, h + 2.3, p.y + 1.5), T_WALL + style * 4, T_ROOF, col),
            }
        }
    }

    let neon_p = match (b.kind, b.lv) {
        (1, _) => 0.9,
        (2, _) => 0.06,
        (_, 9..) => 0.55,
        (_, 4..) => 0.3,
        _ => 0.12,
    };
    if !rng.chance(neon_p) {
        return;
    }
    let k = rng.int(8);
    // neon trim along the roof edge
    for i in 0..n {
        let (a, c) = (b.pts[i], b.pts[(i + 1) % n]);
        let d = (c - a).normalize_or_zero();
        let inw = Vec2::new(d.y, -d.x);
        mb.flat_quad([a + inw * 0.05, a + inw * 0.36, c + inw * 0.36, c + inw * 0.05], h + ph + 0.04, T_STRIP + k, WHITE);
    }
    // rooftop billboard
    if total > 90.0 && rng.chance(0.7) {
        if let Some(c) = sample(&mut rng, 4.5) {
            let s = rng.int(16);
            let c2 = NEON[(s % 8) as usize];
            mb.boxx(Vec3::new(c.x - 4.0, h, c.y - 2.0), Vec3::new(c.x + 4.0, h + 1.4, c.y + 2.0), T_METAL, T_SIGN + s, WHITE);
            glow.ground(c.x - 9.0, c.y - 7.0, c.x + 9.0, c.y + 7.0, h + 1.5, T_GLOW, 0, rgb(c2, 0.2));
        }
    }
    // hanging signs with light spilling on the street
    let nsigns = 1 + rng.int(if b.kind == 1 { 4 } else { 2 });
    for _ in 0..nsigns {
        let i = rng.int(n as u32) as usize;
        let (a, c) = (b.pts[i], b.pts[(i + 1) % n]);
        if a.distance(c) < 6.0 {
            continue;
        }
        let d = (c - a).normalize();
        let out = Vec2::new(-d.y, d.x);
        let m = a.lerp(c, rng.range(0.2, 0.8)) + out * 1.4;
        let s = rng.int(16);
        let c2 = NEON[(s % 8) as usize];
        let y = rng.range(4.0, 7.0).min(h - 0.4);
        mb.boxx(Vec3::new(m.x - 1.6, y, m.y - 1.6), Vec3::new(m.x + 1.6, y + 0.35, m.y + 1.6), T_STRIP + (s % 8), T_SIGN + s, WHITE);
        let g = m + out * 2.0;
        glow.ground(g.x - 8.0, g.y - 8.0, g.x + 8.0, g.y + 8.0, 0.16, T_GLOW, 0, rgb(c2, 0.16));
        lights.push(LightSite { pos: Vec3::new(g.x, 3.0, g.y), color: c2, power: 0.75 });
    }
}

fn spawn_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh: Mesh,
    mat: &Handle<StandardMaterial>,
    parent: Option<Entity>,
    casts: bool,
) -> Entity {
    let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat.clone()), Transform::default()));
    if let Some(p) = parent {
        e.insert(ChildOf(p));
    }
    if !casts {
        e.insert(NotShadowCaster);
    }
    e.id()
}

// ---- streaming ------------------------------------------------------------
pub fn stream_world(
    mut commands: Commands,
    mut city: ResMut<City>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<AssetServer>,
    blobs: Res<Assets<TileBlob>>,
    art: Res<Art>,
    game: Res<Game>,
) {
    let city = &mut *city;
    let center = cell_of(game.cam_focus);

    // 1. request and ingest map tiles around the camera
    let ct = tile_of_cell(center);
    for dz in -TILE_R..=TILE_R {
        for dx in -TILE_R..=TILE_R {
            let t = ct + IVec2::new(dx, dz);
            if city.tile_exists(t) && !city.tiles.contains_key(&t) {
                let h = assets.load(format!("map/{}_{}.bin", t.x, t.y));
                city.tiles.insert(t, TileState::Requested(h));
            }
        }
    }
    let mut arrived: Vec<(IVec2, Option<Handle<TileBlob>>)> = vec![];
    for (t, st) in &city.tiles {
        if let TileState::Requested(h) = st {
            if blobs.contains(h) {
                arrived.push((*t, Some(h.clone())));
            } else if matches!(assets.load_state(h), LoadState::Failed(_)) {
                arrived.push((*t, None));
            }
        }
    }
    arrived.sort_by_key(|(t, _)| (t.x - ct.x).abs().max((t.y - ct.y).abs()));
    for (t, h) in arrived.into_iter().take(3) {
        if let Some(blob) = h.and_then(|h| blobs.get(&h)) {
            city.ingest(t, &blob.0);
        }
        city.tiles.insert(t, TileState::Ready);
    }

    // 2. mesh cells near the camera, drop far ones
    let far: Vec<IVec2> = city
        .cells
        .keys()
        .filter(|c| (c.x - center.x).abs() > LOAD_R + 1 || (c.y - center.y).abs() > LOAD_R + 1)
        .copied()
        .collect();
    for c in far {
        if let Some(cd) = city.cells.remove(&c) {
            commands.entity(cd.entity).despawn();
            if let Some(u) = cd.under {
                commands.entity(u).despawn();
            }
        }
    }
    let mut missing: Vec<IVec2> = vec![];
    for dz in -LOAD_R..=LOAD_R {
        for dx in -LOAD_R..=LOAD_R {
            let c = center + IVec2::new(dx, dz);
            if !city.cells.contains_key(&c) && city.cell_ready(c) {
                missing.push(c);
            }
        }
    }
    missing.sort_by_key(|c| (c.x - center.x).abs().max((c.y - center.y).abs()));
    let budget = if city.cells.len() < 9 { 100 } else { 2 };
    for c in missing.into_iter().take(budget) {
        if !city.src(c).is_some_and(|s| s.scattered) {
            city.scatter(c);
        }
        let src = city.src(c).unwrap();
        let o = c.as_vec2() * CELL;
        let mut mb = MB::default();
        mb.drape = Some(terrain);
        let mut glow = MB::default();
        glow.drape = Some(terrain);
        glow.lift = 0.32; // glows and shadows float just above the road surface
        let mut roof = Flat::default();
        let mut lights: Vec<LightSite> = vec![];

        // ground layers
        let mut grass = Flat::default();
        grass.drape = Some(terrain);
        let mut umb = MB::default();
        umb.drape = Some(terrain);
        let mut uglow = MB::default();
        uglow.drape = Some(terrain);
        uglow.lift = 0.32;
        let mut smb = MB::default();
        smb.drape = Some(terrain);
        smb.lift = 0.2;
        for (q, alpha) in &src.shades {
            smb.quad(*q, T_WHITE, 0, [0.0, 0.0, 0.0, *alpha]);
        }
        let step = if src.holes.is_empty() && src.segs.is_empty() { 8.0 } else { 4.0 };
        let n = (CELL / step) as i32;
        // the ground dips slightly under roads and paths so it can never show through them
        let dip: Vec<f32> = (0..(n + 1) * (n + 1))
            .map(|i| {
                let p = o + Vec2::new((i % (n + 1)) as f32, (i / (n + 1)) as f32) * step;
                if src.segs.iter().any(|(a, b, hw)| seg_dist(p, *a, *b) < *hw + 3.2) { -0.55 } else { 0.0 }
            })
            .collect();
        for gz in 0..n {
            for gx in 0..n {
                let a = o + Vec2::new(gx as f32, gz as f32) * step;
                let mid = a + step * 0.5;
                if src.holes.iter().any(|(h0, h1, hw)| seg_dist(mid, *h0, *h1) < *hw + 0.3) {
                    continue;
                }
                let q = [a, a + Vec2::Y * step, a + step, a + Vec2::X * step];
                let k = q.map(relief_shade);
                let at = |x: i32, z: i32| dip[(z * (n + 1) + x) as usize];
                let d = [at(gx, gz), at(gx, gz + 1), at(gx + 1, gz + 1), at(gx + 1, gz)];
                grass.tri_cy([q[0], q[1], q[2]], [d[0], d[1], d[2]], M_SPAN[M_GRASS], [k[0], k[1], k[2]]);
                grass.tri_cy([q[0], q[2], q[3]], [d[0], d[2], d[3]], M_SPAN[M_GRASS], [k[0], k[2], k[3]]);
            }
        }
        let entity = spawn_mesh(&mut commands, &mut meshes, grass.build(), &art.tiled[M_GRASS], None, false);
        for (m, y) in [(M_ASPHALT, 0.3), (M_PAVING, 0.16), (M_CONCRETE, 0.07), (M_WATER, 0.09)] {
            if !src.tris[m].is_empty() {
                let mut f = Flat::default();
                f.drape = Some(terrain);
                for t in &src.tris[m] {
                    f.tri_c(*t, y, M_SPAN[m], t.map(relief_shade));
                }
                spawn_mesh(&mut commands, &mut meshes, f.build(), &art.tiled[m], Some(entity), false);
            }
        }
        // road paint
        // anything dipping below ground goes into a separate group that stays visible in tunnels
        let below = |ys: &[f32]| ys.iter().any(|y| *y < -0.06);
        let mut under: Option<Entity> = None;
        for (m, mat) in [(0, M_ASPHALT), (1, M_PAVING)] {
            let (mut f, mut uf) = (Flat::default(), Flat::default());
            f.drape = Some(terrain);
            uf.drape = Some(terrain);
            for t in &src.deck[m] {
                if below(&[t[0].y - 0.3, t[1].y - 0.3, t[2].y - 0.3]) { uf.tri3(*t, M_SPAN[mat]) } else { f.tri3(*t, M_SPAN[mat]) }
            }
            if !f.is_empty() {
                let dm = if m == 0 { &art.deck } else { &art.tiled[mat] };
                // (deck shares the asphalt texture and its 16 m repeat)
                spawn_mesh(&mut commands, &mut meshes, f.build(), dm, Some(entity), true);
            }
            if !uf.is_empty() {
                let root = *under.get_or_insert_with(|| commands.spawn((Transform::default(), Visibility::Inherited)).id());
                spawn_mesh(&mut commands, &mut meshes, uf.build(), &art.tiled[mat], Some(root), false);
            }
        }
        for (pts, tile, shade, id) in &src.quads {
            if *tile == 0 {
                // lane and edge lines stop where they would run across a different road:
                // junctions stay clear and merging lanes no longer criss-cross
                let c = ((pts[0] + pts[1] + pts[2] + pts[3]) * 0.25).xz();
                let cy = (pts[0].y + pts[2].y) * 0.5;
                let crossed = src.roads.iter().any(|(a, b, hw, rid, ry)| {
                    if rid == id || (ry - cy).abs() > 1.5 {
                        return false;
                    }
                    let ab = *b - *a;
                    let t = (c - *a).dot(ab) / ab.length_squared().max(1e-6);
                    t > 0.0 && t < 1.0 && (*a + ab * t).distance(c) < hw - 0.4
                });
                if crossed {
                    continue;
                }
            }
            let s = *shade as f32 / 255.0;
            let (t, col, y) = if *tile == 3 { (T_CROSS, WHITE, 0.35) } else { (T_WHITE, [s * 0.85, s * 0.85, s * 0.78, 1.0], 0.36) };
            let target = if below(&[pts[0].y, pts[2].y]) { &mut umb } else { &mut mb };
            target.quad(pts.map(|p| p + Vec3::Y * y), t, 0, col);
        }
        for (q, tile, col) in &src.equads {
            let target = if below(&[q[1].y, q[2].y]) { &mut umb } else { &mut mb };
            target.quad(*q, *tile, 0, *col);
        }
        for (p, q, yp, yq, kind, paint) in &src.edges {
            let (p, q, yp, yq) = (*p, *q, *yp, *yq);
            let d = (q - p).normalize_or_zero();
            let out = Vec2::new(-d.y, d.x);
            let ym = (yp + yq) * 0.5;
            // where another raised road continues beyond this edge (merges, forks, crossings at
            // the same level) there is no edge to build
            if city.deck_adjacent((p + q) * 0.5 + out * 1.0, ym) {
                continue;
            }
            let v = |a: Vec2, y: f32| Vec3::new(a.x, y, a.y);
            let sy = 0.3;
            let target = if ym < -0.06 { &mut umb } else { &mut mb };
            match kind {
                0 => {
                    let dark = [0.22, 0.22, 0.27, 1.0];
                    let rail = [0.78, 0.8, 0.9, 1.0];
                    target.quad([v(p, yp + sy), v(p, yp - 0.9), v(q, yq - 0.9), v(q, yq + sy)], T_METAL, 0, dark);
                    let (t0, t1) = (v(p, yp + 0.95), v(q, yq + 0.95));
                    target.quad([t0, v(p, yp + sy), v(q, yq + sy), t1], T_METAL, 0, rail);
                    target.quad([t1, v(q, yq + sy), v(p, yp + sy), t0], T_METAL, 0, rail);
                    let inw = out * -0.45;
                    target.quad(up_quad([t0, v(p + inw, yp + 0.95), v(q + inw, yq + 0.95), t1]), T_METAL, 0, [1.0, 0.72, 0.3, 1.0]);
                }
                1 => {
                    // grassed embankment sloping down to the ground
                    let (op, oq) = (p + out * (yp * 1.7 + 0.3), q + out * (yq * 1.7 + 0.3));
                    target.quad(up_quad([v(p, yp + sy), v(op, 0.02), v(oq, 0.02), v(q, yq + sy)]), T_GRASS, 0, [0.75, 0.8, 0.8, 1.0]);
                }
                2 => {
                    let wallc = [0.3, 0.3, 0.36, 1.0];
                    target.quad([v(q, 0.0), v(q, yq), v(p, yp), v(p, 0.0)], T_METAL, 0, wallc);
                }
                _ => {
                    let wallc = [0.3, 0.3, 0.36, 1.0];
                    target.quad([v(q, yq + 4.6), v(q, yq), v(p, yp), v(p, yp + 4.6)], T_METAL, 0, wallc);
                    target.quad([v(q, yq + 3.3), v(q, yq + 3.0), v(p, yp + 3.0), v(p, yp + 3.3)], T_STRIP + 2, 0, WHITE);
                }
            }
            if *paint && *kind != 3 {
                let (i0, i1) = (out * -0.3, out * -0.5);
                let e = d * 0.12;
                target.quad(
                    up_quad([v(p - e + i0, yp + 0.4), v(p - e + i1, yp + 0.4), v(q + e + i1, yq + 0.4), v(q + e + i0, yq + 0.4)]),
                    T_WHITE,
                    0,
                    [0.72, 0.72, 0.66, 1.0],
                );
            }
        }
        // from here on: free-standing objects, each lifted to the ground height where it stands
        mb.drape = None;
        for (mn, mx, tile, col) in &src.boxes {
            mb.lift = terrain(((*mn + *mx) * 0.5).xz());
            mb.boxx(*mn, *mx, *tile, *tile, *col);
        }
        for (c0, rad) in &src.islands {
            // landscaped roundabout island with a lit centrepiece
            mb.lift = terrain(*c0);
            let k = hash2(c0.x as i32, c0.y as i32, 5) % 8;
            let nc = NEON[k as usize];
            mb.prism(*c0, *rad, *rad - 0.25, 0.0, 0.32, 20, 0.0, T_METAL, T_GRASS, [0.8, 0.8, 0.85, 1.0]);
            if *rad > 4.0 {
                let pr = (*rad * 0.3).min(3.0);
                mb.prism(*c0, pr, pr, 0.32, 1.1, 10, 0.3, T_METAL, T_STRIP + k, WHITE);
                mb.prism(*c0, pr * 0.45, 0.12, 1.1, 6.5, 6, 0.0, T_METAL, T_HEADLIGHT, WHITE);
                glow.ground(c0.x - rad * 1.6, c0.y - rad * 1.6, c0.x + rad * 1.6, c0.y + rad * 1.6, 0.4, T_GLOW, 0, rgb(nc, 0.22));
                lights.push(LightSite { pos: Vec3::new(c0.x, 3.5, c0.y), color: nc, power: 1.3 });
                for j in 0..6 {
                    let p = *c0 + Vec2::from_angle(j as f32 * 1.047 + 0.4) * (*rad * 0.68);
                    mb.prism(p, 0.9, 0.5, 0.32, 1.3, 6, j as f32, T_LEAF, T_LEAF, WHITE);
                }
            }
        }
        for (p, _) in &src.cams {
            // speed camera: grey mast, striped housing, lens facing the road
            mb.lift = terrain(*p);
            mb.prism(*p, 0.14, 0.11, 0.0, 3.6, 6, 0.0, T_METAL, T_METAL, [0.7, 0.7, 0.75, 1.0]);
            mb.boxx(Vec3::new(p.x - 0.55, 3.6, p.y - 0.4), Vec3::new(p.x + 0.55, 4.5, p.y + 0.4), T_METAL, T_STRIP + 6, WHITE);
            mb.boxx(Vec3::new(p.x - 0.2, 3.85, p.y - 0.46), Vec3::new(p.x + 0.2, 4.25, p.y + 0.46), T_HEADLIGHT, T_HEADLIGHT, WHITE);
            glow.ground(p.x - 2.5, p.y - 2.5, p.x + 2.5, p.y + 2.5, 4.4, T_GLOW, 0, [0.35, 0.22, 0.03, 1.0]);
        }
        for (p, kind) in &src.props {
            mb.lift = terrain(*p);
            if *kind == 0 {
                // traffic signal: mast with a three-lamp head
                mb.prism(*p, 0.12, 0.09, 0.0, 5.0, 5, 0.0, T_METAL, T_METAL, WHITE);
                for (j, t) in [T_TAILLIGHT, T_STRIP + 6, T_STRIP + 3].iter().enumerate() {
                    let x = p.x - 0.6 + j as f32 * 0.42;
                    mb.boxx(Vec3::new(x, 5.0, p.y - 0.2), Vec3::new(x + 0.38, 5.35, p.y + 0.2), T_METAL, *t, WHITE);
                }
                glow.ground(p.x - 2.2, p.y - 2.2, p.x + 2.2, p.y + 2.2, 5.5, T_GLOW, 0, [0.12, 0.3, 0.1, 1.0]);
            } else {
                // bus shelter with a lit advertising panel
                let s = hash2(p.x as i32, p.y as i32, 8) % 16;
                let nc = NEON[(s % 8) as usize];
                for (dx, dz) in [(-1.9, -0.7), (1.9, -0.7), (-1.9, 0.7), (1.9, 0.7)] {
                    mb.boxx(Vec3::new(p.x + dx - 0.06, 0.0, p.y + dz - 0.06), Vec3::new(p.x + dx + 0.06, 2.5, p.y + dz + 0.06), T_METAL, T_METAL, WHITE);
                }
                mb.boxx(Vec3::new(p.x - 2.1, 2.5, p.y - 0.9), Vec3::new(p.x + 2.1, 2.62, p.y + 0.9), T_METAL, T_GLASS, WHITE);
                mb.boxx(Vec3::new(p.x + 1.5, 0.3, p.y - 0.75), Vec3::new(p.x + 1.62, 2.3, p.y + 0.75), T_SIGN + s, T_STRIP + (s % 8), WHITE);
                mb.boxx(Vec3::new(p.x - 1.4, 0.0, p.y - 0.55), Vec3::new(p.x + 0.9, 0.5, p.y - 0.2), T_METAL, T_METAL, [0.6, 0.45, 0.3, 1.0]);
                glow.ground(p.x - 5.0, p.y - 5.0, p.x + 5.0, p.y + 5.0, 0.17, T_GLOW, 0, rgb(nc, 0.14));
                lights.push(LightSite { pos: Vec3::new(p.x, 2.2, p.y), color: nc, power: 0.5 });
            }
        }
        for &bi in &src.own {
            add_building(&city.blds[bi as usize], bi, &mut mb, &mut roof, &mut glow, &mut smb, &mut lights);
        }
        for (p, s) in &src.trees {
            mb.lift = terrain(*p);
            let k = *s as f32 / 255.0;
            let r = 1.7 + k * 1.9;
            let g = 0.7 + ((*s as u32 * 37) % 100) as f32 * 0.006;
            let hgt = 3.2 + k * 3.5;
            let leaf = if *s % 7 == 0 { T_LEAF + 1 } else { T_LEAF };
            let rot = k * 6.0;
            mb.prism(*p, 0.28, 0.2, 0.0, hgt, 5, rot, T_METAL, T_METAL, [0.45, 0.28, 0.2, 1.0]);
            let o2 = Vec2::from_angle(rot) * r * 0.22;
            mb.prism(*p, r, r * 0.9, hgt, hgt + 1.3, 7, rot, leaf, leaf, [g * 0.7, g * 0.7, g * 0.7, 1.0]);
            mb.prism(*p + o2, r * 0.72, r * 0.6, hgt + 1.3, hgt + 2.5, 6, rot + 0.5, leaf, leaf, [g, g, g, 1.0]);
            mb.prism(*p - o2 * 0.6, r * 0.42, r * 0.25, hgt + 2.5, hgt + 3.5, 5, rot + 1.1, leaf, leaf, [g * 1.35, g * 1.35, g * 1.35, 1.0]);
        }
        for (p, kind, base) in &src.lights {
            if city.in_building(*p, 0.6) {
                continue;
            }
            let base = *base;
            // never plant a lamp on a carriageway, path or neighbouring lane: on dual carriageways,
            // slip roads and wide junctions the usual roadside spot belongs to another road
            let on_road = *kind != 4
                && (src.roads.iter().any(|(a, b, hw, _, ry)| (ry - base).abs() < 1.5 && seg_dist(*p, *a, *b) < hw + 0.3)
                    || (base < 0.4 && src.segs.iter().any(|(a, b, hw)| seg_dist(*p, *a, *b) < hw - 0.2)));
            if on_road {
                continue;
            }
            mb.lift = terrain(*p);
            if *kind == 4 {
                // tunnel lamp: no mast, just a pool of sodium light on the carriageway
                let col = [1.0, 0.55, 0.2];
                uglow.ground(p.x - 8.0, p.y - 8.0, p.x + 8.0, p.y + 8.0, base + 0.2, T_GLOW, 0, rgb(col, 0.22));
                lights.push(LightSite { pos: Vec3::new(p.x, base + 3.0, p.y), color: col, power: 0.9 });
                continue;
            }
            let col = LIGHT_COL[(*kind as usize).min(3)];
            let (hgt, power, pool) = if *kind == 3 { (4.0, 0.55, 7.0) } else { (7.5, 1.0, 11.0) };
            let strip = T_STRIP + [7, 1, 2, 0][(*kind as usize).min(3)];
            let hgt = hgt + base;
            mb.prism(*p, 0.16, 0.1, base, hgt, 5, 0.0, T_METAL, T_METAL, WHITE);
            mb.boxx(Vec3::new(p.x - 0.45, hgt, p.y - 0.45), Vec3::new(p.x + 0.45, hgt + 0.18, p.y + 0.45), T_METAL, strip, WHITE);
            glow.ground(p.x - pool, p.y - pool, p.x + pool, p.y + pool, base + 0.15, T_GLOW, 0, rgb(col, 0.13 * power));
            glow.ground(p.x - 1.6, p.y - 1.6, p.x + 1.6, p.y + 1.6, hgt + 0.3, T_GLOW, 0, rgb(col, 0.5));
            // stacked haze discs read as a cone of light in the rain
            for (f, r) in [(0.7, 2.4), (0.42, 4.2), (0.18, 6.4)] {
                let y = base + (hgt - base) * f;
                glow.ground(p.x - r, p.y - r, p.x + r, p.y + r, y, T_GLOW, 0, rgb(col, 0.024 * power));
            }
            lights.push(LightSite { pos: Vec3::new(p.x, hgt - 1.5, p.y), color: col, power });
        }
        let job = hash2(c.x, c.y, 77) % 5 == 0;
        if let Some(p) = src.poi {
            if job {
                glow.ground(p.x - 2.5, p.y - 2.5, p.x + 2.5, p.y + 2.5, 0.17, T_MARKER, 0, [1.0, 0.8, 0.1, 1.0]);
                glow.ground(p.x - 6.0, p.y - 6.0, p.x + 6.0, p.y + 6.0, 0.16, T_GLOW, 0, [0.3, 0.24, 0.03, 1.0]);
            } else if hash2(c.x, c.y, 78) % 100 < 22 && !city.collected.contains(&c) {
                let kind = (hash2(c.x, c.y, 79) % 3) as usize;
                commands.spawn((
                    crate::actors::Pickup { kind, cell: Some(c), value: 0 },
                    Mesh3d(art.pickups[kind].clone()),
                    MeshMaterial3d(art.mat.clone()),
                    Transform::from_xyz(p.x, terrain(p), p.y),
                    ChildOf(entity),
                ));
            }
        }
        if !mb.is_empty() {
            spawn_mesh(&mut commands, &mut meshes, mb.build(), &art.mat, Some(entity), true);
        }
        if !roof.is_empty() {
            spawn_mesh(&mut commands, &mut meshes, roof.build(), &art.tiled[M_ROOF], Some(entity), true);
        }
        if !glow.is_empty() {
            spawn_mesh(&mut commands, &mut meshes, glow.build(), &art.glow, Some(entity), false);
        }
        if !smb.is_empty() {
            spawn_mesh(&mut commands, &mut meshes, smb.build(), &art.shade, Some(entity), false);
        }
        for (p, h) in &src.parked {
            let kind = (hash2(p.x as i32, p.y as i32, 5) % 8) as usize;
            crate::actors::spawn_car(&mut commands, &art, *p, *h, kind, None);
        }
        if !umb.is_empty() || !uglow.is_empty() {
            let root = *under.get_or_insert_with(|| commands.spawn((Transform::default(), Visibility::Inherited)).id());
            if !umb.is_empty() {
                spawn_mesh(&mut commands, &mut meshes, umb.build(), &art.mat, Some(root), false);
            }
            if !uglow.is_empty() {
                spawn_mesh(&mut commands, &mut meshes, uglow.build(), &art.glow, Some(root), false);
            }
        }
        for l in &mut lights {
            l.pos.y += terrain(l.pos.xz());
        }
        commands.entity(entity).insert(Visibility::Inherited);
        city.cells.insert(c, Loaded { lights, entity, under });
    }
}

// ---- dynamic light pool ---------------------------------------------------
#[derive(Component)]
pub struct PoolLight;

pub const POOL_LIGHTS: usize = 40;

pub fn setup_lights(mut commands: Commands) {
    for _ in 0..POOL_LIGHTS {
        commands.spawn((
            PoolLight,
            PointLight { intensity: 0.0, range: 24.0, radius: 0.5, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(0.0, -100.0, 0.0),
        ));
    }
}

pub fn assign_lights(
    city: Res<City>,
    game: Res<Game>,
    mut lights: Query<(&mut PointLight, &mut Transform), With<PoolLight>>,
    mut timer: Local<f32>,
    time: Res<Time>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.15;
    let f = game.cam_focus;
    let center = cell_of(f);
    let mut sites: Vec<(f32, &LightSite)> = vec![];
    for dz in -1..=1 {
        for dx in -1..=1 {
            if let Some(cd) = city.cells.get(&(center + IVec2::new(dx, dz))) {
                for l in &cd.lights {
                    sites.push((Vec2::new(l.pos.x, l.pos.z).distance(f), l));
                }
            }
        }
    }
    sites.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut it = sites.into_iter();
    for (mut pl, mut t) in &mut lights {
        if let Some((d, s)) = it.next() {
            let fade = ((90.0 - d) / 25.0).clamp(0.0, 1.0);
            // a few tired neon tubes and lamps stutter
            let h = hash2((s.pos.x * 3.0) as i32, (s.pos.z * 3.0) as i32, 99);
            let flick = if h % 6 == 0 && hash2(h as i32, (game.time * 7.0) as i32, 5) % 10 < 3 { 0.2 } else { 1.0 };
            pl.intensity = crate::LIGHT_POWER * s.power * fade * flick * (0.05 + 0.95 * game.night);
            pl.color = Color::linear_rgb(s.color[0], s.color[1], s.color[2]);
            t.translation = s.pos;
        } else {
            pl.intensity = 0.0;
        }
    }
}

/// GTA-style cutaway: while the player is below ground, hide the surface so the tunnel is visible.
pub fn underground_view(city: Res<City>, game: Res<Game>, mut vis: Query<&mut Visibility>, mut was: Local<bool>) {
    let under = game.y < -1.6;
    if under == *was && !city.is_changed() {
        return;
    }
    *was = under;
    for cd in city.cells.values() {
        if let Ok(mut v) = vis.get_mut(cd.entity) {
            let want = if under { Visibility::Hidden } else { Visibility::Inherited };
            if *v != want {
                *v = want;
            }
        }
    }
}

impl City {
    /// Does a raised or sunken road surface at about height `y` cover point `p`?
    fn deck_adjacent(&self, p: Vec2, y: f32) -> bool {
        self.src(cell_of(p)).is_some_and(|c| {
            c.decks.iter().any(|(a, b, hw, ya, yb)| {
                let ab = *b - *a;
                let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                (*a + ab * t).distance(p) <= *hw - 0.3 && ((ya + (yb - ya) * t) - y).abs() < 1.6
            })
        })
    }

    /// Is there a bridge deck overhead at `p` for something at height `y`?
    pub fn under_deck(&self, p: Vec2, y: f32) -> bool {
        self.src(cell_of(p)).is_some_and(|c| {
            c.decks.iter().any(|(a, b, hw, ya, yb)| {
                let ab = *b - *a;
                let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                (*a + ab * t).distance(p) <= *hw + 1.5 && ya + (yb - ya) * t > y + 2.5
            })
        })
    }
}

/// Bridges are solid, but turn glassy while the player is underneath so the car stays visible.
pub fn deck_fade(
    time: Res<Time>,
    city: Res<City>,
    game: Res<Game>,
    art: Res<Art>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut alpha: Local<Option<f32>>,
) {
    let cur = alpha.unwrap_or(1.0);
    let want = if city.under_deck(game.pos, game.y) { 0.3 } else { 1.0 };
    let next = cur + (want - cur).clamp(-time.delta_secs() * 3.5, time.delta_secs() * 3.5);
    if (next - cur).abs() < 1e-4 && alpha.is_some() {
        return;
    }
    *alpha = Some(next);
    if let Some(mut m) = mats.get_mut(&art.deck) {
        m.base_color = Color::linear_rgba(1.12, 1.12, 1.2, next);
        m.alpha_mode = if next > 0.995 { AlphaMode::Opaque } else { AlphaMode::Blend };
    }
}

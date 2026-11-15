//! Hand-built models of Vilnius landmarks. Each stands on the real footprint (or spot) of the
//! original, taken from OpenStreetMap by `tools/build_extras.py`; the architecture on top is
//! modelled here from simple parts: masonry walls, windows, columns, pediments, pitched roofs
//! and turned shapes (domes, spires, the TV tower).
//!
//! Heights are in metres above the ground at the landmark. Solid masses stay under about 40 m
//! so the overhead camera never ends up inside one; only thin spires and masts go higher.

use crate::art::*;
use crate::world::{Landmark, LightSite, terrain};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

type Rgb = [f32; 3];

const STONE: Rgb = [0.88, 0.86, 0.8];
const PLASTER: Rgb = [0.93, 0.9, 0.82];
const CREAM: Rgb = [0.93, 0.86, 0.68];
const BRICK: Rgb = [0.58, 0.23, 0.16];
const TILE: Rgb = [0.6, 0.24, 0.16];
const TIN: Rgb = [0.3, 0.33, 0.38];
const COPPER: Rgb = [0.3, 0.5, 0.44];
const GLASS: Rgb = [0.06, 0.08, 0.13];
const GRANITE: Rgb = [0.42, 0.42, 0.45];
const GOLD: Rgb = [1.0, 0.78, 0.25];
const CONCRETE: Rgb = [0.7, 0.7, 0.73];

/// English name of a landmark kind, for the HUD.
pub fn name(kind: u8) -> &'static str {
    match kind {
        1 => "VILNIUS CATHEDRAL",
        2 => "GEDIMINAS' TOWER",
        3 => "CATHEDRAL BELL TOWER",
        4 => "EUROPA TOWER",
        5 => "TOWN HALL",
        6 => "ST. ANNE'S CHURCH",
        7 => "ST. PETER AND ST. PAUL'S",
        8 => "GATE OF DAWN",
        9 => "OPERA AND BALLET THEATRE",
        10 => "PALACE OF THE GRAND DUKES",
        11 => "TV TOWER",
        _ => "THREE CROSSES",
    }
}

// ---- parts ------------------------------------------------------------------

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Quad wound so that it faces `out`.
fn quad_out(m: &mut MB, q: [Vec3; 4], out: Vec3, col: [f32; 4]) {
    let n = (q[1] - q[0]).cross(q[3] - q[0]);
    let q = if n.dot(out) < 0.0 { [q[0], q[3], q[2], q[1]] } else { q };
    m.quad(q, T_WHITE, 0, col);
}

/// Outward normal of the wall running a -> b (footprints are wound so this points outside).
fn outward(a: Vec2, b: Vec2) -> Vec2 {
    let d = (b - a).normalize_or_zero();
    Vec2::new(-d.y, d.x)
}

/// Footprint shrunk (or grown) about a point.
fn scaled(pts: &[Vec2], c: Vec2, k: f32) -> Vec<Vec2> {
    pts.iter().map(|p| c + (*p - c) * k).collect()
}

/// Masonry walls around a footprint, shaded darker towards the ground.
fn walls(m: &mut MB, pts: &[Vec2], y0: f32, y1: f32, tile: u32, col: Rgb) {
    let n = pts.len();
    let rows = ((y1 - y0) / 3.5).round().max(1.0) as i32;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        for row in 0..rows {
            let s = 0.7 + 0.3 * (row + 1) as f32 / rows as f32;
            let (ya, yb) = (y0 + (y1 - y0) * row as f32 / rows as f32, y0 + (y1 - y0) * (row + 1) as f32 / rows as f32);
            m.wall(a, b, ya, yb, tile, rgb(col, s));
        }
        if y0 <= 0.0 {
            m.wall(a, b, -9.0, 0.0, T_METAL, [0.2, 0.2, 0.24, 1.0]); // footing on sloping ground
        }
    }
}

/// A projecting course (plinth, string course, cornice) running round a footprint.
fn band(m: &mut MB, pts: &[Vec2], y0: f32, y1: f32, proud: f32, col: Rgb) {
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a.distance(b) < 0.3 {
            continue;
        }
        let d = (b - a).normalize_or_zero();
        let o = outward(a, b) * proud;
        let (a2, b2) = (a - d * proud + o, b + d * proud + o);
        m.wall(a2, b2, y0, y1, T_WHITE, rgb(col, 1.0));
        m.flat_quad([a, a2, b2, b], y1, T_WHITE, rgb(col, 1.08));
        quad_out(m, [v3(a, y0), v3(a2, y0), v3(b2, y0), v3(b, y0)], Vec3::NEG_Y, rgb(col, 0.5));
    }
}

/// One window on a wall: centre `mid`, wall direction `d`, from y0 to y1.
/// `arch`: 0 square head, 1 round, 2 pointed (Gothic).
fn window(m: &mut MB, mid: Vec2, d: Vec2, w: f32, y0: f32, y1: f32, arch: u8, col: Rgb) {
    let out = Vec2::new(-d.y, d.x);
    let o = out * 0.07;
    let (a, b) = (mid - d * w * 0.5 + o, mid + d * w * 0.5 + o);
    let rise = match arch {
        1 => w * 0.5,
        2 => w * 0.95,
        _ => 0.0,
    };
    let ys = y1 - rise;
    let c4 = rgb(col, 1.0);
    m.wall(a, b, y0, ys, T_WHITE, c4);
    // stone sill and surround catch the light
    m.wall(a - d * 0.12 + o * 0.5, b + d * 0.12 + o * 0.5, y0 - 0.22, y0, T_WHITE, [0.95, 0.94, 0.9, 1.0]);
    let out3 = Vec3::new(out.x, 0.0, out.y);
    match arch {
        1 => {
            let c = v3(mid + o, ys);
            for k in 0..6 {
                let p = |k: i32| {
                    let t = k as f32 / 6.0 * PI;
                    v3(mid + o + d * (t.cos() * w * 0.5), ys + t.sin() * w * 0.5)
                };
                quad_out(m, [c, p(k), p(k + 1), p(k + 1)], out3, c4);
            }
        }
        2 => {
            let top = v3(mid + o, y1);
            quad_out(m, [v3(a, ys), v3(b, ys), top, top], out3, c4);
        }
        _ => {}
    }
}

/// Rows of windows along every wall of a footprint longer than `min_len`.
fn windows(m: &mut MB, pts: &[Vec2], min_len: f32, spacing: f32, w: f32, y0: f32, y1: f32, arch: u8, col: Rgb) {
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let len = a.distance(b);
        if len < min_len {
            continue;
        }
        let d = (b - a) / len;
        let k = ((len - 1.2) / spacing).floor().max(1.0) as i32;
        for j in 0..k {
            window(m, a + d * (len * (j as f32 + 0.5) / k as f32), d, w, y0, y1, arch, col);
        }
    }
}

/// A window on every face of an n-sided tower (same angles as `MB::prism`).
fn ring_windows(m: &mut MB, c: Vec2, r: f32, n: usize, rot: f32, w: f32, y0: f32, y1: f32, arch: u8, col: Rgb) {
    for k in 0..n {
        let dir = Vec2::from_angle(rot - (k as f32 + 0.5) / n as f32 * TAU);
        let mid = c + dir * (r * (PI / n as f32).cos());
        window(m, mid, Vec2::new(dir.y, -dir.x), w, y0, y1, arch, col);
    }
}

/// A shape turned on a lathe: `prof` is a list of (radius, height) going up.
fn lathe(m: &mut MB, c: Vec2, prof: &[(f32, f32)], n: usize, rot: f32, tile: u32, col: Rgb) {
    for w in prof.windows(2) {
        m.prism(c, w[0].0, w[1].0, w[0].1, w[1].1, n, rot, tile, tile, rgb(col, 1.0));
    }
}

/// Hemispherical dome of radius `r` rising `h` from y0.
fn dome(m: &mut MB, c: Vec2, r: f32, y0: f32, h: f32, n: usize, col: Rgb) {
    let prof: Vec<(f32, f32)> = (0..=6).map(|k| {
        let t = k as f32 / 6.0 * PI * 0.5;
        ((r * t.cos()).max(0.05), y0 + h * t.sin())
    }).collect();
    for (i, w) in prof.windows(2).enumerate() {
        // lighter towards the crown, as a real dome catches the sky
        m.prism(c, w[0].0, w[1].0, w[0].1, w[1].1, n, 0.0, T_WHITE, T_WHITE, rgb(col, 0.85 + 0.06 * i as f32));
    }
}

/// Classical column with base and capital.
fn column(m: &mut MB, p: Vec2, r: f32, y0: f32, y1: f32, col: Rgb) {
    let h = y1 - y0;
    lathe(
        m,
        p,
        &[(r * 1.35, y0), (r * 1.3, y0 + 0.3), (r, y0 + 0.4), (r * 0.86, y0 + h - 0.5), (r * 1.3, y0 + h - 0.35), (r * 1.35, y0 + h)],
        10,
        0.0,
        T_WHITE,
        col,
    );
}

/// Box aligned with a frame: `o` origin, `ax` long axis; a = along, s = across.
fn obox(m: &mut MB, o: Vec2, ax: Vec2, a0: f32, a1: f32, s0: f32, s1: f32, y0: f32, y1: f32, col: Rgb) {
    let sd = Vec2::new(-ax.y, ax.x);
    let p = |a: f32, s: f32| o + ax * a + sd * s;
    let q = [p(a0, s0), p(a0, s1), p(a1, s1), p(a1, s0)];
    for i in 0..4 {
        m.wall(q[i], q[(i + 1) % 4], y0, y1, T_WHITE, rgb(col, 0.92));
    }
    m.flat_quad(q, y1, T_WHITE, rgb(col, 1.05));
}

/// Pitched roof on a rectangle of a frame, ridge along the axis; the slopes are laid in the
/// tiled roof material (`rf`, absolute heights, hence `g`). `hip` > 0 slopes the ends in too;
/// otherwise the ends are upright gable walls in `wall` colour.
fn gable(m: &mut MB, rf: &mut Flat, g: f32, o: Vec2, ax: Vec2, a0: f32, a1: f32, s0: f32, s1: f32, y0: f32, rise: f32, hip: f32, roof: Rgb, wall: Rgb) {
    let sd = Vec2::new(-ax.y, ax.x);
    let p = |a: f32, s: f32, y: f32| v3(o + ax * a + sd * s, y);
    let sm = (s0 + s1) * 0.5;
    let (r0, r1) = (p(a0 + hip, sm, y0 + rise), p(a1 - hip, sm, y0 + rise));
    let along = Vec3::new(ax.x, 0.0, ax.y);
    let up = Vec3::Y * g;
    let mut slope = |rf: &mut Flat, q: [Vec3; 4], k: f32| {
        rf.tint = Some(rgb(roof, k));
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let t = t.map(|v| v + up);
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.length_squared() > 1e-6 {
                rf.tri3(if n.y > 0.0 { t } else { [t[0], t[2], t[1]] }, M_SPAN[M_ROOF]);
            }
        }
        rf.tint = None;
    };
    // the slope facing south (towards the sun and the camera) is the brighter one
    let south = |s: f32| 0.98 + 0.14 * (sd * s).dot(Vec2::Y);
    slope(rf, [p(a0, s0, y0), p(a1, s0, y0), r1, r0], south(-1.0));
    slope(rf, [p(a0, s1, y0), p(a1, s1, y0), r1, r0], south(1.0));
    if hip > 0.0 {
        slope(rf, [p(a0, s0, y0), p(a0, s1, y0), r0, r0], 0.98 - 0.14 * ax.dot(Vec2::Y));
        slope(rf, [p(a1, s0, y0), p(a1, s1, y0), r1, r1], 0.98 + 0.14 * ax.dot(Vec2::Y));
    } else {
        quad_out(m, [p(a0, s0, y0), p(a0, s1, y0), r0, r0], -along, rgb(wall, 0.95));
        quad_out(m, [p(a1, s0, y0), p(a1, s1, y0), r1, r1], along, rgb(wall, 0.95));
    }
}

/// Gilded cross of height `h` standing at `y`.
fn cross(m: &mut MB, p: Vec2, y: f32, h: f32) {
    let t = h * 0.06;
    m.prism(p, t * 2.2, t * 2.2, y, y + t * 3.0, 8, 0.0, T_WHITE, T_WHITE, rgb(GOLD, 1.0)); // orb
    m.boxx(Vec3::new(p.x - t, y, p.y - t), Vec3::new(p.x + t, y + h, p.y + t), T_WHITE, T_WHITE, rgb(GOLD, 1.0));
    m.boxx(Vec3::new(p.x - t, y + h * 0.62, p.y - h * 0.3), Vec3::new(p.x + t, y + h * 0.62 + t * 2.0, p.y + h * 0.3), T_WHITE, T_WHITE, rgb(GOLD, 1.0));
}

/// Statue on a pedestal.
fn statue(m: &mut MB, p: Vec2, y: f32, h: f32, col: Rgb) {
    lathe(
        m,
        p,
        &[(0.5, y), (0.5, y + 0.4), (0.36, y + 0.45), (0.3, y + h * 0.55), (0.4, y + h * 0.72), (0.2, y + h * 0.8), (0.24, y + h * 0.92), (0.06, y + h)],
        6,
        0.3,
        T_WHITE,
        col,
    );
}

/// Flat roof of a footprint (in the tiled roof material) at absolute height `y`.
fn cap(roof: &mut Flat, pts: &[Vec2], tris: &[u16], y: f32, col: Rgb) {
    roof.tint = Some(rgb(col, 1.0));
    for t in tris.chunks_exact(3) {
        if t.iter().all(|&i| (i as usize) < pts.len()) {
            roof.tri([pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]], y, M_SPAN[M_ROOF]);
        }
    }
    roof.tint = None;
}

/// Sloping roof from the eaves (`lower`, at y0) up to a smaller outline (`upper`, at y1): with a
/// `cap` on top this makes a hipped roof for any footprint.
fn skirt(roof: &mut Flat, lower: &[Vec2], upper: &[Vec2], y0: f32, y1: f32, col: Rgb) {
    let n = lower.len();
    for i in 0..n {
        let j = (i + 1) % n;
        // sides facing the light a little brighter, as on a real roof
        let k = 0.9 + 0.16 * outward(lower[i], lower[j]).dot(Vec2::new(0.6, 0.8));
        roof.tint = Some(rgb(col, k));
        for t in [[v3(lower[i], y0), v3(lower[j], y0), v3(upper[j], y1)], [v3(lower[i], y0), v3(upper[j], y1), v3(upper[i], y1)]] {
            let t = if (t[1] - t[0]).cross(t[2] - t[0]).y > 0.0 { t } else { [t[0], t[2], t[1]] };
            roof.tri3(t, M_SPAN[M_ROOF]);
        }
    }
    roof.tint = None;
}

/// Working frame of a footprint: long axis (pointing the way `hint` does), extents along it
/// forwards (`f`) and backwards (`b`), and half-width (`w`).
struct Frame {
    c: Vec2,
    ax: Vec2,
    sd: Vec2,
    f: f32,
    b: f32,
    w: f32,
}

impl Frame {
    fn new(l: &Landmark, hint: Vec2) -> Frame {
        let c = l.c;
        // principal axis of the outline, each wall weighted by its length
        let (mut xx, mut xz, mut zz) = (0.0f32, 0.0f32, 0.0f32);
        let n = l.pts.len();
        for i in 0..n {
            let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
            let (d, len) = ((a + b) * 0.5 - c, a.distance(b));
            xx += d.x * d.x * len;
            xz += d.x * d.y * len;
            zz += d.y * d.y * len;
        }
        let mut ax = Vec2::from_angle(0.5 * (2.0 * xz).atan2(xx - zz));
        // snap to the direction of the longest wall near that axis, so parts sit square to the building
        let mut best = 0.0;
        for i in 0..n {
            let e = l.pts[(i + 1) % n] - l.pts[i];
            let d = e.normalize_or_zero();
            if d.dot(ax).abs() > 0.94 && e.length() > best {
                best = e.length();
                ax = if d.dot(ax) < 0.0 { -d } else { d };
            }
        }
        if ax.dot(hint) < 0.0 {
            ax = -ax;
        }
        let sd = Vec2::new(-ax.y, ax.x);
        let (mut f, mut b, mut w) = (0.0f32, 0.0f32, 0.0f32);
        for p in &l.pts {
            let d = *p - c;
            f = f.max(d.dot(ax));
            b = b.max(-d.dot(ax));
            w = w.max(d.dot(sd).abs());
        }
        Frame { c, ax, sd, f, b, w }
    }
    fn at(&self, a: f32, s: f32) -> Vec2 {
        self.c + self.ax * a + self.sd * s
    }
}

/// Temple front: a row of columns carrying an entablature and a pediment, facing along the axis
/// at distance `a`, `half` wide. Returns the height of the pediment's apex.
fn portico(m: &mut MB, rf: &mut Flat, g: f32, fr: &Frame, a: f32, depth: f32, half: f32, cols: i32, r: f32, top: f32, col: Rgb) -> f32 {
    // steps
    for (k, d) in [(0.0f32, 2.4f32), (1.0, 1.6), (2.0, 0.8)] {
        obox(m, fr.c, fr.ax, a - depth, a + d, -half - d, half + d, 0.0, 0.3 + k * 0.3, GRANITE);
    }
    for k in 0..cols {
        let s = -half + r + (2.0 * half - 2.0 * r) * k as f32 / (cols - 1) as f32;
        column(m, fr.at(a - r - 0.2, s), r, 0.9, top, col);
    }
    // entablature, then the pediment as a short gabled roof with its front wall in stone
    obox(m, fr.c, fr.ax, a - depth, a + 0.15, -half - 0.2, half + 0.2, top, top + 1.5, col);
    let rise = half * 0.36;
    gable(m, rf, g, fr.c, fr.sd, -half - 0.4, half + 0.4, -a - 0.35, -(a - depth), top + 1.5, rise, 0.0, TIN, col);
    top + 1.5 + rise
}

// ---- the landmarks ------------------------------------------------------------

/// Builds landmark `l` into the cell's meshes.
pub fn add_landmark(l: &Landmark, mb: &mut MB, roof: &mut Flat, glow: &mut MB, lights: &mut Vec<LightSite>) {
    let g = terrain(l.c);
    mb.lift = g;
    let (c, h) = (l.c, l.h);
    let n = l.pts.len();
    let radius = if n > 0 { l.pts.iter().map(|p| p.distance(c)).sum::<f32>() / n as f32 } else { 0.0 };
    let mut flood = |p: Vec2, y: f32, color: Rgb, power: f32| lights.push(LightSite { pos: Vec3::new(p.x, g + y, p.y), color, power });
    let warm = [1.0, 0.85, 0.6];
    match l.kind {
        1 => {
            // ---- Vilnius Cathedral: Neoclassical temple. Six-column Doric portico with three
            // saints on the pediment, colonnades down both flanks, a tin roof, and the domed
            // chapels at the east end.
            let fr = Frame::new(l, Vec2::new(-1.0, 0.0));
            walls(mb, &l.pts, 0.0, h, T_WHITE, STONE);
            band(mb, &l.pts, 0.0, 1.1, 0.25, GRANITE);
            band(mb, &l.pts, h - 1.6, h - 1.0, 0.3, STONE);
            band(mb, &l.pts, h - 0.4, h, 0.45, STONE);
            windows(mb, &l.pts, 6.0, 5.2, 1.5, 4.5, 12.5, 1, GLASS);
            cap(roof, &l.pts, &l.tris, g + h, TIN);
            // flank colonnades: columns in front of every long side wall, under their own eaves
            for i in 0..n {
                let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
                let len = a.distance(b);
                let d = (b - a) / len.max(0.01);
                if len < 16.0 || d.dot(fr.ax).abs() < 0.9 {
                    continue;
                }
                let out = outward(a, b);
                let k = (len / 3.8).round() as i32;
                for j in 0..=k {
                    column(mb, a + d * (len * j as f32 / k as f32) + out * 2.0, 0.6, 1.1, h * 0.78, STONE);
                }
                let (a2, b2) = (a + out * 2.75, b + out * 2.75);
                mb.wall(a2, b2, h * 0.78, h * 0.78 + 1.2, T_WHITE, rgb(STONE, 1.0));
                mb.flat_quad([a, a2, b2, b], h * 0.78 + 1.2, T_WHITE, rgb(TIN, 1.1));
                mb.flat_quad([a, a2, b2, b], 1.1, T_WHITE, rgb(GRANITE, 1.2));
                mb.wall(a2, b2, 0.0, 1.1, T_WHITE, rgb(GRANITE, 0.9));
            }
            // main roof, a low pitch along the nave
            gable(mb, roof, g, c, fr.ax, -fr.b + 5.0, fr.f - 4.0, -fr.w * 0.62, fr.w * 0.62, h, 3.2, 0.0, TIN, STONE);
            // west front
            let half = (fr.w * 0.7).min(13.0);
            let apex = portico(mb, roof, g, &fr, fr.f + 0.5, 5.5, half, 6, 0.95, h * 0.8, STONE);
            // St. Casimir, St. Stanislaus and St. Helena with her gilded cross
            statue(mb, fr.at(fr.f - 0.2, 0.0), apex, 4.2, STONE);
            cross(mb, fr.at(fr.f - 0.2, 0.9), apex + 1.2, 4.0);
            for s in [-1.0f32, 1.0] {
                statue(mb, fr.at(fr.f - 0.2, s * (half - 0.4)), h * 0.8 + 1.5, 3.4, STONE);
            }
            // the door wall behind the columns: tall doorway and niches
            for s in [-1.0f32, 0.0, 1.0] {
                window(mb, fr.at(fr.f - 5.1, s * half * 0.5), -fr.sd, if s == 0.0 { 2.6 } else { 1.5 }, 1.0, if s == 0.0 { 9.0 } else { 7.0 }, 1, [0.12, 0.08, 0.05]);
            }
            // east end: St. Casimir's Chapel and its twin, each under a dome with a lantern
            for s in [-1.0f32, 1.0] {
                let p = fr.at(-fr.b + 5.5, s * (fr.w - 5.0));
                mb.prism(p, 4.6, 4.6, h, h + 2.6, 8, PI / 8.0, T_WHITE, T_WHITE, rgb(STONE, 0.97));
                ring_windows(mb, p, 4.6, 8, PI / 8.0, 1.0, h + 0.5, h + 2.2, 1, GLASS);
                dome(mb, p, 4.8, h + 2.6, 4.0, 14, COPPER);
                mb.prism(p, 0.9, 0.9, h + 6.4, h + 8.0, 8, 0.0, T_WHITE, T_WHITE, rgb(STONE, 1.0));
                dome(mb, p, 1.0, h + 8.0, 0.9, 8, COPPER);
                cross(mb, p, h + 8.9, 1.8);
            }
            flood(fr.at(fr.f + 9.0, 0.0), 5.0, warm, 1.2);
            flood(fr.at(0.0, fr.w + 8.0), 6.0, warm, 0.8);
        }
        2 => {
            // ---- Gediminas' Tower: the surviving tower of the Upper Castle. Three storeys of red
            // brick on a fieldstone base, pointed windows, battlements, and the flag.
            let rot = PI / 8.0;
            let r = radius.max(6.0);
            lathe(mb, c, &[(r * 1.1, -6.0), (r * 1.08, 0.0), (r * 1.03, 3.2)], 8, rot, T_WHITE, [0.5, 0.46, 0.4]);
            lathe(mb, c, &[(r, 3.2), (r * 0.97, 8.6), (r * 0.95, 14.2), (r * 0.93, h)], 8, rot, T_WHITE, BRICK);
            for y in [8.6f32, 14.2] {
                lathe(mb, c, &[(r * 1.0, y - 0.2), (r * 1.0, y + 0.25)], 8, rot, T_WHITE, [0.7, 0.36, 0.26]);
            }
            for (y0, y1, rr) in [(4.4f32, 7.4f32, r * 0.99), (10.0, 13.0, r * 0.965), (15.4, 18.4, r * 0.945)] {
                ring_windows(mb, c, rr, 8, rot, 1.25, y0, y1, 2, GLASS);
            }
            // battlements: parapet wall, walkway, merlons
            lathe(mb, c, &[(r * 0.96, h), (r * 0.99, h + 0.3), (r * 0.99, h + 1.3), (r * 0.86, h + 1.3)], 8, rot, T_WHITE, BRICK);
            mb.prism(c, r * 0.87, r * 0.87, h + 0.4, h + 0.45, 8, rot, T_WHITE, T_WHITE, [0.3, 0.27, 0.25, 1.0]);
            for k in 0..16 {
                let a = rot - k as f32 / 16.0 * TAU;
                let p = c + Vec2::from_angle(a) * r * 0.93;
                mb.prism(p, 0.55, 0.55, h + 1.3, h + 2.3, 4, a + PI / 4.0, T_WHITE, T_WHITE, rgb(BRICK, 1.05));
            }
            // stair turret and the flagpole with the tricolour
            mb.prism(c, 1.6, 1.6, h + 0.4, h + 3.2, 8, 0.0, T_WHITE, T_WHITE, rgb(BRICK, 0.9));
            mb.prism(c, 1.75, 0.1, h + 3.2, h + 4.6, 8, 0.0, T_WHITE, T_WHITE, rgb(TILE, 1.0));
            mb.prism(c, 0.1, 0.07, h + 4.6, h + 14.0, 6, 0.0, T_METAL, T_METAL, WHITE);
            for (k, fc) in [[0.99, 0.73, 0.07], [0.0, 0.42, 0.27], [0.76, 0.15, 0.18]].iter().enumerate() {
                let (y1, y0) = (h + 14.0 - k as f32 * 0.85, h + 13.15 - k as f32 * 0.85);
                mb.wall(c, c + Vec2::new(4.2, 0.0), y0, y1, T_WHITE, rgb(*fc, 1.0));
                mb.wall(c + Vec2::new(4.2, 0.0), c, y0, y1, T_WHITE, rgb(*fc, 1.0));
            }
            // doorway on the west side
            window(mb, c + Vec2::new(-r * 1.04 * (PI / 8.0).cos(), 0.0), Vec2::new(0.0, 1.0), 1.5, 0.2, 3.0, 2, [0.1, 0.07, 0.05]);
            flood(c + Vec2::new(-r - 6.0, 0.0), 4.0, warm, 1.0);
            flood(c + Vec2::new(0.0, r + 6.0), 4.0, warm, 1.0);
        }
        3 => {
            // ---- Cathedral bell tower: a round defensive tower of the Lower Castle, raised with
            // octagonal Baroque and Classical tiers; clock faces on four sides, spire and cross.
            let r = radius.max(5.5);
            let rot = PI / 8.0;
            let (t1, t2, t3) = (h * 0.36, h * 0.62, h * 0.84);
            lathe(mb, c, &[(r * 1.04, -6.0), (r * 1.04, 0.0), (r, 1.2), (r * 0.97, t1)], 18, 0.0, T_WHITE, [0.86, 0.83, 0.76]);
            ring_windows(mb, c, r * 0.985, 6, 0.4, 0.7, t1 * 0.45, t1 * 0.45 + 1.8, 0, GLASS);
            // tiled offset where the round tower turns octagonal
            lathe(mb, c, &[(r * 1.06, t1 - 0.3), (r * 1.06, t1), (r * 0.82, t1 + 1.3)], 16, 0.0, T_WHITE, TILE);
            let tiers = [(r * 0.8, t1 + 1.3, t2), (r * 0.68, t2 + 0.5, t3), (r * 0.55, t3 + 0.5, h)];
            for (i, (rr, y0, y1)) in tiers.iter().enumerate() {
                mb.prism(c, *rr, rr * 0.97, *y0, *y1, 8, rot, T_WHITE, T_WHITE, rgb(PLASTER, 1.0));
                lathe(mb, c, &[(rr * 1.0, y1 - 0.05), (rr * 1.1, y1 + 0.1), (rr * 1.1, y1 + 0.5), (rr * 0.9, y1 + 0.5)], 8, rot, T_WHITE, STONE);
                let w = rr * 0.38;
                if i == 1 {
                    // the clock
                    for k in 0..4 {
                        let dir = Vec2::from_angle(rot - (2.0 * k as f32 + 0.5) / 8.0 * TAU);
                        let mid = c + dir * (rr * (PI / 8.0).cos() + 0.08);
                        let d = Vec2::new(dir.y, -dir.x);
                        let (ym, cr) = ((y0 + y1) * 0.5, rr * 0.3);
                        let out3 = Vec3::new(dir.x, 0.0, dir.y);
                        for (rad, cc, off) in [(cr * 1.15, [0.1, 0.1, 0.12, 1.0], 0.0), (cr, [0.95, 0.94, 0.88, 1.0], 0.03)] {
                            let ctr = v3(mid + dir * off, ym);
                            for j in 0..12 {
                                let p = |j: i32| {
                                    let t = j as f32 / 12.0 * TAU;
                                    v3(mid + dir * off + d * (t.cos() * rad), ym + t.sin() * rad)
                                };
                                quad_out(mb, [ctr, p(j), p(j + 1), p(j + 1)], out3, cc);
                            }
                        }
                        // hands at ten past ten
                        let hand = |mb: &mut MB, ang: f32, len: f32| {
                            let tip = v3(mid + dir * 0.06 + d * (ang.sin() * len), ym + ang.cos() * len);
                            let b0 = v3(mid + dir * 0.06 + d * (ang.cos() * 0.07), ym - ang.sin() * 0.07);
                            let b1 = v3(mid + dir * 0.06 - d * (ang.cos() * 0.07), ym + ang.sin() * 0.07);
                            quad_out(mb, [b0, b1, tip, tip], out3, [0.08, 0.08, 0.1, 1.0]);
                        };
                        hand(mb, -1.0, cr * 0.55);
                        hand(mb, 1.05, cr * 0.85);
                    }
                } else {
                    ring_windows(mb, c, rr * 0.985, 8, rot, w, y0 + (y1 - y0) * 0.22, y0 + (y1 - y0) * 0.85, 1, GLASS);
                }
            }
            // cupola, spire and cross
            let rr = r * 0.55;
            dome(mb, c, rr * 0.98, h + 0.5, rr * 0.75, 12, COPPER);
            lathe(mb, c, &[(rr * 0.3, h + 0.5 + rr * 0.7), (rr * 0.26, h + 2.4 + rr * 0.7), (rr * 0.34, h + 2.6 + rr * 0.7), (0.06, h + 8.0 + rr * 0.7)], 8, rot, T_WHITE, COPPER);
            cross(mb, c, h + 8.0 + rr * 0.7, 3.0);
            flood(c + Vec2::new(0.0, r + 6.0), 5.0, warm, 1.0);
            flood(c + Vec2::new(-r - 6.0, 0.0), 12.0, warm, 0.8);
        }
        4 => {
            // ---- Europa Tower: the tallest office building in the Baltics. Blue glass, floor
            // bands, a set-back crown and a mast.
            let tint = [0.75, 0.95, 1.15];
            walls(mb, &l.pts, 0.0, 5.0, T_GLASS, [0.5, 0.6, 0.7]);
            band(mb, &l.pts, 4.6, 5.2, 0.5, CONCRETE);
            walls(mb, &l.pts, 5.2, h, T_WALL, tint);
            let mut y = 8.4;
            while y < h - 1.0 {
                band(mb, &l.pts, y, y + 0.22, 0.1, [0.82, 0.86, 0.9]);
                y += 3.2;
            }
            // vertical fins at the corners of the plan
            for i in 0..n {
                let (p, q, s) = (l.pts[(i + n - 1) % n], l.pts[i], l.pts[(i + 1) % n]);
                if (q - p).normalize_or_zero().dot((s - q).normalize_or_zero()) < 0.5 {
                    let o = (q - c).normalize_or_zero();
                    mb.prism(q + o * 0.15, 0.3, 0.3, 0.0, h + 1.2, 4, 0.0, T_WHITE, T_WHITE, [0.85, 0.88, 0.92, 1.0]);
                }
            }
            band(mb, &l.pts, h - 0.3, h + 0.9, 0.15, CONCRETE);
            cap(roof, &l.pts, &l.tris, g + h, [0.3, 0.32, 0.38]);
            let c1 = scaled(&l.pts, c, 0.74);
            walls(mb, &c1, h, h + 4.5, T_WALL, tint);
            band(mb, &c1, h + 4.3, h + 4.8, 0.12, CONCRETE);
            cap(roof, &c1, &l.tris, g + h + 4.5, [0.3, 0.32, 0.38]);
            let c2 = scaled(&l.pts, c, 0.42);
            walls(mb, &c2, h + 4.5, h + 8.0, T_WALL, tint);
            cap(roof, &c2, &l.tris, g + h + 8.0, [0.3, 0.32, 0.38]);
            lathe(mb, c, &[(1.1, h + 8.0), (0.7, h + 11.0), (0.28, h + 17.0), (0.08, h + 24.0)], 8, 0.0, T_METAL, [0.9, 0.9, 0.95]);
            glow.ground(c.x - 2.5, c.y - 2.5, c.x + 2.5, c.y + 2.5, g + h + 24.2, T_GLOW, 0, [0.9, 0.08, 0.05, 1.0]);
            flood(c + Vec2::new(0.0, radius + 8.0), 10.0, [0.5, 0.8, 1.0], 1.2);
        }
        5 => {
            // ---- Town Hall: Laurynas Gucevičius' Classical block with a six-column Doric portico
            // and pediment on the square, under a hipped tin roof.
            let fr = Frame::new(l, Vec2::new(0.0, -1.0));
            walls(mb, &l.pts, 0.0, h, T_WHITE, PLASTER);
            band(mb, &l.pts, 0.0, 1.0, 0.2, GRANITE);
            band(mb, &l.pts, h * 0.5, h * 0.5 + 0.3, 0.15, STONE);
            band(mb, &l.pts, h - 0.9, h, 0.4, STONE);
            windows(mb, &l.pts, 5.0, 3.6, 1.3, 2.0, h * 0.5 - 0.8, 0, GLASS);
            windows(mb, &l.pts, 5.0, 3.6, 1.3, h * 0.5 + 1.2, h - 2.0, 1, GLASS);
            let up = scaled(&l.pts, c, 0.5);
            skirt(roof, &scaled(&l.pts, c, 1.03), &up, g + h, g + h + 4.0, TIN);
            cap(roof, &up, &l.tris, g + h + 4.0, TIN);
            let half = (fr.w * 0.5).clamp(6.0, 11.0);
            let apex = portico(mb, roof, g, &fr, fr.f + 4.0, 4.2, half, 6, 0.75, h * 0.86, PLASTER);
            window(mb, fr.at(fr.f + 0.05, 0.0), -fr.sd, 2.0, 1.0, 5.2, 1, [0.14, 0.09, 0.05]);
            mb.prism(fr.at(fr.f + 1.5, 0.0), 0.08, 0.06, apex, apex + 5.0, 5, 0.0, T_METAL, T_METAL, WHITE);
            flood(fr.at(fr.f + 12.0, 0.0), 4.0, warm, 1.2);
        }
        6 => {
            // ---- St. Anne's: Flamboyant brick Gothic. Buttressed nave with lancet windows under a
            // steep tiled roof, and the famous west front: three openwork towers crowned with
            // crocketed spires.
            let fr = Frame::new(l, Vec2::new(-1.0, 0.0));
            walls(mb, &l.pts, 0.0, h, T_WHITE, BRICK);
            band(mb, &l.pts, 0.0, 0.8, 0.18, [0.4, 0.2, 0.16]);
            band(mb, &l.pts, h - 0.5, h, 0.22, [0.66, 0.3, 0.22]);
            windows(mb, &l.pts, 3.4, 3.4, 1.0, 2.6, h - 1.6, 2, GLASS);
            // buttress with a pinnacle at every corner of the plan
            for i in 0..n {
                let (p, q, s) = (l.pts[(i + n - 1) % n], l.pts[i], l.pts[(i + 1) % n]);
                if q.distance(p) < 1.5 && q.distance(s) < 1.5 {
                    continue;
                }
                let o = (outward(p, q) + outward(q, s)).normalize_or_zero();
                let b = q + o * 0.5;
                mb.prism(b, 0.6, 0.5, 0.0, h * 0.7, 4, o.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(BRICK, 0.9));
                mb.prism(b, 0.45, 0.4, h * 0.7, h + 0.6, 4, o.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(BRICK, 1.0));
                mb.prism(b, 0.42, 0.04, h + 0.6, h + 3.2, 4, o.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(BRICK, 1.15));
            }
            cap(roof, &l.pts, &l.tris, g + h, TILE);
            let hw = fr.w * 0.78;
            let rise = (hw * 1.05).min(8.5);
            gable(mb, roof, g, c, fr.ax, -fr.b + 1.5, fr.f - 2.5, -hw, hw, h, rise, 0.0, TILE, BRICK);
            // ridge turret
            let rt = fr.at(-fr.b * 0.35, 0.0);
            let ridge = h + rise;
            mb.prism(rt, 0.7, 0.6, ridge - 1.0, ridge + 1.6, 6, 0.0, T_WHITE, T_WHITE, rgb(COPPER, 0.9));
            mb.prism(rt, 0.75, 0.04, ridge + 1.6, ridge + 6.0, 6, 0.0, T_WHITE, T_WHITE, rgb(COPPER, 1.0));
            cross(mb, rt, ridge + 6.0, 1.4);
            // west front
            let fa = fr.f - 1.6;
            for (s, top, r) in [(-1.0f32, 27.0f32, 1.9f32), (0.0, 33.0, 2.2), (1.0, 27.0, 1.9)] {
                let p = fr.at(fa + if s == 0.0 { 0.9 } else { 0.0 }, s * fr.w * 0.72);
                let rot = fr.ax.to_angle() + PI / 8.0;
                let y1 = top - 12.5;
                mb.prism(p, r, r * 0.95, 0.0, y1, 8, rot, T_WHITE, T_WHITE, rgb(BRICK, 1.0));
                ring_windows(mb, p, r * 0.99, 8, rot, r * 0.42, 2.0, y1 * 0.5, 2, GLASS);
                ring_windows(mb, p, r * 0.97, 8, rot, r * 0.42, y1 * 0.56, y1 - 0.8, 2, GLASS);
                // gallery, open belfry stage, spire
                lathe(mb, p, &[(r * 0.96, y1), (r * 1.2, y1 + 0.3), (r * 1.2, y1 + 0.7), (r * 0.8, y1 + 0.7)], 8, rot, T_WHITE, [0.68, 0.3, 0.22]);
                mb.prism(p, r * 0.78, r * 0.72, y1 + 0.7, y1 + 4.6, 8, rot, T_WHITE, T_WHITE, rgb(BRICK, 1.05));
                ring_windows(mb, p, r * 0.76, 8, rot, r * 0.34, y1 + 1.2, y1 + 4.2, 2, GLASS);
                lathe(mb, p, &[(r * 0.86, y1 + 4.6), (r * 0.86, y1 + 5.0), (r * 0.62, y1 + 5.0), (0.05, top)], 8, rot, T_WHITE, [0.66, 0.28, 0.2]);
                cross(mb, p, top, 1.6);
                // four pinnacles round the foot of each spire
                for k in 0..4 {
                    let q = p + Vec2::from_angle(rot + PI / 8.0 + k as f32 * PI / 2.0) * r * 1.02;
                    mb.prism(q, 0.3, 0.26, y1 + 0.7, y1 + 3.0, 4, 0.0, T_WHITE, T_WHITE, rgb(BRICK, 1.0));
                    mb.prism(q, 0.3, 0.03, y1 + 3.0, y1 + 5.6, 4, 0.0, T_WHITE, T_WHITE, rgb(BRICK, 1.15));
                }
            }
            // screen wall between the towers: portal, great window, traceried gable
            let half = fr.w * 0.72 - 1.9;
            obox(mb, c, fr.ax, fa - 0.6, fa + 0.5, -half, half, 0.0, h + 3.0, BRICK);
            window(mb, fr.at(fa + 0.5, 0.0), -fr.sd, 1.9, 0.2, 4.6, 2, [0.12, 0.08, 0.05]);
            for s in [-1.0f32, 1.0] {
                window(mb, fr.at(fa + 0.5, s * half * 0.62), -fr.sd, 1.1, 0.2, 3.6, 2, [0.12, 0.08, 0.05]);
                window(mb, fr.at(fa + 0.5, s * half * 0.55), -fr.sd, 0.9, 6.0, h + 1.5, 2, GLASS);
            }
            flood(fr.at(fr.f + 9.0, 0.0), 4.0, warm, 1.3);
            flood(fr.at(0.0, fr.w + 7.0), 5.0, warm, 0.7);
        }
        7 => {
            // ---- St. Peter and St. Paul's: Baroque basilica on a Latin-cross plan. Twin towers with
            // helmets flank a columned front; a great dome and lantern rise over the crossing.
            let fr = Frame::new(l, Vec2::new(-1.0, 0.3));
            walls(mb, &l.pts, 0.0, h, T_WHITE, PLASTER);
            band(mb, &l.pts, 0.0, 1.0, 0.2, GRANITE);
            band(mb, &l.pts, h - 1.0, h, 0.4, STONE);
            band(mb, &l.pts, h * 0.52, h * 0.52 + 0.3, 0.15, STONE);
            windows(mb, &l.pts, 4.5, 4.4, 1.3, 2.4, h * 0.5 - 1.0, 1, GLASS);
            windows(mb, &l.pts, 4.5, 4.4, 1.5, h * 0.52 + 1.4, h - 2.2, 1, GLASS);
            cap(roof, &l.pts, &l.tris, g + h, TILE);
            // nave and transept roofs
            let cross_a = -fr.b * 0.28;
            gable(mb, roof, g, c, fr.ax, -fr.b + 2.0, fr.f - 5.0, -fr.w * 0.5, fr.w * 0.5, h, 4.5, 0.0, TILE, PLASTER);
            gable(mb, roof, g, c, fr.sd, -fr.w + 1.0, fr.w - 1.0, -cross_a - fr.w * 0.42, -cross_a + fr.w * 0.42, h, 4.5, 0.0, TILE, PLASTER);
            // dome on an octagonal drum, with lantern
            let dp = fr.at(cross_a, 0.0);
            let dr = (fr.w * 0.42).clamp(5.0, 7.5);
            mb.prism(dp, dr, dr, h + 1.0, h + 7.0, 8, PI / 8.0, T_WHITE, T_WHITE, rgb(PLASTER, 1.0));
            ring_windows(mb, dp, dr * 0.995, 8, PI / 8.0, dr * 0.3, h + 2.4, h + 6.0, 1, GLASS);
            lathe(mb, dp, &[(dr, h + 6.9), (dr * 1.08, h + 7.1), (dr * 1.08, h + 7.5), (dr * 0.98, h + 7.5)], 16, 0.0, T_WHITE, STONE);
            dome(mb, dp, dr, h + 7.5, dr * 0.9, 16, COPPER);
            let ly = h + 7.5 + dr * 0.86;
            mb.prism(dp, dr * 0.24, dr * 0.24, ly, ly + 2.4, 8, 0.0, T_WHITE, T_WHITE, rgb(PLASTER, 1.0));
            ring_windows(mb, dp, dr * 0.24, 8, 0.0, dr * 0.09, ly + 0.3, ly + 2.0, 1, GLASS);
            dome(mb, dp, dr * 0.27, ly + 2.4, dr * 0.25, 8, COPPER);
            cross(mb, dp, ly + 2.4 + dr * 0.25, 2.2);
            // west towers: square shafts, octagonal belfries, Baroque helmets
            let fa = fr.f - 3.6;
            let ts = (fr.w * 0.6).clamp(6.0, 9.0);
            for s in [-1.0f32, 1.0] {
                let p = fr.at(fa, s * ts);
                obox(mb, p, fr.ax, -3.4, 3.4, -3.4, 3.4, 0.0, h + 1.5, PLASTER);
                for y in [h * 0.52f32, h + 1.5] {
                    obox(mb, p, fr.ax, -3.65, 3.65, -3.65, 3.65, y - 0.35, y, STONE);
                }
                window(mb, p + fr.ax * 3.42, -fr.sd, 1.2, 3.0, 7.0, 1, GLASS);
                window(mb, p + fr.ax * 3.42, -fr.sd, 1.2, h * 0.52 + 1.5, h - 0.5, 1, GLASS);
                let rot = fr.ax.to_angle() + PI / 8.0;
                mb.prism(p, 3.1, 3.0, h + 1.5, h + 7.5, 8, rot, T_WHITE, T_WHITE, rgb(PLASTER, 1.0));
                ring_windows(mb, p, 3.08, 8, rot, 1.0, h + 2.6, h + 6.6, 1, GLASS);
                lathe(
                    mb,
                    p,
                    &[(3.0, h + 7.4), (3.35, h + 7.6), (3.35, h + 8.0), (3.0, h + 8.0), (2.9, h + 9.0), (2.0, h + 10.4), (1.2, h + 11.0), (1.1, h + 12.2), (1.4, h + 12.5), (1.0, h + 13.4), (0.1, h + 15.0)],
                    8,
                    rot,
                    T_WHITE,
                    COPPER,
                );
                cross(mb, p, h + 15.0, 2.0);
            }
            // centre of the front: paired columns on two storeys, balcony, scrolled gable with a cross
            let half = ts - 3.4;
            for s in [-1.0f32, 1.0] {
                for d in [0.55f32, 0.95] {
                    column(mb, fr.at(fr.f + 0.7, s * half * d), 0.5, 1.0, h * 0.5, PLASTER);
                    column(mb, fr.at(fr.f + 0.7, s * half * d), 0.42, h * 0.5 + 0.9, h - 0.6, PLASTER);
                }
            }
            obox(mb, c, fr.ax, fr.f - 0.3, fr.f + 1.5, -half - 0.3, half + 0.3, h * 0.5, h * 0.5 + 0.9, STONE);
            obox(mb, c, fr.ax, fr.f - 0.3, fr.f + 1.5, -half - 0.3, half + 0.3, h - 0.6, h + 0.3, STONE);
            gable(mb, roof, g, c, fr.sd, -half - 0.3, half + 0.3, -(fr.f + 1.5), -(fr.f - 0.6), h + 0.3, half * 0.5, 0.0, TILE, PLASTER);
            cross(mb, fr.at(fr.f + 0.4, 0.0), h + 0.3 + half * 0.5, 2.4);
            window(mb, fr.at(fr.f + 0.05, 0.0), -fr.sd, 2.2, 1.0, 6.4, 1, [0.14, 0.09, 0.05]);
            window(mb, fr.at(fr.f + 0.05, 0.0), -fr.sd, 2.0, h * 0.5 + 1.6, h - 1.6, 1, GLASS);
            flood(fr.at(fr.f + 10.0, 0.0), 5.0, warm, 1.3);
            flood(fr.at(cross_a, fr.w + 8.0), 6.0, warm, 0.8);
        }
        8 => {
            // ---- Gate of Dawn: the last surviving city gate. Passage through the middle, the
            // chapel's three tall windows on the town side, a Renaissance attic on the outside.
            let fr = Frame::new(l, Vec2::new(0.0, 1.0));
            walls(mb, &l.pts, 0.0, h, T_WHITE, PLASTER);
            band(mb, &l.pts, 0.0, 0.9, 0.18, GRANITE);
            band(mb, &l.pts, h * 0.5, h * 0.5 + 0.3, 0.18, STONE);
            band(mb, &l.pts, h - 0.6, h, 0.3, STONE);
            let up = scaled(&l.pts, c, 0.45);
            skirt(roof, &scaled(&l.pts, c, 1.04), &up, g + h, g + h + 3.4, TILE);
            cap(roof, &up, &l.tris, g + h + 3.4, TILE);
            // gateway arch at both ends of the passage
            for (a, dir, town) in [(fr.f, 1.0f32, false), (-fr.b, -1.0, true)] {
                let mid = fr.at(a + dir * 0.02, 0.0);
                let d = fr.sd * -dir;
                window(mb, mid, d, 3.6, 0.0, 5.6, 1, [0.03, 0.03, 0.04]);
                if town {
                    // chapel of Our Lady: three arched windows framed by pilasters, pediment above
                    for s in [-1.0f32, 0.0, 1.0] {
                        window(mb, mid + d * s * 2.6, d, 1.5, h * 0.5 + 1.0, h - 1.6, 1, [0.5, 0.42, 0.2]);
                    }
                    for s in [-2.0f32, -1.0, 1.0, 2.0] {
                        let p = mid + d * s * 1.3 * if s.abs() > 1.5 { 1.55 } else { 1.0 } + fr.ax * dir * 0.12;
                        mb.prism(p, 0.24, 0.2, h * 0.5 + 0.3, h - 0.7, 4, fr.ax.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(STONE, 1.05));
                    }
                } else {
                    // outer face: attic with a frieze of roundels, the Vytis panel over the arch
                    obox(mb, c, fr.ax, a - 0.5, a + 0.25, -fr.w, fr.w, h, h + 2.6, PLASTER);
                    for k in 0..5 {
                        let s = (k as f32 - 2.0) / 2.0 * fr.w * 0.74;
                        window(mb, fr.at(a + 0.27, s), d, 0.9, h + 0.8, h + 1.7, 1, [0.25, 0.2, 0.18]);
                    }
                    window(mb, mid + fr.ax * dir * 0.05, d, 1.8, 6.6, 8.6, 0, [0.6, 0.15, 0.12]);
                }
            }
            cross(mb, c, h + 3.4, 1.8);
            flood(fr.at(-fr.b - 8.0, 0.0), 4.0, warm, 1.2);
        }
        9 => {
            // ---- Opera and Ballet Theatre (1974): glass walls behind a rhythm of slender piers,
            // a deep roof slab, the fly tower above, all on a granite terrace.
            let terrace = scaled(&l.pts, c, 1.1);
            walls(mb, &terrace, 0.0, 1.4, T_WHITE, GRANITE);
            cap(roof, &terrace, &l.tris, g + 1.4, [0.5, 0.5, 0.54]);
            let y1 = h - 3.0;
            walls(mb, &l.pts, 1.4, y1, T_GLASS, [0.75, 0.6, 0.42]);
            for i in 0..n {
                let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
                let len = a.distance(b);
                if len < 2.0 {
                    continue;
                }
                let d = (b - a) / len;
                let out = outward(a, b);
                let k = (len / 2.6).round().max(1.0) as i32;
                for j in 0..=k {
                    let p = a + d * (len * j as f32 / k as f32) + out * 0.55;
                    mb.prism(p, 0.42, 0.42, 1.4, y1, 4, d.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(STONE, 1.0));
                }
            }
            // warm light from the foyers
            let slab = scaled(&l.pts, c, 1.05);
            walls(mb, &slab, y1, h, T_WHITE, STONE);
            band(mb, &slab, y1, y1 + 0.35, 0.2, [0.75, 0.72, 0.66]);
            cap(roof, &slab, &l.tris, g + h, [0.4, 0.4, 0.44]);
            let fly = scaled(&l.pts, c, 0.42);
            walls(mb, &fly, h, h + 8.0, T_WHITE, [0.36, 0.33, 0.3]);
            band(mb, &fly, h + 7.5, h + 8.0, 0.2, [0.5, 0.47, 0.42]);
            cap(roof, &fly, &l.tris, g + h + 8.0, [0.3, 0.3, 0.33]);
            for k in 0..4 {
                let p = c + Vec2::from_angle(k as f32 * PI / 2.0 + 0.4) * (radius + 6.0);
                flood(p, 5.0, [1.0, 0.75, 0.45], 0.9);
            }
        }
        10 => {
            // ---- Palace of the Grand Dukes: the rebuilt Renaissance residence. Cream walls with
            // three rows of windows, steep red-tiled roofs, dormers and a corner tower.
            let fr = Frame::new(l, Vec2::new(-1.0, 0.0));
            walls(mb, &l.pts, 0.0, h, T_WHITE, CREAM);
            band(mb, &l.pts, 0.0, 1.0, 0.2, [0.6, 0.57, 0.5]);
            band(mb, &l.pts, h - 0.7, h, 0.35, PLASTER);
            for (y0, y1, arch) in [(2.0f32, 4.6f32, 0u8), (6.6, 9.8, 0), (11.8, 15.4, 1)] {
                windows(mb, &l.pts, 4.0, 3.3, 1.25, y0, y1, arch, GLASS);
                band(mb, &l.pts, y0 - 0.75, y0 - 0.5, 0.1, PLASTER);
            }
            let up = scaled(&l.pts, c, 0.8);
            skirt(roof, &scaled(&l.pts, c, 1.015), &up, g + h, g + h + 5.5, TILE);
            cap(roof, &up, &l.tris, g + h + 5.5, [0.5, 0.2, 0.14]);
            // dormers on the long roofs
            for i in 0..n {
                let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
                let len = a.distance(b);
                if len < 14.0 {
                    continue;
                }
                let d = (b - a) / len;
                let inw = -outward(a, b);
                let k = (len / 7.0).floor() as i32;
                for j in 0..k {
                    let p = a + d * (len * (j as f32 + 0.5) / k as f32) + inw * 1.6;
                    obox(mb, p, d, -0.8, 0.8, -0.9, 0.9, h + 0.6, h + 2.4, CREAM);
                    gable(mb, roof, g, p, inw, -1.0, 1.4, -0.95, 0.95, h + 2.4, 0.9, 0.0, TILE, CREAM);
                    window(mb, p - inw * 0.91, d, 0.8, h + 1.0, h + 2.1, 0, GLASS);
                }
            }
            // corner tower with a pyramid roof
            let far = l.pts.iter().copied().fold(c, |best, p| if (p - c).dot(fr.ax + fr.sd) > (best - c).dot(fr.ax + fr.sd) { p } else { best });
            let tp = far + (c - far).normalize_or_zero() * 4.0;
            obox(mb, tp, fr.ax, -4.2, 4.2, -4.2, 4.2, 0.0, h + 6.0, CREAM);
            obox(mb, tp, fr.ax, -4.45, 4.45, -4.45, 4.45, h + 5.5, h + 6.0, PLASTER);
            for dir in [fr.ax, -fr.ax, fr.sd, -fr.sd] {
                window(mb, tp + dir * 4.22, Vec2::new(dir.y, -dir.x), 1.3, h + 1.6, h + 4.6, 1, GLASS);
            }
            mb.prism(tp, 6.2, 0.15, h + 6.0, h + 13.0, 4, fr.ax.to_angle() + PI / 4.0, T_WHITE, T_WHITE, rgb(TILE, 1.0));
            mb.prism(tp, 0.08, 0.05, h + 13.0, h + 16.0, 5, 0.0, T_METAL, T_METAL, WHITE);
            flood(fr.at(fr.f + 8.0, 0.0), 5.0, warm, 1.0);
            flood(fr.at(0.0, fr.w + 8.0), 5.0, warm, 1.0);
            flood(fr.at(0.0, -fr.w - 8.0), 5.0, warm, 0.8);
        }
        11 => {
            // ---- TV Tower: 326 m in life, the tallest structure in Lithuania. Low round base
            // building, tapering concrete shaft, the saucer with the revolving restaurant,
            // equipment decks, and a red-and-white mast.
            lathe(mb, c, &[(13.0, -6.0), (13.0, 0.0), (13.0, 3.2)], 24, 0.0, T_GLASS, [0.6, 0.75, 0.9]);
            lathe(mb, c, &[(13.5, 3.2), (13.5, 3.9), (6.5, 5.2)], 24, 0.0, T_WHITE, [0.4, 0.41, 0.45]);
            for k in 0..12 {
                // ribs on the base building
                let a = k as f32 / 12.0 * TAU;
                mb.prism(c + Vec2::from_angle(a) * 13.0, 0.5, 0.5, 0.0, 3.9, 4, a + PI / 4.0, T_WHITE, T_WHITE, rgb(CONCRETE, 1.05));
            }
            lathe(mb, c, &[(6.2, 5.0), (5.2, 10.0), (4.4, 16.0), (3.9, 22.0), (3.6, 27.0)], 20, 0.0, T_WHITE, CONCRETE);
            for k in 0..6 {
                // strips of windows up the shaft
                let dir = Vec2::from_angle(k as f32 / 6.0 * TAU + 0.3);
                for j in 0..7 {
                    let y = 7.0 + j as f32 * 2.8;
                    let r = 6.2 - (y - 5.0) * 0.118 + 0.02;
                    window(mb, c + dir * r, Vec2::new(dir.y, -dir.x), 0.45, y, y + 1.5, 0, GLASS);
                }
            }
            // saucer: flared underside, band of glass, roof deck
            lathe(mb, c, &[(3.6, 27.0), (6.4, 27.9), (9.3, 29.3)], 28, 0.0, T_WHITE, [0.6, 0.6, 0.64]);
            lathe(mb, c, &[(9.3, 29.3), (9.5, 29.6)], 28, 0.0, T_WHITE, CONCRETE);
            lathe(mb, c, &[(9.5, 29.6), (9.5, 31.4)], 28, 0.0, T_GLASS, [0.7, 0.9, 1.1]);
            lathe(mb, c, &[(9.6, 31.4), (9.6, 31.8), (8.8, 32.0), (4.2, 32.8)], 28, 0.0, T_WHITE, CONCRETE);
            lathe(mb, c, &[(3.2, 32.8), (2.6, 39.0)], 14, 0.0, T_WHITE, CONCRETE);
            for y in [34.4f32, 36.6] {
                lathe(mb, c, &[(2.9, y), (4.2, y + 0.05), (4.2, y + 0.4), (2.9, y + 0.45)], 14, 0.0, T_WHITE, [0.55, 0.55, 0.6]);
            }
            for k in 0..7 {
                let (y0, r0) = (39.0 + k as f32 * 2.6, 1.5 - k as f32 * 0.18);
                let col = if k % 2 == 0 { [0.82, 0.1, 0.08] } else { [0.94, 0.94, 0.94] };
                lathe(mb, c, &[(r0, y0), (r0 - 0.18, y0 + 2.6)], 8, 0.0, T_WHITE, col);
            }
            lathe(mb, c, &[(0.12, 57.2), (0.05, 61.0)], 5, 0.0, T_METAL, [0.9, 0.9, 0.9]);
            // aircraft warning lights
            glow.ground(c.x - 2.5, c.y - 2.5, c.x + 2.5, c.y + 2.5, g + 57.4, T_GLOW, 0, [0.9, 0.06, 0.04, 1.0]);
            for k in 0..6 {
                let p = c + Vec2::from_angle(k as f32 / 6.0 * TAU) * 9.6;
                glow.ground(p.x - 1.2, p.y - 1.2, p.x + 1.2, p.y + 1.2, g + 32.0, T_GLOW, 0, [0.7, 0.05, 0.03, 1.0]);
            }
            flood(c + Vec2::new(0.0, 24.0), 4.0, [0.7, 0.85, 1.0], 1.2);
            flood(c + Vec2::new(0.0, 12.0), 30.5, [1.0, 0.8, 0.5], 1.0);
        }
        _ => {
            // ---- Three Crosses: white concrete monument on the Bleak Hill, the middle cross
            // tallest, on a stepped plinth. (Turned to face the camera; the real ones look west.)
            let white = [0.96, 0.96, 0.94];
            for (k, (hx, hz)) in [(10.5f32, 4.4f32), (9.6, 3.6), (8.8, 2.8)].iter().enumerate() {
                let (y0, y1) = (k as f32 * 0.45 - if k == 0 { 6.0 } else { 0.0 }, (k + 1) as f32 * 0.45);
                mb.boxx(Vec3::new(c.x - hx, y0, c.y - hz), Vec3::new(c.x + hx, y1, c.y + hz), T_WHITE, T_WHITE, rgb(GRANITE, 1.2 + k as f32 * 0.15));
            }
            // low wall joining the feet of the crosses
            mb.boxx(Vec3::new(c.x - 7.4, 1.35, c.y - 0.7), Vec3::new(c.x + 7.4, 3.2, c.y + 0.7), T_WHITE, T_WHITE, rgb(white, 0.95));
            for (k, top) in [(-1.0f32, 13.0f32), (0.0, 15.5), (1.0, 13.0)] {
                let x = c.x + k * 5.9;
                // shaft and arms taper slightly, as cast
                mb.boxx(Vec3::new(x - 0.6, 1.35, c.y - 0.6), Vec3::new(x + 0.6, top * 0.55, c.y + 0.6), T_WHITE, T_WHITE, rgb(white, 0.97));
                mb.boxx(Vec3::new(x - 0.5, top * 0.55, c.y - 0.5), Vec3::new(x + 0.5, top, c.y + 0.5), T_WHITE, T_WHITE, rgb(white, 1.0));
                let ay = top - 3.9;
                mb.boxx(Vec3::new(x - 2.7, ay, c.y - 0.5), Vec3::new(x + 2.7, ay + 1.0, c.y + 0.5), T_WHITE, T_WHITE, rgb(white, 1.0));
                mb.boxx(Vec3::new(x - 2.9, ay - 0.1, c.y - 0.56), Vec3::new(x - 2.6, ay + 1.1, c.y + 0.56), T_WHITE, T_WHITE, rgb(white, 0.93));
                mb.boxx(Vec3::new(x + 2.6, ay - 0.1, c.y - 0.56), Vec3::new(x + 2.9, ay + 1.1, c.y + 0.56), T_WHITE, T_WHITE, rgb(white, 0.93));
                mb.boxx(Vec3::new(x - 0.56, top - 0.3, c.y - 0.56), Vec3::new(x + 0.56, top, c.y + 0.56), T_WHITE, T_WHITE, rgb(white, 0.93));
            }
            flood(c + Vec2::new(-5.0, 7.0), 3.0, [1.0, 0.96, 0.88], 1.3);
            flood(c + Vec2::new(5.0, 7.0), 3.0, [1.0, 0.96, 0.88], 1.3);
        }
    }
}

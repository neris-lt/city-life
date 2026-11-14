//! Procedural pixel-art atlas + mesh building helpers. Everything in the game
//! is drawn from one 512x512 atlas that is painted in code at startup.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const TS: usize = 64; // tile size in pixels (patterns are authored at 32 and refined)
pub const AT: usize = 16; // atlas tiles per row
pub const ASIZE: usize = TS * AT;

// ---- tile ids -------------------------------------------------------------
pub const T_WHITE: u32 = 0;
pub const T_ASPHALT: u32 = 1;
pub const T_LANE: u32 = 2;
pub const T_CROSS: u32 = 3;
pub const T_SIDEWALK: u32 = 4;
pub const T_CONCRETE: u32 = 5;
pub const T_GRASS: u32 = 6;
pub const T_WATER: u32 = 7;
pub const T_PLAZA: u32 = 8;
pub const T_ROOF: u32 = 9;
pub const T_ROOF_VENT: u32 = 10;
pub const T_ROOF_SKY: u32 = 11;
pub const T_ROOF_PANEL: u32 = 12;
pub const T_PAINT: u32 = 13;
pub const T_GLASS: u32 = 14;
pub const T_HEADLIGHT: u32 = 15;
pub const T_TAILLIGHT: u32 = 16;
pub const T_POLICE: u32 = 17;
pub const T_TREE: u32 = 18;
pub const T_CONTAINER: u32 = 19;
pub const T_METAL: u32 = 20;
pub const T_PARKING: u32 = 21;
pub const T_SHOP: u32 = 24; // 4 shopfront variants
pub const T_TYRE: u32 = 28;
pub const T_LEAF: u32 = 29; // 29, 30 canopy variants
pub const T_WALL: u32 = 32; // 6 styles x 4 variants
pub const T_SIGN: u32 = 64; // 16 signs
pub const T_GLOW: u32 = 80;
pub const T_CONE: u32 = 81;
pub const T_SOLID: u32 = 82;
pub const T_RING: u32 = 83;
pub const T_STREAK: u32 = 84;
pub const T_STRIP: u32 = 85; // 8 neon colours
pub const T_ARROW: u32 = 93;
pub const T_MARKER: u32 = 94;

pub const NEON: [[f32; 3]; 8] = [
    [0.10, 0.90, 1.00], // cyan
    [1.00, 0.15, 0.70], // magenta
    [1.00, 0.50, 0.10], // orange
    [0.50, 1.00, 0.20], // lime
    [0.60, 0.30, 1.00], // violet
    [1.00, 0.10, 0.15], // red
    [1.00, 0.85, 0.20], // yellow
    [0.70, 0.85, 1.00], // ice
];

// ---- hashing / rng --------------------------------------------------------
pub fn hash2(x: i32, y: i32, s: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x85eb_ca6b)
        ^ (y as u32).wrapping_mul(0xc2b2_ae35)
        ^ s.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

pub fn hf(x: i32, y: i32, s: u32) -> f32 {
    (hash2(x, y, s) & 0xffff) as f32 / 65535.0
}

pub fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = x - xi;
    let fy = y - yi;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hf(xi, yi, seed);
    let b = hf(xi + 1, yi, seed);
    let c = hf(xi, yi + 1, seed);
    let d = hf(xi + 1, yi + 1, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

#[derive(Clone)]
pub struct Rng(pub u32);
impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x.wrapping_mul(0x9e37_79b1)
    }
    pub fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn int(&mut self, n: u32) -> u32 {
        (self.next() >> 8) % n.max(1)
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
}

// ---- atlas painting -------------------------------------------------------
#[derive(Clone, Copy)]
struct Px {
    c: [f32; 3],
    e: [f32; 3],
    r: f32,
    m: f32,
}

fn g3(v: f32) -> [f32; 3] {
    [v, v, v]
}
fn mul(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn asphalt(n: f32) -> Px {
    let b = 0.050 + n * 0.035;
    Px { c: [b * 0.9, b * 0.95, b * 1.3], e: g3(0.0), r: 0.42 + n * 0.2, m: 0.0 }
}

fn roof(n: f32) -> Px {
    let b = 0.075 + n * 0.03;
    Px { c: [b, b * 0.95, b * 1.15], e: g3(0.0), r: 0.9, m: 0.0 }
}

fn wall(style: u32, variant: u32, x: i32, y: i32, n: f32) -> Px {
    let seed = 900 + style * 16 + variant;
    let mut p = Px { c: g3(0.1), e: g3(0.0), r: 0.8, m: 0.0 };
    match style {
        0 => {
            // glass curtain wall
            let (px, py) = (x / 8, y / 8);
            let frame = x % 8 == 0 || y % 8 == 0;
            let lit = hf(px, py, seed) < 0.3;
            p.c = if frame { [0.05, 0.06, 0.09] } else { [0.03, 0.07 + n * 0.02, 0.12] };
            p.r = if frame { 0.6 } else { 0.1 };
            if lit && !frame {
                let k = 0.5 + hf(px, py, seed + 1) * 0.5;
                p.e = mul([0.45, 0.85, 1.0], k * 0.8);
            }
        }
        1 | 2 | 5 => {
            // masonry with a 2x2 (or 3 wide) window grid
            let cols = if style == 5 { 3 } else { 2 };
            let cw = 32 / cols;
            let (wx, wy) = (x % cw, y % 16);
            let (px, py) = (x / cw, y / 16);
            let inwin = wx >= 3 && wx < cw - 3 && wy >= 4 && wy < 13;
            p.c = match style {
                1 => [0.13 + n * 0.02, 0.11, 0.16],
                2 => {
                    let brick = (y % 4 == 0) || ((x + (y / 4 % 2) * 4) % 8 == 0);
                    if brick { [0.07, 0.04, 0.04] } else { [0.17 + n * 0.04, 0.08, 0.07] }
                }
                _ => [0.11, 0.125 + n * 0.02, 0.12],
            };
            if inwin {
                let h = hf(px, py, seed);
                let lit = h < 0.45;
                p.c = [0.03, 0.04, 0.07];
                p.r = 0.15;
                if lit {
                    let col = match style {
                        1 => if h < 0.2 { [1.0, 0.45, 0.75] } else { [1.0, 0.8, 0.45] },
                        2 => [1.0, 0.72, 0.38],
                        _ => [0.4, 1.0, 0.8],
                    };
                    let blind = wy < 4 + (hf(px, py, seed + 3) * 6.0) as i32;
                    p.e = mul(col, if blind { 0.25 } else { 0.75 });
                }
            }
        }
        3 => {
            // corrugated industrial
            let s = if x % 4 < 2 { 0.13 } else { 0.09 };
            p.c = [s * 0.9, s, s * 1.15 + n * 0.02];
            p.r = 0.55;
            p.m = 0.5;
            if y > 26 {
                p.c = mul(p.c, 0.6);
            }
            if (12..20).contains(&x) && (6..11).contains(&y) && hf(0, 0, seed) < 0.5 {
                p.c = [0.03, 0.03, 0.03];
                p.e = mul([1.0, 0.55, 0.15], 0.8);
            }
        }
        _ => {
            // black tower with LED bands
            p.c = [0.035, 0.03, 0.05 + n * 0.01];
            p.r = 0.3;
            if y % 16 == 14 || y % 16 == 15 {
                let c = if variant % 2 == 0 { [1.0, 0.15, 0.7] } else { [0.5, 0.3, 1.0] };
                p.e = mul(c, 0.9);
            } else if x % 8 > 1 && x % 8 < 6 && y % 16 > 3 && y % 16 < 11 {
                if hf(x / 8, y / 16, seed) < 0.35 {
                    p.e = mul([0.6, 0.35, 1.0], 0.6);
                }
            }
        }
    }
    p
}

fn sign(k: u32, x: i32, y: i32) -> Px {
    let col = NEON[(k % 8) as usize];
    let mut p = Px { c: mul(col, 0.05), e: mul(col, 0.04), r: 0.4, m: 0.0 };
    let border = x == 1 || x == 30 || y == 5 || y == 26;
    if (1..=30).contains(&x) && (5..=26).contains(&y) && border {
        p.e = col;
    }
    // three pseudo-glyphs made of strokes on a 4x6 grid
    for gi in 0..3 {
        let gx = 4 + gi * 9;
        if x >= gx && x < gx + 7 && y >= 9 && y < 23 {
            let (lx, ly) = (x - gx, y - 9);
            let s = 500 + k * 8 + gi as u32;
            let horiz = ly % 4 == 0 && hf(ly / 4, 0, s) < 0.75;
            let vert = lx % 3 == 0 && hf(lx / 3, 1 + ly / 7, s) < 0.6;
            if horiz || vert {
                p.e = if k >= 8 { [1.0, 1.0, 1.0] } else { col };
                p.e = mul(p.e, 1.0);
            }
        }
    }
    if y < 5 || y > 26 {
        p.c = g3(0.03);
        p.e = g3(0.0);
    }
    p
}

pub const T_DIGIT: u32 = 96; // 0-9, then G, N, A

/// 5x7 LED-style characters for route numbers.
const FONT: [[u8; 7]; 13] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
    [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
    [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
    [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
];

fn glyph(k: u32, x: i32, y: i32) -> Px {
    let (gx, gy) = ((x - 6) / 4, (y - 2) / 4);
    let on = x >= 6 && y >= 2 && gx < 5 && gy < 7 && FONT[k as usize][gy as usize] >> (4 - gx) & 1 == 1;
    let lit = on && (x - 6) % 4 < 3 && (y - 2) % 4 < 3;
    Px { c: g3(0.015), e: if lit { [1.0, 0.62, 0.08] } else { g3(0.0) }, r: 0.5, m: 0.0 }
}

fn paint(t: u32, x: i32, y: i32) -> Px {
    let n = hf(x, y, t * 31 + 7);
    let fx = x as f32 - 15.5;
    let fy = y as f32 - 15.5;
    let d = (fx * fx + fy * fy).sqrt();
    let glow = |v: f32| Px { c: g3(0.0), e: g3(v.clamp(0.0, 1.0)), r: 1.0, m: 0.0 };
    match t {
        T_WHITE => Px { c: g3(1.0), e: g3(0.0), r: 0.8, m: 0.0 },
        T_ASPHALT => asphalt(n),
        T_LANE => {
            let mut p = asphalt(n);
            if y < 2 && n > 0.12 {
                p.c = [0.55, 0.42, 0.06];
            } else if y >= 30 && (x / 8) % 2 == 0 && n > 0.15 {
                p.c = g3(0.42);
            }
            p
        }
        T_CROSS => {
            let mut p = asphalt(n);
            if (x / 4) % 2 == 0 && (4..28).contains(&y) && n > 0.18 {
                p.c = g3(0.45);
            }
            p
        }
        22 | 23 => {
            let mut p = asphalt(n);
            let v = vnoise(x as f32 / 7.0, y as f32 / 7.0, t) + (1.0 - d / 15.0) * 0.5;
            if v > 0.95 && d < 14.5 {
                p.c = [0.012, 0.016, 0.03];
                p.r = 0.05;
            }
            p
        }
        T_SIDEWALK => {
            let grout = x % 16 == 0 || y % 16 == 0;
            let b = if grout { 0.07 } else { 0.15 + n * 0.03 };
            Px { c: [b, b, b * 1.2], e: g3(0.0), r: 0.5, m: 0.0 }
        }
        T_CONCRETE => {
            let b = 0.10 + n * 0.03 - if (x + y * 3) % 29 == 0 { 0.04 } else { 0.0 };
            Px { c: [b, b, b * 1.1], e: g3(0.0), r: 0.65, m: 0.0 }
        }
        T_GRASS => Px { c: [0.025, 0.09 + n * 0.06, 0.05 + n * 0.02], e: g3(0.0), r: 0.95, m: 0.0 },
        T_WATER => {
            let w = ((x as f32 * 0.3927 + (y as f32 * 0.7854).sin() * 1.5).sin() * 0.5 + 0.5).powi(6);
            Px { c: [0.008, 0.04, 0.06], e: mul([0.1, 0.7, 0.8], w * 0.12), r: 0.04, m: 0.0 }
        }
        T_PLAZA => {
            let line = x % 16 == 0 || y % 16 == 0;
            Px {
                c: if line { g3(0.02) } else { [0.06, 0.06 + n * 0.01, 0.09] },
                e: if line { mul([0.0, 0.8, 1.0], 0.35) } else { g3(0.0) },
                r: 0.2,
                m: 0.0,
            }
        }
        T_ROOF => roof(n),
        T_ROOF_VENT => {
            let mut p = roof(n);
            if (9..23).contains(&x) && (9..23).contains(&y) {
                let s = if y % 3 == 0 { 0.2 } else { 0.05 };
                p.c = [s, s, s * 1.1];
                p.m = 0.6;
                p.r = 0.4;
            }
            p
        }
        T_ROOF_SKY => {
            let mut p = roof(n);
            if (5..27).contains(&x) && (9..23).contains(&y) {
                let frame = x == 5 || x == 26 || y == 9 || y == 22 || x == 16;
                p.c = g3(0.04);
                if !frame {
                    p.e = mul([1.0, 0.75, 0.4], 0.35);
                }
            }
            p
        }
        T_ROOF_PANEL => {
            let frame = x % 16 < 1 || y % 8 < 1;
            Px {
                c: if frame { g3(0.12) } else { [0.02, 0.04, 0.12 + n * 0.02] },
                e: g3(0.0),
                r: 0.15,
                m: 0.3,
            }
        }
        T_PAINT => Px { c: g3(0.92 + n * 0.03), e: g3(0.0), r: 0.22, m: 0.4 },
        T_GLASS => {
            let hl = (x + y) % 13 < 2;
            Px { c: if hl { [0.15, 0.22, 0.3] } else { [0.03, 0.05, 0.08] }, e: g3(0.0), r: 0.05, m: 0.2 }
        }
        T_HEADLIGHT => Px { c: g3(1.0), e: [1.0, 0.95, 0.8], r: 0.3, m: 0.0 },
        T_TAILLIGHT => Px { c: [1.0, 0.1, 0.05], e: [1.0, 0.08, 0.04], r: 0.3, m: 0.0 },
        T_POLICE => {
            let c = if x < 16 { [1.0, 0.05, 0.05] } else { [0.1, 0.3, 1.0] };
            Px { c, e: c, r: 0.3, m: 0.0 }
        }
        T_TREE => {
            let v = vnoise(x as f32 / 4.0, y as f32 / 4.0, 5);
            let c = if v > 0.55 { [0.04, 0.22, 0.13] } else { [0.015, 0.10 + n * 0.03, 0.07] };
            Px { c, e: g3(0.0), r: 0.9, m: 0.0 }
        }
        T_CONTAINER => {
            let s = if x % 4 < 2 { 0.85 } else { 0.55 };
            let edge = x < 1 || x > 30 || y < 1 || y > 30;
            Px { c: g3(if edge { 0.3 } else { s - n * 0.1 }), e: g3(0.0), r: 0.5, m: 0.4 }
        }
        T_METAL => {
            let b = 0.17 + n * 0.05;
            Px { c: [b, b * 1.03, b * 1.15], e: g3(0.0), r: 0.45, m: 0.7 }
        }
        T_PARKING => {
            let b = if x < 2 && n > 0.1 { 0.4 } else { 0.085 + n * 0.03 };
            Px { c: [b, b, b * 1.1], e: g3(0.0), r: 0.55, m: 0.0 }
        }
        24..=27 => {
            // shopfront: lit glass with a coloured fascia
            let col = NEON[((t - 24) * 3 % 8) as usize];
            let mut p = Px { c: [0.05, 0.05, 0.07], e: g3(0.0), r: 0.5, m: 0.0 };
            if (2..7).contains(&y) {
                p.e = mul(col, 0.9);
            } else if (9..28).contains(&y) && x % 16 > 1 && x % 16 < 15 {
                let shelf = y % 6 == 0 || hf(x / 3, y / 6, t) < 0.25;
                p.c = [0.03, 0.04, 0.06];
                p.r = 0.1;
                p.e = if shelf { mul(col, 0.25) } else { mul([1.0, 0.86, 0.62], 0.55) };
            }
            p
        }
        T_TYRE => Px { c: g3(0.015 + n * 0.02), e: g3(0.0), r: 0.9, m: 0.0 },
        29 | 30 => {
            let v = vnoise(x as f32 / 3.0, y as f32 / 3.0, t) * 0.6 + n * 0.4;
            let c = if v > 0.62 {
                [0.07, 0.30, 0.16]
            } else if v > 0.4 {
                [0.03, 0.17, 0.10]
            } else {
                [0.012, 0.075, 0.06]
            };
            Px { c: if t == 30 { [c[0] * 1.6, c[1] * 0.95, c[2] * 0.7] } else { c }, e: g3(0.0), r: 0.85, m: 0.0 }
        }
        32..=55 => wall((t - 32) / 4, (t - 32) % 4, x, y, n),
        64..=79 => sign(t - 64, x, y),
        T_GLOW => glow((1.0 - d / 15.5).max(0.0).powi(2)),
        T_CONE => {
            let u = x as f32 / 31.0;
            let w = 2.0 + u * 13.0;
            glow((1.0 - fy.abs() / w).max(0.0) * (1.0 - u).powf(1.3))
        }
        T_SOLID => glow(1.0),
        T_RING => glow(if (d - 12.5).abs() < 1.3 { 1.0 } else { 0.0 }),
        T_STREAK => glow(if fx.abs() < 1.0 { y as f32 / 31.0 } else { 0.0 }),
        96..=108 => glyph(t - 96, x, y),
        85..=92 => {
            let c = NEON[(t - 85) as usize];
            Px { c: mul(c, 0.5), e: c, r: 0.4, m: 0.0 }
        }
        T_ARROW => glow(if x >= 6 && fy.abs() < (31 - x) as f32 * 0.55 && x < 30 { 1.0 } else { 0.0 }),
        T_MARKER => {
            let m = fx.abs().max(fy.abs());
            glow(if (m > 13.0 && m < 15.5) || (m > 6.0 && m < 8.0) { 1.0 } else if m < 13.0 { 0.12 } else { 0.0 })
        }
        _ => Px { c: [1.0, 0.0, 1.0], e: g3(0.0), r: 1.0, m: 0.0 },
    }
}

pub struct Atlas {
    pub base: Image,
    pub emissive: Image,
    pub mr: Image,
}

pub fn build_atlas() -> Atlas {
    let n = ASIZE * ASIZE * 4;
    let (mut base, mut emis, mut mr) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
    let to8 = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    for ty in 0..AT {
        for tx in 0..AT {
            let t = (ty * AT + tx) as u32;
            if t > 108 {
                continue;
            }
            for y in 0..TS {
                for x in 0..TS {
                    let mut p = paint(t, (x / 2) as i32, (y / 2) as i32);
                    if p.e == [0.0; 3] {
                        // fine grain at full resolution keeps surfaces from looking blocky up close
                        let g = 0.95 + 0.1 * hf(x as i32, y as i32, t + 4242);
                        p.c = mul(p.c, g);
                    }
                    let o = ((ty * TS + y) * ASIZE + tx * TS + x) * 4;
                    for k in 0..3 {
                        base[o + k] = to8(p.c[k]);
                        emis[o + k] = to8(p.e[k]);
                    }
                    base[o + 3] = 255;
                    emis[o + 3] = 255;
                    mr[o + 1] = (p.r.clamp(0.0, 1.0) * 255.0) as u8;
                    mr[o + 2] = (p.m.clamp(0.0, 1.0) * 255.0) as u8;
                    mr[o + 3] = 255;
                }
            }
        }
    }
    let mk = |data: Vec<u8>, fmt: TextureFormat| {
        let mut img = Image::new(
            Extent3d { width: ASIZE as u32, height: ASIZE as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            fmt,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::linear();
        img
    };
    Atlas {
        base: mk(base, TextureFormat::Rgba8UnormSrgb),
        emissive: mk(emis, TextureFormat::Rgba8UnormSrgb),
        mr: mk(mr, TextureFormat::Rgba8Unorm),
    }
}


// ---- world-space repeating ground textures --------------------------------
pub const M_ASPHALT: usize = 0;
pub const M_PAVING: usize = 1;
pub const M_CONCRETE: usize = 2;
pub const M_WATER: usize = 3;
pub const M_GRASS: usize = 4;
pub const M_ROOF: usize = 5;
/// Metres covered by one repeat of each tiled texture.
pub const M_SPAN: [f32; 6] = [16.0, 4.0, 4.0, 4.0, 8.0, 8.0];

fn pnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = x - xi;
    let fy = y - yi;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let w = |v: i32| v.rem_euclid(period);
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hf(w(xi), w(yi), seed);
    let b = hf(w(xi + 1), w(yi), seed);
    let c = hf(w(xi), w(yi + 1), seed);
    let d = hf(w(xi + 1), w(yi + 1), seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

fn tiled_px(kind: usize, x: i32, y: i32) -> Px {
    let n = hf(x, y, 77 + kind as u32);
    match kind {
        M_ASPHALT => {
            // fine-grained wet tarmac: aggregate speckle, tyre-polished bands, cracks, shallow puddles
            let grain = hf(x / 2, y / 2, 5) * 0.5 + n * 0.5;
            let b = 0.036 + grain * 0.03;
            let mut p = Px { c: [b * 0.92, b * 0.97, b * 1.28], e: g3(0.0), r: 0.36 + grain * 0.2, m: 0.0 };
            let patch = pnoise(x as f32 / 24.0, y as f32 / 24.0, 5, 31);
            if patch > 0.7 {
                p.c = mul(p.c, 0.88); // newer repair patch
                p.r -= 0.08;
            }
            let crack = pnoise(x as f32 / 5.0, y as f32 / 5.0, 25, 8);
            if (crack - 0.5).abs() < 0.012 {
                p.c = mul(p.c, 0.45);
            }
            let v = pnoise(x as f32 / 14.0, y as f32 / 14.0, 9, 3) * 0.75 + pnoise(x as f32 / 4.0, y as f32 / 4.0, 32, 4) * 0.25;
            if v > 0.78 {
                p.c = [0.03, 0.036, 0.055];
                p.r = 0.04;
            } else if v > 0.66 {
                p.c = mul(p.c, 0.8);
                p.r = 0.14;
            }
            if n > 0.985 {
                p.c = mul(p.c, 2.2);
            }
            p
        }
        M_PAVING => {
            // herringbone-ish small pavers with a worn middle
            let (bx, by) = (x / 4, y / 2);
            let off = if by % 2 == 0 { 0 } else { 2 };
            let joint = (x + off) % 4 == 0 || y % 2 == 0 && hf(bx, by, 3) < 0.5;
            let tone = hf((x + off) / 4, by, 6);
            let b = if joint { 0.075 } else { 0.105 + tone * 0.025 + n * 0.01 };
            let wet = pnoise(x as f32 / 8.0, y as f32 / 8.0, 4, 14) > 0.66;
            Px { c: if wet { [b * 0.6, b * 0.62, b * 0.8] } else { [b, b * 0.98, b * 1.16] }, e: g3(0.0), r: if wet { 0.1 } else { 0.5 }, m: 0.0 }
        }
        M_CONCRETE => {
            let slab = x % 32 == 0 || y % 32 == 0;
            let stain = pnoise(x as f32 / 8.0, y as f32 / 8.0, 4, 19);
            let b = if slab { 0.035 } else { 0.07 + n * 0.02 - (stain - 0.5).max(0.0) * 0.06 };
            let bay = y % 32 < 1 || (x % 16 == 0 && y % 32 < 22);
            if bay && n > 0.2 {
                return Px { c: g3(0.2), e: g3(0.0), r: 0.5, m: 0.0 };
            }
            Px { c: [b, b, b * 1.12], e: g3(0.0), r: if stain > 0.62 { 0.12 } else { 0.5 }, m: 0.0 }
        }
        M_WATER => paint(T_WATER, x % 32, y % 32),
        M_GRASS => {
            let v = pnoise(x as f32 / 9.0, y as f32 / 9.0, 7, 9);
            let fine = hf(x, y / 2, 13);
            let mut c = [0.014 + fine * 0.012, 0.045 + v * 0.045 + fine * 0.03, 0.03 + fine * 0.012];
            let mut r = 0.92;
            if v < 0.14 {
                // worn, damp patch
                c = [0.016 + fine * 0.01, 0.04 + fine * 0.02, 0.03];
                r = 0.45;
            } else if fine > 0.93 {
                c = [0.05, 0.16, 0.07]; // blade highlights
            } else if hf(x, y, 21) > 0.996 {
                c = [0.3, 0.26, 0.1]; // dandelions
            }
            Px { c, e: g3(0.0), r, m: 0.0 }
        }
        _ => {
            let mut p = roof(n);
            p.c = mul(p.c, 1.5);
            let (lx, ly) = (x % 32, y % 32);
            // bitumen sheet seams
            if ly == 0 || (lx == 0 && (y / 32) % 2 == 0) || (lx == 16 && (y / 32) % 2 == 1) {
                p.c = mul(p.c, 0.5);
            }
            let v = pnoise(x as f32 / 8.0, y as f32 / 8.0, 8, 12);
            if v > 0.7 {
                p.c = mul(p.c, 0.7);
                p.r = 0.12; // standing water
            } else if v < 0.3 {
                p.c = mul(p.c, 1.2);
            }
            p
        }
    }
}

fn tiled_images(kind: usize) -> (Image, Image) {
    use bevy::image::{ImageAddressMode, ImageSamplerDescriptor};
    let size: usize = (M_SPAN[kind] * 16.0) as usize;
    let (mut base, mut mr) = (vec![0u8; size * size * 4], vec![0u8; size * size * 4]);
    for y in 0..size {
        for x in 0..size {
            let mut p = tiled_px(kind, (x / 2) as i32, (y / 2) as i32);
            p.c = mul(p.c, 0.94 + 0.12 * hf(x as i32, y as i32, 9000 + kind as u32));
            p.c = mul(p.c, if kind == M_WATER { 1.0 } else { 1.7 });
            let o = (y * size + x) * 4;
            for k in 0..3 {
                base[o + k] = (p.c[k].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            }
            base[o + 3] = 255;
            mr[o + 1] = (p.r.clamp(0.0, 1.0) * 255.0) as u8;
            mr[o + 2] = (p.m.clamp(0.0, 1.0) * 255.0) as u8;
            mr[o + 3] = 255;
        }
    }
    let mk = |data: Vec<u8>, fmt: TextureFormat| {
        let mut img = Image::new(
            Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            fmt,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        img
    };
    (mk(base, TextureFormat::Rgba8UnormSrgb), mk(mr, TextureFormat::Rgba8Unorm))
}

/// Builder for flat, upward-facing geometry textured in world space.
#[derive(Default)]
pub struct Flat {
    p: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    c: Vec<[f32; 4]>,
    /// Colour multiplier applied to the triangles added next (None = white).
    pub tint: Option<[f32; 4]>,
    /// When set, every vertex is raised by the terrain height at its position.
    pub drape: Option<fn(Vec2) -> f32>,
}

impl Flat {
    pub fn is_empty(&self) -> bool {
        self.p.is_empty()
    }
    /// Triangle given in xz with "negative shoelace" winding (faces up).
    pub fn tri(&mut self, t: [Vec2; 3], y: f32, span: f32) {
        for v in t {
            self.p.push([v.x, y + self.drape.map_or(0.0, |f| f(v)), v.y]);
            self.uv.push([v.x / span, v.y / span]);
            self.c.push(self.tint.unwrap_or(WHITE));
        }
    }
    /// Flat triangle with a brightness per corner (terrain relief shading).
    pub fn tri_c(&mut self, t: [Vec2; 3], y: f32, span: f32, k: [f32; 3]) {
        for (v, k) in t.iter().zip(k) {
            self.p.push([v.x, y + self.drape.map_or(0.0, |f| f(*v)), v.y]);
            self.uv.push([v.x / span, v.y / span]);
            self.c.push([k, k, k, 1.0]);
        }
    }

    /// Draped triangle with a height offset and brightness per corner.
    pub fn tri_cy(&mut self, t: [Vec2; 3], ys: [f32; 3], span: f32, k: [f32; 3]) {
        for i in 0..3 {
            let v = t[i];
            self.p.push([v.x, ys[i] + self.drape.map_or(0.0, |f| f(v)), v.y]);
            self.uv.push([v.x / span, v.y / span]);
            self.c.push([k[i], k[i], k[i], 1.0]);
        }
    }

    /// Sloped or raised triangle; texture still mapped from above.
    pub fn tri3(&mut self, t: [Vec3; 3], span: f32) {
        for v in t {
            self.p.push([v.x, v.y + self.drape.map_or(0.0, |f| f(Vec2::new(v.x, v.z))), v.z]);
            self.uv.push([v.x / span, v.z / span]);
            self.c.push(self.tint.unwrap_or(WHITE));
        }
    }
    pub fn build(self) -> Mesh {
        let n = vec![[0.0, 1.0, 0.0]; self.p.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.p)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.c)
    }
}

// ---- mesh builder ---------------------------------------------------------
pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

pub fn rgb(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

#[derive(Default)]
pub struct MB {
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    c: Vec<[f32; 4]>,
    i: Vec<u32>,
    /// Constant height added to everything built next (an object standing on a hillside).
    pub lift: f32,
    /// When set, each vertex is raised by the terrain height at its own position instead.
    pub drape: Option<fn(Vec2) -> f32>,
}

impl MB {
    pub fn is_empty(&self) -> bool {
        self.i.is_empty()
    }

    /// Corners: a, b, c, d counter-clockwise seen from the front face.
    pub fn quad(&mut self, v: [Vec3; 4], tile: u32, rot: usize, col: [f32; 4]) {
        let (lift, drape) = (self.lift, self.drape);
        let v = v.map(|p| Vec3::new(p.x, p.y + lift + drape.map_or(0.0, |f| f(Vec2::new(p.x, p.z))), p.z));
        let base = self.p.len() as u32;
        let nrm = (v[1] - v[0]).cross(v[3] - v[0]).normalize_or_zero();
        let (tx, ty) = ((tile as usize % AT) as f32, (tile as usize / AT) as f32);
        let e = 0.6; // half-texel inset so smooth filtering never bleeds a neighbouring tile
        let s = ASIZE as f32;
        let (u0, u1) = ((tx * TS as f32 + e) / s, ((tx + 1.0) * TS as f32 - e) / s);
        let (v0, v1) = ((ty * TS as f32 + e) / s, ((ty + 1.0) * TS as f32 - e) / s);
        let uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
        for k in 0..4 {
            self.p.push(v[k].to_array());
            self.n.push(nrm.to_array());
            self.uv.push(uvs[(k + rot) % 4]);
            self.c.push(col);
        }
        self.i.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Horizontal quad facing up.
    pub fn ground(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, y: f32, tile: u32, rot: usize, col: [f32; 4]) {
        self.quad(
            [Vec3::new(x0, y, z0), Vec3::new(x0, y, z1), Vec3::new(x1, y, z1), Vec3::new(x1, y, z0)],
            tile,
            rot,
            col,
        );
    }

    /// Horizontal quad from arbitrary xz corners; winding is fixed up automatically.
    pub fn flat_quad(&mut self, mut q: [Vec2; 4], y: f32, tile: u32, col: [f32; 4]) {
        let mut a = 0.0;
        for i in 0..4 {
            let (p, n) = (q[i], q[(i + 1) % 4]);
            a += p.x * n.y - n.x * p.y;
        }
        if a > 0.0 {
            q.swap(1, 3);
        }
        self.quad(q.map(|p| Vec3::new(p.x, y, p.y)), tile, 0, col);
    }

    /// Vertical quad from a to b (xz), facing right-hand side of travel a->b... outward for CCW footprints.
    pub fn wall(&mut self, a: Vec2, b: Vec2, y0: f32, y1: f32, tile: u32, col: [f32; 4]) {
        self.quad(
            [Vec3::new(a.x, y1, a.y), Vec3::new(a.x, y0, a.y), Vec3::new(b.x, y0, b.y), Vec3::new(b.x, y1, b.y)],
            tile,
            0,
            col,
        );
    }

    /// Axis-aligned box without a bottom face.
    pub fn boxx(&mut self, min: Vec3, max: Vec3, side: u32, top: u32, col: [f32; 4]) {
        let (x0, z0, x1, z1) = (min.x, min.z, max.x, max.z);
        self.wall(Vec2::new(x0, z1), Vec2::new(x1, z1), min.y, max.y, side, col);
        self.wall(Vec2::new(x1, z1), Vec2::new(x1, z0), min.y, max.y, side, col);
        self.wall(Vec2::new(x1, z0), Vec2::new(x0, z0), min.y, max.y, side, col);
        self.wall(Vec2::new(x0, z0), Vec2::new(x0, z1), min.y, max.y, side, col);
        self.ground(x0, z0, x1, z1, max.y, top, 0, col);
    }

    /// N-sided upright prism (trees, tanks, bollards). `r1` lets the top taper.
    pub fn prism(&mut self, c: Vec2, r0: f32, r1: f32, y0: f32, y1: f32, sides: usize, rot: f32, side: u32, top: u32, col: [f32; 4]) {
        let ring = |r: f32, k: usize| {
            let a = rot - k as f32 / sides as f32 * std::f32::consts::TAU;
            c + Vec2::new(a.cos(), a.sin()) * r
        };
        for k in 0..sides {
            let (a0, b0) = (ring(r0, k), ring(r0, k + 1));
            let (a1, b1) = (ring(r1, k), ring(r1, k + 1));
            self.quad(
                [Vec3::new(a1.x, y1, a1.y), Vec3::new(a0.x, y0, a0.y), Vec3::new(b0.x, y0, b0.y), Vec3::new(b1.x, y1, b1.y)],
                side,
                0,
                col,
            );
            if k >= 1 && k + 1 < sides {
                let p0 = ring(r1, 0);
                // top cap as a fan; quad() with a doubled corner gives one triangle
                self.quad(
                    [Vec3::new(p0.x, y1, p0.y), Vec3::new(a1.x, y1, a1.y), Vec3::new(b1.x, y1, b1.y), Vec3::new(b1.x, y1, b1.y)],
                    top,
                    0,
                    col,
                );
            }
        }
    }

    pub fn build(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.p)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.n)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.c)
            .with_inserted_indices(Indices::U32(self.i))
    }
}

// ---- shared handles -------------------------------------------------------
pub const CAR_COLORS: [[f32; 3]; 8] = [
    [0.55, 0.05, 0.08],
    [0.05, 0.25, 0.55],
    [0.75, 0.75, 0.80],
    [0.06, 0.06, 0.08],
    [0.70, 0.45, 0.05],
    [0.05, 0.40, 0.35],
    [0.40, 0.10, 0.50],
    [0.80, 0.80, 0.20],
];
pub const POLICE_KIND: usize = 8;

#[derive(Resource)]
pub struct Art {
    pub mat: Handle<StandardMaterial>,
    pub glow: Handle<StandardMaterial>,
    pub tiled: Vec<Handle<StandardMaterial>>,
    /// Bridge decks: fades out while the player is underneath.
    pub deck: Handle<StandardMaterial>,
    /// Soft black overlay for contact shadows (alpha comes from vertex colour).
    pub shade: Handle<StandardMaterial>,
    /// Civilian cars: 8 colours x 3 body styles (index = colour + 8 * style).
    pub cars: Vec<Handle<Mesh>>,
    pub police_car: Handle<Mesh>,
    pub car_glow: Handle<Mesh>,
    pub police_glow: Handle<Mesh>,
    /// Each person has two meshes: the two halves of a walking stride.
    pub peds: Vec<[Handle<Mesh>; 2]>,
    pub player: [Handle<Mesh>; 2],
    pub bullet: Handle<Mesh>,
    pub puffs: Vec<Handle<Mesh>>, // 0 spark, 1 fire, 2 smoke, 3 flash, 4 spray
    pub blood: Handle<Mesh>,
    pub rain: Handle<Mesh>,
    pub ripple: Handle<Mesh>,
    pub pickups: Vec<Handle<Mesh>>,
    pub marker: Handle<Mesh>,
    pub arrow: Handle<Mesh>,
}

/// Adds a quad, flipping it if needed so it faces `out`.
fn face(m: &mut MB, q: [Vec3; 4], out: Vec3, tile: u32, col: [f32; 4]) {
    let n = (q[1] - q[0]).cross(q[3] - q[0]);
    let q = if n.dot(out) < 0.0 { [q[0], q[3], q[2], q[1]] } else { q };
    m.quad(q, tile, 0, col);
}

/// A closed body section between x0 and x1 whose top edge follows (x, y) profile points and whose
/// sides lean in from half-width `w0` at the bottom to `w1` at the top.
fn section(m: &mut MB, profile: &[(f32, f32)], y0: f32, w0: f32, w1: f32, tile: u32, top_tile: u32, col: [f32; 4], top_col: [f32; 4]) {
    for w in profile.windows(2) {
        let ((xa, ya), (xb, yb)) = (w[0], w[1]);
        // top surface
        face(m, [Vec3::new(xa, ya, -w1), Vec3::new(xa, ya, w1), Vec3::new(xb, yb, w1), Vec3::new(xb, yb, -w1)], Vec3::Y, top_tile, top_col);
        // both flanks
        for s in [-1.0f32, 1.0] {
            face(
                m,
                [Vec3::new(xa, ya, s * w1), Vec3::new(xa, y0, s * w0), Vec3::new(xb, y0, s * w0), Vec3::new(xb, yb, s * w1)],
                Vec3::Z * s,
                tile,
                col,
            );
        }
    }
    // end caps
    for (i, out) in [(0usize, -1.0f32), (profile.len() - 1, 1.0)] {
        let (x, y) = profile[i];
        face(m, [Vec3::new(x, y, -w1), Vec3::new(x, y0, -w0), Vec3::new(x, y0, w0), Vec3::new(x, y, w1)], Vec3::X * out, tile, col);
    }
}

/// Car body styles: 0 saloon, 1 hatchback, 2 van / SUV.
fn car_mesh(col: [f32; 3], police: bool, style: u32) -> Mesh {
    let mut m = MB::default();
    let body = rgb(col, 1.0);
    let sill = rgb(col, 0.5);
    let trim = [0.05, 0.05, 0.06, 1.0];
    let chrome = [0.75, 0.78, 0.85, 1.0];
    let glass = [0.75, 0.85, 1.0, 1.0];
    let (len, wid, deck, roof) = match style {
        2 => (2.35, 1.02, 1.12, 1.92),
        1 => (2.05, 0.95, 0.98, 1.5),
        _ => (2.25, 0.96, 0.98, 1.48),
    };
    // wheels with hubcaps
    for x in [-len * 0.62, len * 0.6] {
        for s in [-1.0f32, 1.0] {
            let z = s * (wid + 0.02);
            m.boxx(Vec3::new(x - 0.4, 0.0, z - 0.14), Vec3::new(x + 0.4, 0.7, z + 0.14), T_TYRE, T_TYRE, WHITE);
            m.boxx(Vec3::new(x - 0.2, 0.18, z + s * 0.155 - 0.02), Vec3::new(x + 0.2, 0.52, z + s * 0.155 + 0.02), T_METAL, T_METAL, chrome);
        }
    }
    // sills and bumpers
    m.boxx(Vec3::new(-len, 0.22, -wid), Vec3::new(len, 0.6, wid), T_PAINT, T_PAINT, sill);
    m.boxx(Vec3::new(-len - 0.1, 0.28, -wid + 0.08), Vec3::new(-len + 0.06, 0.55, wid - 0.08), T_METAL, T_METAL, trim);
    m.boxx(Vec3::new(len - 0.06, 0.28, -wid + 0.08), Vec3::new(len + 0.1, 0.55, wid - 0.08), T_METAL, T_METAL, trim);
    // main body: boot, shoulder line, bonnet falling away to the nose
    let nose = if style == 2 { deck - 0.1 } else { deck - 0.18 };
    let (cab_f, cab_r) = match style {
        2 => (len * 0.42, -len + 0.12),
        1 => (len * 0.3, -len + 0.18),
        _ => (len * 0.3, -len * 0.55),
    };
    section(
        &mut m,
        &[(-len, deck - 0.06), (cab_r, deck), (cab_f, deck), (len - 0.25, nose), (len, nose - 0.14)],
        0.6,
        wid,
        wid - 0.05,
        T_PAINT,
        T_PAINT,
        body,
        body,
    );
    // glasshouse: raked windscreen, roof, rear screen; glass flanks
    let (ws, rs) = match style {
        2 => (0.45, 0.2),
        1 => (0.62, 0.45),
        _ => (0.62, 0.55),
    };
    section(
        &mut m,
        &[(cab_r, deck), (cab_r + rs, roof), (cab_f - ws, roof), (cab_f, deck)],
        deck,
        wid - 0.07,
        wid - 0.2,
        T_GLASS,
        T_GLASS,
        glass,
        glass,
    );
    // painted roof panel and pillars sit just proud of the glass
    let roofc = if police { [0.92, 0.92, 0.96, 1.0] } else { body };
    m.boxx(Vec3::new(cab_r + rs + 0.02, roof - 0.04, -wid + 0.19), Vec3::new(cab_f - ws - 0.02, roof + 0.03, wid - 0.19), T_PAINT, T_PAINT, roofc);
    let mid = (cab_r + rs + cab_f - ws) * 0.5;
    for s in [-1.0f32, 1.0] {
        // B-pillar
        face(
            &mut m,
            [Vec3::new(mid - 0.07, roof, s * (wid - 0.19)), Vec3::new(mid - 0.07, deck, s * (wid - 0.06)), Vec3::new(mid + 0.07, deck, s * (wid - 0.06)), Vec3::new(mid + 0.07, roof, s * (wid - 0.19))],
            Vec3::Z * s,
            T_PAINT,
            sill,
        );
        // door mirrors
        let (za, zb) = (s * wid, s * (wid + 0.24));
        m.boxx(Vec3::new(cab_f - 0.18, deck - 0.02, za.min(zb)), Vec3::new(cab_f + 0.02, deck + 0.14, za.max(zb)), T_PAINT, T_PAINT, sill);
    }
    // lamps, grille, plates
    for s in [-1.0f32, 1.0] {
        let (z0, z1) = if s < 0.0 { (-wid + 0.06, -wid + 0.5) } else { (wid - 0.5, wid - 0.06) };
        m.boxx(Vec3::new(len - 0.12, nose - 0.36, z0), Vec3::new(len + 0.04, nose - 0.12, z1), T_HEADLIGHT, T_HEADLIGHT, WHITE);
        m.boxx(Vec3::new(-len - 0.04, deck - 0.36, z0), Vec3::new(-len + 0.1, deck - 0.1, z1), T_TAILLIGHT, T_TAILLIGHT, WHITE);
    }
    m.boxx(Vec3::new(len - 0.02, nose - 0.4, -0.4), Vec3::new(len + 0.05, nose - 0.2, 0.4), T_METAL, T_METAL, trim);
    m.boxx(Vec3::new(-len - 0.06, 0.6, -0.28), Vec3::new(-len + 0.02, 0.78, 0.28), T_WHITE, T_WHITE, [0.85, 0.85, 0.8, 1.0]);
    if style == 2 {
        // roof rails
        for s in [-1.0f32, 1.0] {
            m.boxx(Vec3::new(cab_r + 0.5, roof + 0.03, s * (wid - 0.3) - 0.03), Vec3::new(cab_f - 0.7, roof + 0.1, s * (wid - 0.3) + 0.03), T_METAL, T_METAL, chrome);
        }
    }
    if police {
        m.boxx(Vec3::new(mid - 0.18, roof + 0.03, -0.62), Vec3::new(mid + 0.18, roof + 0.22, 0.62), T_POLICE, T_POLICE, WHITE);
        // white doors and bonnet stripe
        for s in [-1.0f32, 1.0] {
            face(
                &mut m,
                [Vec3::new(cab_r + 0.2, deck - 0.03, s * (wid + 0.005)), Vec3::new(cab_r + 0.2, 0.62, s * (wid + 0.005)), Vec3::new(cab_f - 0.1, 0.62, s * (wid + 0.005)), Vec3::new(cab_f - 0.1, deck - 0.03, s * (wid + 0.005))],
                Vec3::Z * s,
                T_PAINT,
                [0.92, 0.92, 0.96, 1.0],
            );
        }
    }
    m.build()
}

/// A 12 m city bus / trolleybus with its route number in lights on the roof.
pub fn bus_mesh(name: &str, kind: u8) -> Mesh {
    let mut m = MB::default();
    let (body, roofc) = match kind {
        1 => ([0.6, 0.06, 0.07, 1.0], [0.85, 0.85, 0.9, 1.0]), // trolleybus: red
        2 => ([0.05, 0.07, 0.25, 1.0], [0.2, 0.22, 0.4, 1.0]), // night bus: deep blue
        _ => ([0.1, 0.5, 0.25, 1.0], [0.85, 0.9, 0.86, 1.0]),  // bus: green and white
    };
    let (l, w) = (6.0, 1.28);
    for x in [-3.9f32, 3.6] {
        for z in [-w, w] {
            m.boxx(Vec3::new(x - 0.5, 0.0, z - 0.18), Vec3::new(x + 0.5, 0.95, z + 0.18), T_TYRE, T_TYRE, WHITE);
        }
    }
    m.boxx(Vec3::new(-l, 0.35, -w), Vec3::new(l, 1.45, w), T_PAINT, T_PAINT, body);
    m.boxx(Vec3::new(-l + 0.1, 1.45, -w + 0.05), Vec3::new(l - 0.1, 2.5, w - 0.05), T_GLASS, T_GLASS, WHITE);
    m.boxx(Vec3::new(-l, 2.5, -w), Vec3::new(l, 3.0, w), T_PAINT, T_PAINT, roofc);
    // roof plant
    m.boxx(Vec3::new(-l + 0.6, 3.0, -0.8), Vec3::new(-l + 2.6, 3.3, 0.8), T_METAL, T_ROOF_VENT, WHITE);
    m.boxx(Vec3::new(l - 2.2, 3.0, -0.7), Vec3::new(l - 0.9, 3.25, 0.7), T_METAL, T_METAL, WHITE);
    if kind == 1 {
        // trolley poles reaching back up to the wires
        for z in [-0.35f32, 0.35] {
            m.quad(
                [Vec3::new(-0.5, 3.1, z - 0.06), Vec3::new(-0.5, 3.1, z + 0.06), Vec3::new(-l - 1.2, 5.2, z + 0.06), Vec3::new(-l - 1.2, 5.2, z - 0.06)],
                T_METAL,
                0,
                [0.2, 0.2, 0.22, 1.0],
            );
        }
    }
    // route number, LED style, readable from above
    let chars: Vec<u32> = name
        .bytes()
        .filter_map(|c| match c {
            b'0'..=b'9' => Some((c - b'0') as u32),
            b'G' => Some(10),
            b'N' => Some(11),
            b'A' => Some(12),
            _ => None,
        })
        .take(4)
        .collect();
    let cw = 1.35;
    let x0 = -(chars.len() as f32) * cw * 0.5 + 0.4;
    m.ground(x0 - 0.2, -1.05, x0 + chars.len() as f32 * cw + 0.2, 1.05, 3.02, T_GLASS, 0, [0.2, 0.2, 0.2, 1.0]);
    for (i, c) in chars.iter().enumerate() {
        let x = x0 + i as f32 * cw;
        m.ground(x, -0.95, x + cw, 0.95, 3.04, T_DIGIT + c, 0, WHITE);
    }
    for (z0, z1) in [(-1.15f32, -0.55f32), (0.55, 1.15)] {
        m.boxx(Vec3::new(l - 0.05, 0.6, z0), Vec3::new(l + 0.06, 1.0, z1), T_HEADLIGHT, T_HEADLIGHT, WHITE);
        m.boxx(Vec3::new(-l - 0.06, 0.7, z0), Vec3::new(-l + 0.05, 1.2, z1), T_TAILLIGHT, T_TAILLIGHT, WHITE);
    }
    m.build()
}

/// A person, 1.8 m tall, facing +x. `frame` 0/1 are the two halves of a walking stride.
fn ped_mesh(cloth: [f32; 3], head: [f32; 3], frame: u32) -> Mesh {
    let mut m = MB::default();
    let s = if frame == 0 { 1.0 } else { -1.0 };
    let top = rgb(cloth, 1.0);
    let dark = rgb(cloth, 0.55);
    let legs = [0.07 + cloth[2] * 0.15, 0.08 + cloth[0] * 0.1, 0.12 + cloth[1] * 0.12, 1.0];
    let skin = [0.62, 0.44, 0.34, 1.0];
    let hair = rgb(head, 0.8);
    // legs and shoes, one forward one back
    for (side, fwd) in [(-1.0f32, s), (1.0, -s)] {
        let (z, x) = (side * 0.13, fwd * 0.2);
        m.boxx(Vec3::new(x - 0.1, 0.1, z - 0.09), Vec3::new(x + 0.1, 0.92, z + 0.09), T_WHITE, T_WHITE, legs);
        m.boxx(Vec3::new(x - 0.1, 0.0, z - 0.1), Vec3::new(x + 0.2, 0.1, z + 0.1), T_WHITE, T_WHITE, [0.05, 0.05, 0.06, 1.0]);
    }
    // torso, shoulders
    m.boxx(Vec3::new(-0.15, 0.9, -0.25), Vec3::new(0.15, 1.5, 0.25), T_WHITE, T_WHITE, top);
    m.boxx(Vec3::new(-0.13, 1.42, -0.34), Vec3::new(0.13, 1.54, 0.34), T_WHITE, T_WHITE, top);
    // arms swing opposite to the legs, hands at the ends
    for (side, fwd) in [(-1.0f32, -s), (1.0, s)] {
        let (z, x) = (side * 0.33, fwd * 0.16);
        m.boxx(Vec3::new(x - 0.07, 0.95, z - 0.07), Vec3::new(x + 0.07, 1.46, z + 0.07), T_WHITE, T_WHITE, dark);
        m.boxx(Vec3::new(x - 0.06, 0.85, z - 0.06), Vec3::new(x + 0.06, 0.95, z + 0.06), T_WHITE, T_WHITE, skin);
    }
    // neck, head, hair, a hint of a face
    m.boxx(Vec3::new(-0.06, 1.5, -0.07), Vec3::new(0.06, 1.58, 0.07), T_WHITE, T_WHITE, skin);
    m.boxx(Vec3::new(-0.13, 1.56, -0.13), Vec3::new(0.13, 1.82, 0.13), T_WHITE, T_WHITE, skin);
    m.boxx(Vec3::new(-0.15, 1.74, -0.15), Vec3::new(0.1, 1.86, 0.15), T_WHITE, T_WHITE, hair);
    m.build()
}

fn flat(tile: u32, hx: f32, hz: f32, y: f32, col: [f32; 4]) -> Mesh {
    let mut m = MB::default();
    m.ground(-hx, -hz, hx, hz, y, tile, 0, col);
    m.build()
}

pub fn setup_art(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let atlas = build_atlas();
    let base = images.add(atlas.base);
    let emissive = images.add(atlas.emissive);
    let mr = images.add(atlas.mr);

    let mat = mats.add(StandardMaterial {
        base_color_texture: Some(base),
        emissive: LinearRgba::rgb(2.6, 2.6, 2.6),
        emissive_texture: Some(emissive.clone()),
        metallic_roughness_texture: Some(mr),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        ..default()
    });
    let glow = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(1.7, 1.7, 1.7),
        base_color_texture: Some(emissive),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    let tiled = (0..6)
        .map(|k| {
            let (b, m) = tiled_images(k);
            mats.add(StandardMaterial {
                base_color_texture: Some(images.add(b)),
                metallic_roughness_texture: Some(images.add(m)),
                perceptual_roughness: 1.0,
                metallic: 1.0,
                ..default()
            })
        })
        .collect();

    let (db, dm) = tiled_images(M_ASPHALT);
    let deck = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(1.12, 1.12, 1.2),
        base_color_texture: Some(images.add(db)),
        metallic_roughness_texture: Some(images.add(dm)),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        ..default()
    });
    let shade = mats.add(StandardMaterial { unlit: true, alpha_mode: AlphaMode::Blend, ..default() });

    let mut cars: Vec<Handle<Mesh>> = vec![];
    for style in 0..3 {
        for c in CAR_COLORS.iter() {
            cars.push(meshes.add(car_mesh(*c, false, style)));
        }
    }
    let police_car = meshes.add(car_mesh([0.04, 0.04, 0.07], true, 0));

    // headlight beams + tail glow
    let mut cg = MB::default();
    cg.ground(2.0, -4.0, 16.0, 4.0, 0.12, T_CONE, 0, [1.0, 0.92, 0.7, 1.0]);
    cg.ground(-4.2, -1.8, -1.6, 1.8, 0.12, T_GLOW, 0, [0.5, 0.03, 0.02, 1.0]);
    let car_glow = meshes.add(cg.build());
    let mut pg = MB::default();
    pg.ground(0.3, -3.2, 6.7, 3.2, 1.9, T_GLOW, 0, [0.8, 0.03, 0.03, 1.0]);
    pg.ground(-6.7, -3.2, -0.3, 3.2, 1.9, T_GLOW, 0, [0.06, 0.16, 0.9, 1.0]);
    let police_glow = meshes.add(pg.build());

    let cloths = [
        [0.5, 0.1, 0.12],
        [0.1, 0.25, 0.5],
        [0.45, 0.4, 0.1],
        [0.12, 0.4, 0.3],
        [0.35, 0.12, 0.45],
        [0.3, 0.3, 0.33],
        [0.6, 0.3, 0.1],
        [0.08, 0.08, 0.1],
    ];
    let heads = [[0.5, 0.33, 0.25], [0.2, 0.12, 0.08], [0.05, 0.04, 0.04], [0.6, 0.5, 0.2]];
    let peds = cloths
        .iter()
        .enumerate()
        .map(|(i, c)| [meshes.add(ped_mesh(*c, heads[i % 4], 0)), meshes.add(ped_mesh(*c, heads[i % 4], 1))])
        .collect();
    let player = [meshes.add(ped_mesh([0.0, 0.75, 0.9], [0.9, 0.9, 0.95], 0)), meshes.add(ped_mesh([0.0, 0.75, 0.9], [0.9, 0.9, 0.95], 1))];

    let mut b = MB::default();
    b.ground(-1.4, -0.14, 1.4, 0.14, 0.0, T_SOLID, 0, [1.0, 0.85, 0.4, 1.0]);
    let bullet = meshes.add(b.build());

    let puffs = vec![
        meshes.add(flat(T_GLOW, 0.5, 0.5, 0.0, [1.0, 0.8, 0.3, 1.0])),
        meshes.add(flat(T_GLOW, 0.5, 0.5, 0.0, [1.0, 0.35, 0.05, 1.0])),
        meshes.add(flat(T_GLOW, 0.5, 0.5, 0.0, [0.10, 0.10, 0.13, 1.0])),
        meshes.add(flat(T_GLOW, 0.5, 0.5, 0.0, [1.0, 0.95, 0.8, 1.0])),
        meshes.add(flat(T_GLOW, 0.5, 0.5, 0.0, [0.07, 0.09, 0.13, 1.0])), // tyre spray
    ];
    let blood = meshes.add(flat(T_WHITE, 0.5, 0.5, 0.0, [0.25, 0.0, 0.01, 1.0]));

    let mut r = MB::default();
    let rc = [0.26, 0.32, 0.44, 1.0];
    r.wall(Vec2::new(-0.07, 0.0), Vec2::new(0.07, 0.0), 0.0, 3.6, T_STREAK, rc);
    r.wall(Vec2::new(0.0, -0.07), Vec2::new(0.0, 0.07), 0.0, 3.6, T_STREAK, rc);
    let rain = meshes.add(r.build());
    let ripple = meshes.add(flat(T_RING, 0.5, 0.5, 0.0, [0.05, 0.065, 0.09, 1.0]));

    let pickups = [3u32, 2, 6]
        .iter()
        .map(|k| {
            let mut m = MB::default();
            m.boxx(Vec3::new(-0.45, 0.4, -0.45), Vec3::new(0.45, 1.3, 0.45), T_STRIP + k, T_STRIP + k, WHITE);
            meshes.add(m.build())
        })
        .collect();
    let marker = meshes.add(flat(T_MARKER, 2.5, 2.5, 0.0, [1.0, 0.2, 0.8, 1.0]));
    let arrow = meshes.add(flat(T_ARROW, 1.0, 1.0, 0.0, [1.0, 0.85, 0.2, 1.0]));

    commands.insert_resource(Art {
        mat,
        glow,
        tiled,
        deck,
        shade,
        cars,
        police_car,
        car_glow,
        police_glow,
        peds,
        player,
        bullet,
        puffs,
        blood,
        rain,
        ripple,
        pickups,
        marker,
        arrow,
    });
}

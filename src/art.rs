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
            if (x / 4) % 2 == 0 && n > 0.1 {
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
            glow((1.0 - fy.abs() / w).max(0.0).powf(1.6) * (1.0 - u).powf(1.7))
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
pub const M_SPAN: [f32; 6] = [16.0, 4.0, 4.0, 16.0, 16.0, 8.0];
/// Texture size in pixels for each of them (asphalt and grass are drawn at full detail).
const M_RES: [usize; 6] = [512, 64, 64, 512, 1024, 128];

fn pnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    pn(x, y, period, period, seed)
}

/// Smooth value noise that repeats after `px` cells across and `py` cells down.
fn pn(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = x - xi;
    let fy = y - yi;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let w = |v: i32| v.rem_euclid(px);
    let h = |v: i32| v.rem_euclid(py);
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hf(w(xi), h(yi), seed);
    let b = hf(w(xi + 1), h(yi), seed);
    let c = hf(w(xi), h(yi + 1), seed);
    let d = hf(w(xi + 1), h(yi + 1), seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

fn sstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Tarmac at full texture resolution: aggregate grain, soft wear and repair patches, hairline
/// cracks and damp patches with feathered edges.
fn asphalt_px(x: i32, y: i32, size: usize) -> Px {
    let k = 1.0 / size as f32;
    let (u, v) = (x as f32 * k, y as f32 * k);
    let grain = hf(x, y, 5) * 0.45 + hf(x / 2, y / 2, 6) * 0.35 + hf(x / 4, y / 4, 7) * 0.2;
    let tone = pn(u * 6.0, v * 6.0, 6, 6, 31) * 0.6 + pn(u * 22.0, v * 22.0, 22, 22, 32) * 0.4;
    let b = 0.035 + grain * 0.028 + (tone - 0.5) * 0.014;
    let mut c = [b * 0.92, b * 0.97, b * 1.28];
    let mut r = 0.38 + grain * 0.2;
    // newer repair patch
    let patch = sstep(0.66, 0.7, pn(u * 5.0, v * 5.0, 5, 5, 33));
    c = mul(c, 1.0 - 0.13 * patch);
    r -= 0.08 * patch;
    // cracks, only in the more worn areas
    let crack = pn(u * 26.0, v * 26.0, 26, 26, 8) * 0.8 + pn(u * 90.0, v * 90.0, 90, 90, 18) * 0.2;
    let worn = sstep(0.45, 0.65, pn(u * 4.0, v * 4.0, 4, 4, 44));
    let line = 1.0 - sstep(0.004, 0.014, (crack - 0.5).abs());
    c = mul(c, 1.0 - 0.5 * line * worn);
    // damp patches
    let w = pn(u * 9.0, v * 9.0, 9, 9, 3) * 0.75 + pn(u * 32.0, v * 32.0, 32, 32, 4) * 0.25;
    let wet = sstep(0.64, 0.8, w);
    // (only a little smoother and darker: rain makes the whole road glossy, sun leaves no stains)
    c = mix3(c, [0.03, 0.036, 0.055], wet * 0.1);
    r += (0.36 - r) * wet;
    if hf(x, y, 77) > 0.992 {
        c = mul(c, 2.0); // glint of a stone chip
    }
    Px { c, e: g3(0.0), r, m: 0.0 }
}

/// Height of the water surface at texture position (u, v): three sizes of ripple.
fn wave(u: f32, v: f32) -> f32 {
    pn(u * 10.0, v * 6.0, 10, 6, 61) * 0.5 + pn(u * 26.0, v * 18.0, 26, 18, 62) * 0.3 + pn(u * 70.0, v * 54.0, 70, 54, 63) * 0.2
}

/// River water: deep blue-green with lighter shallows and streaks, smooth enough to mirror the sun.
fn water_px(x: i32, y: i32, size: usize) -> Px {
    let k = 1.0 / size as f32;
    let (u, v) = (x as f32 * k, y as f32 * k);
    let depth = pn(u * 3.0, v * 3.0, 3, 3, 64) * 0.6 + pn(u * 9.0, v * 9.0, 9, 9, 65) * 0.4;
    let h = wave(u, v);
    let mut c = mix3([0.012, 0.05, 0.085], [0.03, 0.105, 0.13], sstep(0.3, 0.75, depth));
    // crests are a touch lighter, troughs darker
    c = mul(c, 0.8 + 0.45 * h);
    // drifting streaks of foam and silt
    let streak = pn(u * 6.0, v * 40.0, 6, 40, 66);
    c = mix3(c, [0.09, 0.15, 0.16], sstep(0.78, 0.95, streak) * 0.35);
    Px { c, e: g3(0.0), r: 0.07 + 0.12 * (1.0 - h), m: 0.0 }
}

/// Normal map of the ripples in `wave`, so the surface glints as light plays over it.
pub fn water_normal() -> Image {
    let size = M_RES[M_WATER];
    let k = 1.0 / size as f32;
    let mut data = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 * k, y as f32 * k);
            let dx = (wave(u + k, v) - wave(u - k, v)) * 9.0;
            let dy = (wave(u, v + k) - wave(u, v - k)) * 9.0;
            let n = Vec3::new(-dx, -dy, 1.0).normalize();
            let o = (y * size + x) * 4;
            data[o] = ((n.x * 0.5 + 0.5) * 255.0) as u8;
            data[o + 1] = ((n.y * 0.5 + 0.5) * 255.0) as u8;
            data[o + 2] = ((n.z * 0.5 + 0.5) * 255.0) as u8;
            data[o + 3] = 255;
        }
    }
    mip_image(size, data, TextureFormat::Rgba8Unorm)
}

/// Lawn at full texture resolution: lusher and drier patches, mottling, individual blades.
fn grass_px(x: i32, y: i32, size: usize) -> Px {
    let k = 1.0 / size as f32;
    let (u, v) = (x as f32 * k, y as f32 * k);
    let broad = pn(u * 4.0, v * 4.0, 4, 4, 9) * 0.6 + pn(u * 11.0, v * 11.0, 11, 11, 10) * 0.4;
    let mott = pn(u * 72.0, v * 72.0, 72, 72, 11);
    // blades: fine noise stretched along two directions
    let blade = pn(u * 340.0, v * 96.0, 340, 96, 12) * 0.55 + pn(u * 112.0, v * 300.0, 112, 300, 13) * 0.45;
    let h = hf(x, y, 13);
    // (the broad patches are kept faint: they repeat with the texture and show from high up)
    let t = (0.26 + broad * 0.22 + mott * 0.34).clamp(0.0, 1.0);
    let mut c = mix3([0.012, 0.042, 0.017], [0.046, 0.135, 0.04], t);
    // drier, yellower turf in the thin patches
    let dry = sstep(0.42, 0.2, broad);
    c = mix3(c, [0.06, 0.085, 0.03], dry * 0.07);
    c = mul(c, (0.62 + 0.76 * blade) * (0.93 + 0.14 * h));
    if blade > 0.8 && h > 0.5 {
        c = mul(c, 1.35); // blades catching the light
    }
    // daisies and dandelions, a few centimetres across
    let f = hf(x / 4, y / 4, 21);
    if f > 0.9988 {
        c = if hf(x / 4, y / 4, 22) > 0.5 { [0.3, 0.26, 0.07] } else { [0.3, 0.3, 0.27] };
    }
    Px { c, e: g3(0.0), r: 0.88 + 0.08 * blade, m: 0.0 }
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
            // damp patches fade in softly rather than ending at a hard outline
            let wet = sstep(0.6, 0.82, pnoise(x as f32 / 8.0, y as f32 / 8.0, 4, 14)) * 0.4;
            Px { c: mix3([b, b * 0.98, b * 1.16], [b * 0.6, b * 0.62, b * 0.8], wet), e: g3(0.0), r: 0.5 - 0.3 * wet, m: 0.0 }
        }
        M_CONCRETE => {
            let slab = x % 32 == 0 || y % 32 == 0;
            let stain = pnoise(x as f32 / 8.0, y as f32 / 8.0, 4, 19);
            let b = if slab { 0.035 } else { 0.07 + n * 0.02 - (stain - 0.5).max(0.0) * 0.03 };
            let bay = y % 32 < 1 || (x % 16 == 0 && y % 32 < 22);
            if bay && n > 0.2 {
                return Px { c: g3(0.2), e: g3(0.0), r: 0.5, m: 0.0 };
            }
            Px { c: [b, b, b * 1.12], e: g3(0.0), r: 0.5 - 0.22 * sstep(0.55, 0.8, stain), m: 0.0 }
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

/// Square repeating texture with a full mip chain.
fn mip_image(size: usize, data: Vec<u8>, fmt: TextureFormat) -> Image {
    use bevy::image::{ImageAddressMode, ImageSamplerDescriptor};
    {
        // full mip chain (box filtered) so the ground stays smooth instead of sparkling when
        // seen from high up or at a slant
        let mut all = data.clone();
        let (mut prev, mut w, mut levels) = (data.clone(), size, 1u32);
        while w > 1 {
            let h = w / 2;
            let mut next = vec![0u8; h * h * 4];
            for y in 0..h {
                for x in 0..h {
                    for k in 0..4 {
                        let at = |xx: usize, yy: usize| prev[(yy * w + xx) * 4 + k] as u32;
                        next[(y * h + x) * 4 + k] =
                            ((at(x * 2, y * 2) + at(x * 2 + 1, y * 2) + at(x * 2, y * 2 + 1) + at(x * 2 + 1, y * 2 + 1) + 2) / 4) as u8;
                    }
                }
            }
            all.extend_from_slice(&next);
            prev = next;
            w = h;
            levels += 1;
        }
        let mut img = Image::new(
            Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            fmt,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.texture_descriptor.mip_level_count = levels;
        img.data = Some(all);
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        img
    }
}

fn tiled_images(kind: usize) -> (Image, Image) {
    let size: usize = M_RES[kind];
    let (mut base, mut mr) = (vec![0u8; size * size * 4], vec![0u8; size * size * 4]);
    for y in 0..size {
        for x in 0..size {
            let mut p = match kind {
                M_ASPHALT => asphalt_px(x as i32, y as i32, size),
                M_GRASS => grass_px(x as i32, y as i32, size),
                M_WATER => water_px(x as i32, y as i32, size),
                _ => tiled_px(kind, (x / 2) as i32, (y / 2) as i32),
            };
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
    let mk = |data: Vec<u8>, fmt: TextureFormat| mip_image(size, data, fmt);
    (mk(base, TextureFormat::Rgba8UnormSrgb), mk(mr, TextureFormat::Rgba8Unorm))
}

/// Animated water surface (see `web/assets/shaders/water.wgsl`).
#[derive(Asset, TypePath, bevy::render::render_resource::AsBindGroup, Clone)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub p: WaterParams,
}

#[derive(bevy::render::render_resource::ShaderType, Clone, Default)]
pub struct WaterParams {
    /// Camera position, w = time.
    pub cam: Vec4,
    /// Direction towards the sun or moon, w = daylight.
    pub sun: Vec4,
    /// Reflected sky colour, w = rain.
    pub sky: Vec4,
    /// Glitter colour, w = wind.
    pub sunc: Vec4,
    /// Colour of the water body.
    pub deep: Vec4,
    /// xy = drift direction.
    pub flow: Vec4,
}

impl bevy::pbr::Material for WaterMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/water.wgsl".into()
    }
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
        // tangents follow the world-space texture mapping (needed by the water's normal map)
        let tg = vec![[1.0, 0.0, 0.0, -1.0]; self.p.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tg)
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

    /// Like `quad`, but with a normal per corner (smooth-shaded surfaces).
    pub fn quad_n(&mut self, v: [Vec3; 4], n: [Vec3; 4], tile: u32, col: [f32; 4]) {
        let base = self.p.len() as u32;
        let (tx, ty) = ((tile as usize % AT) as f32, (tile as usize / AT) as f32);
        let (e, s) = (0.6, ASIZE as f32);
        let (u0, u1) = ((tx * TS as f32 + e) / s, ((tx + 1.0) * TS as f32 - e) / s);
        let (v0, v1) = ((ty * TS as f32 + e) / s, ((ty + 1.0) * TS as f32 - e) / s);
        let uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
        for k in 0..4 {
            self.p.push([v[k].x, v[k].y + self.lift, v[k].z]);
            self.n.push(n[k].to_array());
            self.uv.push(uvs[k]);
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
    /// The river surface.
    pub water: Handle<WaterMaterial>,
    /// Bridge decks: fades out while the player is underneath.
    pub deck: Handle<StandardMaterial>,
    /// Soft black overlay for contact shadows (alpha comes from vertex colour).
    pub shade: Handle<StandardMaterial>,
    /// Civilian cars: 8 colours x 3 body styles (index = colour + 8 * style).
    pub cars: Vec<Handle<Mesh>>,
    pub police_car: Handle<Mesh>,
    pub car_glow: Handle<Mesh>,
    pub police_glow: Handle<Mesh>,
    /// Each person has five meshes: four frames of a walking cycle, then standing still.
    pub peds: Vec<[Handle<Mesh>; 5]>,
    pub player: [Handle<Mesh>; 5],
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

/// Round wheel on a sideways axle at (x, z), standing on y = 0: tread, sidewall and a hubcap
/// on the outward face (`s` = +1 / -1 for the side of the vehicle).
fn wheel(m: &mut MB, x: f32, z: f32, s: f32, r: f32, hw: f32) {
    const N: usize = 16;
    let chrome = [0.75, 0.78, 0.85, 1.0];
    let wall = [0.55, 0.55, 0.6, 1.0];
    let pt = |k: usize, rr: f32, zz: f32| {
        let a = k as f32 / N as f32 * std::f32::consts::TAU;
        Vec3::new(x + a.cos() * rr, r + a.sin() * rr, zz)
    };
    let zo = z + s * hw;
    let hub = Vec3::new(x, r, zo + s * 0.03);
    for k in 0..N {
        let a = (k as f32 + 0.5) / N as f32 * std::f32::consts::TAU;
        face(m, [pt(k, r, z - hw), pt(k, r, z + hw), pt(k + 1, r, z + hw), pt(k + 1, r, z - hw)], Vec3::new(a.cos(), a.sin(), 0.0), T_TYRE, WHITE);
        face(m, [pt(k, r, zo), pt(k + 1, r, zo), pt(k + 1, r * 0.64, zo + s * 0.03), pt(k, r * 0.64, zo + s * 0.03)], Vec3::Z * s, T_TYRE, wall);
        // hubcap: a fan of triangles (quad with a doubled corner)
        let (p0, p1) = (pt(k, r * 0.64, zo + s * 0.03), pt(k + 1, r * 0.64, zo + s * 0.03));
        face(m, [hub, p0, p1, p1], Vec3::Z * s, T_METAL, if k % 4 == 0 { [0.3, 0.31, 0.35, 1.0] } else { chrome });
    }
}

/// Skin stretched over a row of cross-sections (`grid[station][point]`), like the panels of a
/// car body. `smooth` blends the shading across panels so the surface reads as curved;
/// `pick` gives the texture tile and colour of each panel.
fn loft(m: &mut MB, grid: &[Vec<Vec3>], smooth: bool, pick: &dyn Fn(usize, usize) -> (u32, [f32; 4])) {
    let (ni, nj) = (grid.len(), grid[0].len());
    let cen: Vec<Vec3> = grid.iter().map(|r| r.iter().copied().sum::<Vec3>() / nj as f32).collect();
    // panel normals, pointing away from the spine of the shape
    let face_n = |i: usize, j: usize| {
        let (a, b, c, d) = (grid[i][j], grid[i][j + 1], grid[i + 1][j + 1], grid[i + 1][j]);
        let n = (c - a).cross(d - b).normalize_or_zero();
        if n.dot((a + b + c + d) * 0.25 - (cen[i] + cen[i + 1]) * 0.5) < 0.0 { -n } else { n }
    };
    let mut vn = vec![vec![Vec3::ZERO; nj]; ni];
    for i in 0..ni - 1 {
        for j in 0..nj - 1 {
            let n = face_n(i, j);
            for (di, dj) in [(0, 0), (0, 1), (1, 1), (1, 0)] {
                vn[i + di][j + dj] += n;
            }
        }
    }
    for i in 0..ni - 1 {
        for j in 0..nj - 1 {
            let n = face_n(i, j);
            let q = [(i, j), (i, j + 1), (i + 1, j + 1), (i + 1, j)];
            let mut v = q.map(|(a, b)| grid[a][b]);
            let mut ns = q.map(|(a, b)| if smooth { vn[a][b].normalize_or(n) } else { n });
            if (v[1] - v[0]).cross(v[3] - v[0]).dot(n) < 0.0 {
                v.swap(1, 3);
                ns.swap(1, 3);
            }
            let (tile, col) = pick(i, j);
            m.quad_n(v, ns, tile, col);
        }
    }
}

/// Car body styles: 0 saloon, 1 hatchback, 2 SUV. Built to real proportions (about 4.5 m long,
/// 1.8 m wide) from a curved lower shell, a glasshouse, round wheels in arches, and trim.
fn car_mesh(col: [f32; 3], police: bool, style: u32) -> Mesh {
    let mut m = MB::default();
    let body = rgb(col, 1.0);
    let low = rgb(col, 0.7);
    let seam = rgb(col, 0.3);
    let trim = [0.05, 0.05, 0.06, 1.0];
    let chrome = [0.75, 0.78, 0.85, 1.0];
    let glass = [0.75, 0.85, 1.0, 1.0];
    let white = [0.92, 0.92, 0.96, 1.0];
    // half length, half width, sill height, belt line, roof, wheel radius, axle distance from centre
    let (len, wid, sill, belt, roof, wr, axle) = match style {
        2 => (2.3f32, 0.94f32, 0.3f32, 1.06f32, 1.74f32, 0.38f32, 1.42f32),
        1 => (2.02, 0.88, 0.2, 0.92, 1.44, 0.32, 1.28),
        _ => (2.3, 0.9, 0.2, 0.9, 1.4, 0.33, 1.38),
    };
    // cabin: front and rear of the glasshouse, rake of windscreen and rear screen
    let (cab_f, cab_r, ws, rs) = match style {
        2 => (0.95, -len + 0.1, 0.5, 0.22),
        1 => (0.7, -len + 0.12, 0.62, 0.42),
        _ => (0.72, -1.3, 0.66, 0.6),
    };
    let nose = belt - if style == 2 { 0.1 } else { 0.16 };
    let tail = if style == 0 { belt - 0.03 } else { belt };

    // wheels in dark arches
    for x in [-axle, axle] {
        for s in [-1.0f32, 1.0] {
            wheel(&mut m, x, s * (wid - 0.1), s, wr, 0.11);
            let z = s * (wid + 0.004);
            let c = Vec3::new(x, wr, z);
            let ra = wr + 0.08;
            let arch = [0.3, 0.3, 0.34, 1.0];
            for k in 0..10 {
                let p = |k: usize| {
                    let a = k as f32 / 10.0 * std::f32::consts::PI;
                    Vec3::new(x + a.cos() * ra, wr + a.sin() * ra, z)
                };
                face(&mut m, [c, p(k), p(k + 1), p(k + 1)], Vec3::Z * s, T_TYRE, arch);
            }
            face(&mut m, [Vec3::new(x - ra, wr, z), Vec3::new(x - ra, sill, z), Vec3::new(x + ra, sill, z), Vec3::new(x + ra, wr, z)], Vec3::Z * s, T_TYRE, arch);
        }
    }
    // floor pan, so nothing shows through between the wheels
    m.boxx(Vec3::new(-len + 0.25, 0.15, -wid + 0.14), Vec3::new(len - 0.25, sill + 0.03, wid - 0.14), T_METAL, T_METAL, trim);

    // lower shell: cross-sections from tail to nose (x, underside, top, half width)
    let stations = [
        (-len, sill + 0.22, tail - 0.18, wid * 0.78),
        (-len + 0.07, sill + 0.1, tail - 0.05, wid * 0.93),
        (-len + 0.3, sill, tail, wid),
        (-0.2, sill, belt, wid + 0.01),
        (cab_f, sill, belt, wid),
        (len - 0.55, sill, nose + 0.07, wid - 0.01),
        (len - 0.1, sill + 0.1, nose - 0.03, wid * 0.94),
        (len, sill + 0.22, nose - 0.15, wid * 0.78),
    ];
    // shape of each cross-section: rocker, bulging flank, shoulder, crowned top
    let ring = [(-0.93f32, 0.0f32), (-1.0, 0.35), (-0.99, 0.75), (-0.94, 0.96), (-0.8, 1.0), (0.0, 1.02), (0.8, 1.0), (0.94, 0.96), (0.99, 0.75), (1.0, 0.35), (0.93, 0.0)];
    let grid: Vec<Vec<Vec3>> = stations
        .iter()
        .map(|&(x, y0, y1, w)| ring.iter().map(|&(zf, yf)| Vec3::new(x, y0 + (y1 - y0) * yf, w * zf)).collect())
        .collect();
    loft(&mut m, &grid, true, &|_, j| (T_PAINT, if j == 0 || j == 9 { low } else { body }));
    for (i, out) in [(0usize, -1.0f32), (stations.len() - 1, 1.0)] {
        for j in 1..ring.len() - 1 {
            face(&mut m, [grid[i][0], grid[i][j], grid[i][j + 1], grid[i][j + 1]], Vec3::X * out, T_PAINT, low);
        }
    }

    // glasshouse: raked windscreen, side windows, rear screen, painted roof and roof rails
    let roofc = if police { white } else { body };
    let cab = [
        (cab_r, belt + 0.03, wid - 0.07),
        (cab_r + rs, roof, wid - 0.1),
        (cab_f - ws, roof, wid - 0.1),
        (cab_f, belt + 0.03, wid - 0.07),
    ];
    let cring = [(-1.0f32, 0.0f32), (-0.87, 0.84), (-0.73, 0.99), (0.0, 1.02), (0.73, 0.99), (0.87, 0.84), (1.0, 0.0)];
    let base = belt - 0.06;
    let cgrid: Vec<Vec<Vec3>> =
        cab.iter().map(|&(x, top, w)| cring.iter().map(|&(zf, yf)| Vec3::new(x, base + (top - base) * yf, w * zf)).collect()).collect();
    loft(&mut m, &cgrid, false, &|i, j| match (i, j) {
        (1, 0) | (1, 5) => (T_GLASS, glass),
        (1, 1) | (1, 4) => (T_PAINT, low),
        (1, _) => (T_PAINT, roofc),
        _ => (T_GLASS, glass),
    });
    let mid = (cab_r + rs + cab_f - ws) * 0.5;
    let top_y = base + (roof - base) * 0.84;
    for s in [-1.0f32, 1.0] {
        // pillars: B in the middle, A and C framing the screens
        for (x0, x1) in [(mid - 0.06, mid + 0.06), (cab_r + rs - 0.05, cab_r + rs + 0.05), (cab_f - ws - 0.05, cab_f - ws + 0.05)] {
            let (zb, zt) = (s * (wid - 0.1 + 0.006), s * ((wid - 0.1) * 0.87 + 0.006));
            face(&mut m, [Vec3::new(x0, top_y, zt), Vec3::new(x0, belt, zb), Vec3::new(x1, belt, zb), Vec3::new(x1, top_y, zt)], Vec3::Z * s, T_PAINT, low);
        }
        // door shut lines and handles
        let zs = s * (wid + 0.013);
        let mut seams = vec![mid, cab_f - 0.03];
        if style != 1 {
            seams.push(cab_r + rs * 0.5 + 0.1);
        }
        for x in seams {
            face(&mut m, [Vec3::new(x - 0.012, belt - 0.04, zs), Vec3::new(x - 0.012, sill + 0.06, zs), Vec3::new(x + 0.012, sill + 0.06, zs), Vec3::new(x + 0.012, belt - 0.04, zs)], Vec3::Z * s, T_PAINT, seam);
        }
        for x in [mid - 0.3, mid + 0.42] {
            let (za, zb) = (s * (wid - 0.01), s * (wid + 0.035));
            m.boxx(Vec3::new(x, belt - 0.15, za.min(zb)), Vec3::new(x + 0.17, belt - 0.11, za.max(zb)), T_METAL, T_METAL, chrome);
        }
        // door mirrors
        let (za, zb) = (s * (wid - 0.04), s * (wid + 0.2));
        m.boxx(Vec3::new(cab_f - 0.14, belt - 0.01, za.min(zb)), Vec3::new(cab_f + 0.03, belt + 0.12, za.max(zb)), T_PAINT, T_PAINT, low);
        // lamp clusters wrapped round the corners
        let (z0, z1) = if s < 0.0 { (-wid * 0.82, -wid * 0.36) } else { (wid * 0.36, wid * 0.82) };
        m.boxx(Vec3::new(len - 0.2, nose - 0.34, z0), Vec3::new(len + 0.015, nose - 0.17, z1), T_HEADLIGHT, T_HEADLIGHT, WHITE);
        m.boxx(Vec3::new(-len - 0.015, tail - 0.37, z0), Vec3::new(-len + 0.2, tail - 0.2, z1), T_TAILLIGHT, T_TAILLIGHT, WHITE);
    }
    // grille, bumpers, number plates
    m.boxx(Vec3::new(len - 0.05, sill + 0.24, -wid * 0.34), Vec3::new(len + 0.02, nose - 0.19, wid * 0.34), T_METAL, T_METAL, trim);
    m.boxx(Vec3::new(len - 0.16, sill + 0.06, -wid * 0.84), Vec3::new(len + 0.05, sill + 0.24, wid * 0.84), T_METAL, T_METAL, trim);
    m.boxx(Vec3::new(-len - 0.05, sill + 0.06, -wid * 0.84), Vec3::new(-len + 0.16, sill + 0.24, wid * 0.84), T_METAL, T_METAL, trim);
    m.boxx(Vec3::new(len + 0.05, sill + 0.09, -0.26), Vec3::new(len + 0.06, sill + 0.21, 0.26), T_WHITE, T_WHITE, [0.85, 0.85, 0.8, 1.0]);
    m.boxx(Vec3::new(-len - 0.06, sill + 0.09, -0.26), Vec3::new(-len - 0.05, sill + 0.21, 0.26), T_WHITE, T_WHITE, [0.85, 0.85, 0.8, 1.0]);
    if style == 2 {
        // roof rails
        for s in [-1.0f32, 1.0] {
            let z = s * (wid - 0.1) * 0.7;
            m.boxx(Vec3::new(cab_r + 0.5, roof, z - 0.03), Vec3::new(cab_f - 0.7, roof + 0.08, z + 0.03), T_METAL, T_METAL, chrome);
        }
    }
    if police {
        m.boxx(Vec3::new(mid - 0.16, roof, -0.55), Vec3::new(mid + 0.16, roof + 0.2, 0.55), T_POLICE, T_POLICE, WHITE);
        // white doors
        for s in [-1.0f32, 1.0] {
            let z = s * (wid + 0.011);
            face(&mut m, [Vec3::new(cab_r + 0.3, belt - 0.05, z), Vec3::new(cab_r + 0.3, sill + 0.1, z), Vec3::new(cab_f - 0.06, sill + 0.1, z), Vec3::new(cab_f - 0.06, belt - 0.05, z)], Vec3::Z * s, T_PAINT, white);
        }
    }
    m.build()
}

/// Rounded clump of foliage: a slightly lumpy ball, darker underneath where the light does not reach.
fn blob(m: &mut MB, c: Vec3, rx: f32, ry: f32, seed: u32, tile: u32, col: [f32; 3]) {
    const SEG: usize = 8;
    const RING: usize = 5;
    let grid: Vec<Vec<Vec3>> = (0..=RING)
        .map(|i| {
            // from a little below the middle up to the crown
            let lat = -1.0 + (i as f32 / RING as f32) * 2.55;
            (0..=SEG)
                .map(|j| {
                    let lon = j as f32 / SEG as f32 * std::f32::consts::TAU;
                    let lump = 0.86 + 0.28 * hf((j % SEG) as i32, i as i32, seed);
                    let pole = if i == RING { 0.05 } else { 1.0 };
                    let rr = rx * lat.cos().max(0.0) * lump * pole + 0.02;
                    c + Vec3::new(lon.cos() * rr, lat.sin() * ry * if i == RING { 1.0 } else { lump }, lon.sin() * rr)
                })
                .collect()
        })
        .collect();
    loft(m, &grid, true, &|i, j| {
        let k = 0.5 + 0.17 * i as f32 + 0.1 * hf(i as i32, j as i32, seed + 7);
        (tile, rgb(col, k))
    });
}

/// A tree at `p` (the builder's `lift` is the ground height there). `s` picks size and species:
/// broad-leaved trees with clumped crowns, spruces, and white-stemmed birches.
pub fn add_tree(m: &mut MB, p: Vec2, s: u8) {
    let k = s as f32 / 255.0;
    let seed = s as u32 * 31 + (p.x as i32 as u32 & 255);
    let tone = 0.78 + ((s as u32 * 37) % 100) as f32 * 0.005;
    let rot = k * 6.0;
    let at = |dx: f32, y: f32, dz: f32| Vec3::new(p.x + dx, y, p.y + dz);
    match s % 6 {
        0 => {
            // spruce: tiers of drooping branches narrowing to a point
            let h = 8.0 + k * 5.0;
            let r = 2.0 + k * 1.0;
            m.prism(p, 0.26, 0.16, 0.0, h * 0.35, 6, rot, T_METAL, T_METAL, [0.34, 0.22, 0.16, 1.0]);
            let tiers = 5;
            for t in 0..tiers {
                let f = t as f32 / tiers as f32;
                let (y0, y1) = (h * (0.14 + 0.72 * f), h * (0.14 + 0.72 * f) + h * 0.3);
                let rr = r * (1.0 - 0.78 * f);
                let g = tone * (0.62 + 0.14 * t as f32);
                m.prism(p, rr, rr * 0.12, y0, y1.min(h), 9, rot + t as f32 * 0.4, T_LEAF + 1, T_LEAF + 1, [0.42 * g, 0.62 * g, 0.5 * g, 1.0]);
            }
        }
        1 => {
            // birch: slender white stem, light airy crown
            let h = 6.5 + k * 3.5;
            m.prism(p, 0.17, 0.1, 0.0, h, 6, rot, T_WHITE, T_WHITE, [0.86, 0.86, 0.8, 1.0]);
            for j in 0..3 {
                m.prism(p, 0.175, 0.17, h * (0.18 + 0.2 * j as f32), h * (0.18 + 0.2 * j as f32) + 0.18, 6, rot, T_WHITE, T_WHITE, [0.12, 0.12, 0.12, 1.0]);
            }
            let r = 1.3 + k * 0.8;
            let c = [0.95 * tone, 1.12 * tone, 0.6 * tone];
            blob(m, at(0.0, h * 0.82, 0.0), r, r * 1.25, seed, T_LEAF, c);
            let o = Vec2::from_angle(rot) * r * 0.6;
            blob(m, at(o.x, h * 0.62, o.y), r * 0.7, r * 0.8, seed + 3, T_LEAF, c);
            blob(m, at(-o.y * 0.9, h * 0.7, o.x * 0.9), r * 0.62, r * 0.7, seed + 5, T_LEAF, c);
        }
        _ => {
            // lime, maple, oak: stout trunk forking into boughs, crown of several clumps
            let h = 3.4 + k * 3.2;
            let r = 1.9 + k * 1.7;
            let bark = [0.33, 0.23, 0.17, 1.0];
            m.prism(p, 0.42, 0.26, 0.0, h, 7, rot, T_METAL, T_METAL, bark);
            m.prism(p, 0.6, 0.42, -0.3, 0.5, 7, rot, T_METAL, T_METAL, bark); // root flare
            let leaf = if s % 7 == 0 { T_LEAF + 1 } else { T_LEAF };
            let warm = (s % 5) as f32 * 0.04;
            let c = [(0.78 + warm) * tone, 1.0 * tone, (0.72 - warm) * tone];
            blob(m, at(0.0, h + r * 0.75, 0.0), r, r * 0.9, seed, leaf, c);
            for j in 0..3 {
                let a = rot + j as f32 * 2.1 + 0.4;
                let o = Vec2::from_angle(a) * r * 0.72;
                let rr = r * (0.56 + 0.1 * hf(j, s as i32, 5));
                // a bough reaching out to each clump
                m.quad(
                    [Vec3::new(p.x - 0.12, h * 0.8, p.y), Vec3::new(p.x + 0.12, h * 0.8, p.y), Vec3::new(p.x + o.x + 0.06, h + r * 0.35, p.y + o.y), Vec3::new(p.x + o.x - 0.06, h + r * 0.35, p.y + o.y)],
                    T_METAL,
                    0,
                    bark,
                );
                blob(m, at(o.x, h + r * (0.42 + 0.14 * j as f32), o.y), rr, rr * 0.85, seed + 11 * (j as u32 + 1), leaf, [c[0] * (0.92 + 0.07 * j as f32), c[1] * (0.92 + 0.07 * j as f32), c[2]]);
            }
        }
    }
}

/// A parked narrow-body airliner (about 37 m long, 34 m span) at `pos`, nose towards `heading`,
/// wheels on the ground at height `y`.
pub fn add_plane(m: &mut MB, pos: Vec2, heading: f32, y: f32, livery: usize) {
    let f = Vec2::from_angle(heading);
    let r = Vec2::new(-f.y, f.x);
    // local (forward, up, right) -> world
    let w = |x: f32, h: f32, z: f32| {
        let p = pos + f * x + r * z;
        Vec3::new(p.x, y + h, p.y)
    };
    let white = [0.9, 0.91, 0.94, 1.0];
    let belly = [0.55, 0.57, 0.62, 1.0];
    let accent = rgb(NEON[livery % 8], 0.75);
    let grey = [0.5, 0.52, 0.56, 1.0];
    let dark = [0.07, 0.07, 0.09, 1.0];
    // a tube of `n` sides around a centre line of (x, height, radius) stations, offset sideways by `z`
    let tube = |m: &mut MB, st: &[(f32, f32, f32)], z: f32, n: usize, pick: &dyn Fn(usize, usize) -> (u32, [f32; 4])| {
        let grid: Vec<Vec<Vec3>> = st
            .iter()
            .map(|&(x, h, rad)| {
                (0..=n)
                    .map(|k| {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU;
                        w(x, h + a.cos() * rad, z + a.sin() * rad)
                    })
                    .collect()
            })
            .collect();
        loft(m, &grid, true, pick);
    };
    // fuselage: pointed nose, constant section, tail cone sweeping up
    let cy = 3.5;
    let body = [
        (18.8, cy - 0.5, 0.05),
        (18.2, cy - 0.35, 0.7),
        (17.0, cy - 0.1, 1.45),
        (15.0, cy, 1.95),
        (-6.0, cy, 1.95),
        (-12.0, cy + 0.35, 1.5),
        (-16.5, cy + 0.9, 0.75),
        (-18.8, cy + 1.2, 0.2),
    ];
    // k = 0 is the top of the tube; the lower third is grey, a cheat line of windows runs along the side
    tube(m, &body, 0.0, 14, &|i, k| {
        let side = k == 3 || k == 10;
        if i == 2 && (k == 0 || k == 13) {
            (T_GLASS, [0.5, 0.6, 0.8, 1.0]) // flight deck windows
        } else if side && (3..5).contains(&i) {
            (T_GLASS, [0.35, 0.4, 0.5, 1.0])
        } else if (5..9).contains(&k) {
            (T_WHITE, belly)
        } else {
            (T_WHITE, white)
        }
    });
    // flat surfaces: top and underside of a wing, tailplane or fin, given its four corners
    let slab = |m: &mut MB, q: [Vec3; 4], col: [f32; 4], out: Vec3| {
        let n = (q[1] - q[0]).cross(q[3] - q[0]);
        let q2 = if n.dot(out) < 0.0 { [q[0], q[3], q[2], q[1]] } else { q };
        m.quad(q2, T_WHITE, 0, col);
        m.quad([q2[0], q2[3], q2[2], q2[1]], T_WHITE, 0, [col[0] * 0.6, col[1] * 0.6, col[2] * 0.6, 1.0]);
    };
    for s in [-1.0f32, 1.0] {
        // swept wing with a little dihedral, winglet at the tip
        slab(m, [w(4.5, cy - 1.0, s * 1.6), w(-2.2, cy - 1.0, s * 1.6), w(-7.6, cy - 0.1, s * 17.0), w(-6.0, cy - 0.1, s * 17.0)], grey, Vec3::Y);
        slab(m, [w(-6.0, cy - 0.1, s * 17.0), w(-7.6, cy - 0.1, s * 17.0), w(-8.6, cy + 1.9, s * 17.3), w(-7.6, cy + 1.9, s * 17.3)], accent, Vec3::new(r.x, 0.0, r.y) * s);
        // tailplane
        slab(m, [w(-13.6, cy + 0.9, s * 0.8), w(-16.6, cy + 0.9, s * 0.8), w(-18.6, cy + 1.2, s * 6.2), w(-17.4, cy + 1.2, s * 6.2)], grey, Vec3::Y);
        // engine under the wing on a pylon
        let (ez, eh) = (s * 5.6, cy - 2.0);
        tube(m, &[(5.2, eh, 0.95), (4.6, eh, 1.1), (1.6, eh, 1.05), (0.6, eh, 0.6)], ez, 10, &|_, _| (T_WHITE, white));
        tube(m, &[(5.25, eh, 0.9), (5.2, eh, 0.05)], ez, 10, &|_, _| (T_WHITE, dark));
        slab(m, [w(3.6, cy - 0.9, ez), w(1.2, cy - 0.9, ez), w(1.2, eh + 0.9, ez), w(3.6, eh + 0.9, ez)], grey, Vec3::new(r.x, 0.0, r.y) * s);
        // main landing gear
        let (gx, gz) = (-1.6, s * 3.6);
        m.quad([w(gx - 0.12, cy - 1.0, gz), w(gx - 0.12, 0.5, gz), w(gx + 0.12, 0.5, gz), w(gx + 0.12, cy - 1.0, gz)], T_METAL, 0, grey);
        m.quad([w(gx + 0.12, cy - 1.0, gz), w(gx + 0.12, 0.5, gz), w(gx - 0.12, 0.5, gz), w(gx - 0.12, cy - 1.0, gz)], T_METAL, 0, grey);
        tube(m, &[(gx, 0.55, 0.05), (gx, 0.55, 0.55)], gz - 0.3, 10, &|_, _| (T_TYRE, WHITE));
        tube(m, &[(gx, 0.55, 0.55), (gx, 0.55, 0.05)], gz + 0.3, 10, &|_, _| (T_TYRE, WHITE));
    }
    // fin in the airline's colour, nose gear
    let side = Vec3::new(r.x, 0.0, r.y);
    slab(m, [w(-11.8, cy + 1.6, 0.0), w(-17.6, cy + 1.7, 0.0), w(-19.2, cy + 8.2, 0.0), w(-16.6, cy + 8.2, 0.0)], accent, side);
    m.quad([w(13.4, cy - 1.6, -0.1), w(13.4, 0.4, -0.1), w(13.4, 0.4, 0.1), w(13.4, cy - 1.6, 0.1)], T_METAL, 0, grey);
    tube(m, &[(13.4, 0.42, 0.05), (13.4, 0.42, 0.42)], -0.2, 8, &|_, _| (T_TYRE, WHITE));
    tube(m, &[(13.4, 0.42, 0.42), (13.4, 0.42, 0.05)], 0.2, 8, &|_, _| (T_TYRE, WHITE));
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
        for sd in [-1.0f32, 1.0] {
            wheel(&mut m, x, sd * (w - 0.12), sd, 0.5, 0.18);
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

/// A person, 1.8 m tall, facing +x. Frames 0..4 are one walking cycle: left foot forward,
/// passing (right knee lifted), right foot forward, passing (left knee lifted).
fn ped_mesh(cloth: [f32; 3], head: [f32; 3], frame: u32) -> Mesh {
    let mut m = MB::default();
    let (s, lifted) = match frame {
        0 => (1.0f32, 9usize),
        1 => (0.0, 1),
        2 => (-1.0, 9),
        3 => (0.0, 0),
        _ => (1e-6, 9), // standing
    };
    // the body rises as the legs pass under it
    let b = if s == 0.0 { 0.04 } else { 0.0 };
    let top = rgb(cloth, 1.0);
    let dark = rgb(cloth, 0.55);
    let legs = [0.07 + cloth[2] * 0.15, 0.08 + cloth[0] * 0.1, 0.12 + cloth[1] * 0.12, 1.0];
    let skin = [0.62, 0.44, 0.34, 1.0];
    let hair = rgb(head, 0.8);
    let shoe = [0.05, 0.05, 0.06, 1.0];
    // legs: thigh, shin and shoe, so a stride reads as a bent limb rather than a sliding post
    for (i, (side, fwd)) in [(-1.0f32, s), (1.0, -s)].into_iter().enumerate() {
        let (z, x) = (side * 0.13, fwd * 0.26);
        let up = if lifted == i { 0.14 } else { 0.0 };
        let knee = if lifted == i { 0.12 } else { x * 0.45 };
        let foot = if lifted == i { -0.06 } else { x };
        m.boxx(Vec3::new(knee - 0.1, 0.5, z - 0.09), Vec3::new(knee + 0.1, 0.94 + b, z + 0.09), T_WHITE, T_WHITE, legs);
        m.boxx(Vec3::new(foot - 0.09, 0.08 + up, z - 0.085), Vec3::new(foot + 0.09, 0.54 + up * 0.4, z + 0.085), T_WHITE, T_WHITE, legs);
        m.boxx(Vec3::new(foot - 0.1, up, z - 0.1), Vec3::new(foot + 0.2, 0.1 + up, z + 0.1), T_WHITE, T_WHITE, shoe);
    }
    // torso, shoulders
    m.boxx(Vec3::new(-0.15, 0.9 + b, -0.25), Vec3::new(0.15, 1.5 + b, 0.25), T_WHITE, T_WHITE, top);
    m.boxx(Vec3::new(-0.13, 1.42 + b, -0.34), Vec3::new(0.13, 1.54 + b, 0.34), T_WHITE, T_WHITE, top);
    // arms swing opposite to the legs: upper arm, forearm, hand
    for (side, fwd) in [(-1.0f32, -s), (1.0, s)] {
        let (z, x) = (side * 0.33, fwd * 0.2);
        m.boxx(Vec3::new(x * 0.4 - 0.07, 1.18 + b, z - 0.07), Vec3::new(x * 0.4 + 0.07, 1.46 + b, z + 0.07), T_WHITE, T_WHITE, dark);
        m.boxx(Vec3::new(x - 0.065, 0.95 + b, z - 0.065), Vec3::new(x + 0.065, 1.2 + b, z + 0.065), T_WHITE, T_WHITE, dark);
        m.boxx(Vec3::new(x - 0.06, 0.85 + b, z - 0.06), Vec3::new(x + 0.06, 0.95 + b, z + 0.06), T_WHITE, T_WHITE, skin);
    }
    // neck, head, hair
    m.boxx(Vec3::new(-0.06, 1.5 + b, -0.07), Vec3::new(0.06, 1.58 + b, 0.07), T_WHITE, T_WHITE, skin);
    m.boxx(Vec3::new(-0.13, 1.56 + b, -0.13), Vec3::new(0.13, 1.82 + b, 0.13), T_WHITE, T_WHITE, skin);
    m.boxx(Vec3::new(-0.15, 1.74 + b, -0.15), Vec3::new(0.1, 1.86 + b, 0.15), T_WHITE, T_WHITE, hair);
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
    mut waters: ResMut<Assets<WaterMaterial>>,
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
                normal_map_texture: (k == M_WATER).then(|| images.add(water_normal())),
                metallic_roughness_texture: Some(images.add(m)),
                perceptual_roughness: 1.0,
                metallic: 1.0,
                ..default()
            })
        })
        .collect();

    let water = waters.add(WaterMaterial {
        p: WaterParams { deep: Vec4::new(0.05, 0.14, 0.15, 0.0), flow: Vec4::new(-0.95, 0.3, 0.0, 0.0), ..default() },
    });
    let (db, dm) = tiled_images(M_ASPHALT);
    let deck = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(1.0, 1.0, 1.0), // same tone as the road on the ground, so ramps blend in
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
    // one narrow beam from each headlamp, overlapping into a brighter core down the middle,
    // a small hot spot at each lamp, and two soft red tail lamps
    for z in [-0.6f32, 0.6] {
        cg.ground(2.2, z - 2.3, 21.0, z + 2.3, 0.12, T_CONE, 0, [0.55, 0.52, 0.42, 1.0]);
        cg.ground(1.9, z - 0.5, 2.9, z + 0.5, 0.75, T_GLOW, 0, [0.9, 0.85, 0.7, 1.0]);
        cg.ground(-3.6, z - 0.75, -2.1, z + 0.75, 0.12, T_GLOW, 0, [0.45, 0.02, 0.015, 1.0]);
    }
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
        .map(|(i, c)| [0, 1, 2, 3, 4].map(|f| meshes.add(ped_mesh(*c, heads[i % 4], f))))
        .collect();
    let player = [0, 1, 2, 3, 4].map(|f| meshes.add(ped_mesh([0.0, 0.75, 0.9], [0.9, 0.9, 0.95], f)));

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
        water,
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

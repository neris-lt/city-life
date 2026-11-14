//! Vilnius public transport: real bus and trolleybus routes from the city's GTFS feed
//! (see `tools/build_transit.py`). How many vehicles run on each route follows the real
//! weekday timetable for the current in-game hour; they drive the real path and call at
//! the real stops.

use crate::Game;
use crate::actors::{Bus, spawn_bus};
use crate::art::*;
use crate::world::*;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

static DATA: &[u8] = include_bytes!("../data/transit.bin");
/// Average speed including stops, used to space vehicles along a route.
const AVG_SPEED: f32 = 7.5;
/// The in-game day is compressed, so services run this many times closer together than in
/// reality; the proportions between routes and between hours stay true to the timetable.
const DENSITY: f32 = 4.0;

pub struct Route {
    pub name: String,
    pub kind: u8, // 0 bus, 1 trolleybus, 2 night bus
    hours: [u8; 24],
    pub stops: Vec<Vec2>,
    pub pts: Vec<Vec2>,
    cum: Vec<f32>,
    min: Vec2,
    max: Vec2,
}

#[derive(Resource)]
pub struct Transit {
    pub routes: Vec<Route>,
    meshes: HashMap<(String, u8), Handle<Mesh>>,
}

impl Transit {
    pub fn load() -> Transit {
        let d = DATA;
        let mut o = 4;
        let u16_ = |o: &mut usize| {
            *o += 2;
            u16::from_le_bytes([d[*o - 2], d[*o - 1]])
        };
        let f = |o: &mut usize| {
            *o += 4;
            f32::from_le_bytes(d[*o - 4..*o].try_into().unwrap())
        };
        let n = u16_(&mut o);
        let mut routes = vec![];
        for _ in 0..n {
            let nl = d[o] as usize;
            let name = String::from_utf8_lossy(&d[o + 1..o + 1 + nl]).into_owned();
            o += 1 + nl;
            let kind = d[o];
            o += 1;
            let mut hours = [0u8; 24];
            hours.copy_from_slice(&d[o..o + 24]);
            o += 24;
            let ns = u16_(&mut o);
            let stops = (0..ns)
                .map(|_| {
                    let x = f(&mut o);
                    Vec2::new(x, f(&mut o))
                })
                .collect();
            o += 4;
            let np = u32::from_le_bytes(d[o - 4..o].try_into().unwrap());
            let pts: Vec<Vec2> = (0..np)
                .map(|_| {
                    let x = f(&mut o);
                    Vec2::new(x, f(&mut o))
                })
                .collect();
            let mut cum = vec![0.0];
            for w in pts.windows(2) {
                cum.push(cum[cum.len() - 1] + w[0].distance(w[1]));
            }
            let min = pts.iter().fold(Vec2::MAX, |a, p| a.min(*p));
            let max = pts.iter().fold(Vec2::MIN, |a, p| a.max(*p));
            routes.push(Route { name, kind, hours, stops, pts, cum, min, max });
        }
        Transit { routes, meshes: HashMap::default() }
    }
}

impl Route {
    /// Position, travel direction and next point index at distance `s` along the route.
    fn at(&self, s: f32) -> (Vec2, Vec2, usize) {
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let l = (self.cum[i] - self.cum[i - 1]).max(0.01);
        (a.lerp(b, ((s - self.cum[i - 1]) / l).clamp(0.0, 1.0)), (b - a) / l, i)
    }
}

pub fn bus_spawner(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    city: Res<City>,
    mut transit: ResMut<Transit>,
    game: Res<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    buses: Query<&Bus>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.6;
    let alive: HashSet<(u16, i32)> = buses.iter().map(|b| b.key).collect();
    if alive.len() >= 14 {
        return;
    }
    let focus = game.pos;
    let vh = game.view_half + Vec2::splat(14.0);
    let hour = (game.clock as usize) % 24;
    let transit = &mut *transit;
    let mut spawned = 0;
    for (ri, r) in transit.routes.iter().enumerate() {
        let per_hour = r.hours[hour];
        if per_hour == 0 || focus.x < r.min.x - 160.0 || focus.x > r.max.x + 160.0 || focus.y < r.min.y - 160.0 || focus.y > r.max.y + 160.0 {
            continue;
        }
        // real headway -> real spacing between vehicles along the line
        let spacing = 3600.0 / per_hour as f32 * AVG_SPEED / DENSITY;
        let total = r.cum[r.cum.len() - 1];
        let travelled = AVG_SPEED * game.time;
        let shift = (travelled / spacing).floor();
        let offset = travelled - shift * spacing;
        let slots = (total / spacing).ceil() as i32;
        for k in 0..slots {
            let s = k as f32 * spacing + offset;
            if s > total - 40.0 {
                continue;
            }
            let key = (ri as u16, k - shift as i32);
            if alive.contains(&key) {
                continue;
            }
            let (pos, dir, idx) = r.at(s);
            let d = pos.distance(focus);
            let offscreen = (pos.x - focus.x).abs() > vh.x || (pos.y - focus.y).abs() > vh.y;
            if d > 145.0 || !offscreen || !city.area_ready(pos) {
                continue;
            }
            // next stop: the first one still ahead of the vehicle
            let stop = r.stops.iter().position(|st| {
                let (near, _) = r.pts.iter().enumerate().fold((0usize, f32::MAX), |b, (i, p)| {
                    let dd = p.distance_squared(*st);
                    if dd < b.1 { (i, dd) } else { b }
                });
                r.cum[near] > s + 8.0
            });
            let mesh = transit
                .meshes
                .entry((r.name.clone(), r.kind))
                .or_insert_with(|| meshes.add(bus_mesh(&r.name, r.kind)))
                .clone();
            let p = pos + Vec2::new(-dir.y, dir.x) * 1.6;
            spawn_bus(&mut commands, &art, mesh, p, dir.y.atan2(dir.x), ri as u16, idx, stop.unwrap_or(r.stops.len()), key);
            spawned += 1;
            if spawned >= 2 {
                return;
            }
        }
    }
}

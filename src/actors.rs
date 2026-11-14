//! Player, vehicles, traffic AI, pedestrians, police, bullets, particles.

use crate::art::*;
use crate::world::*;
use crate::{Game, Mission};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Car {
    pub vel: Vec2,
    pub heading: f32,
    pub health: f32,
    pub steer: f32,
    pub throttle: f32,
    pub handbrake: bool,
    pub player: bool,
    pub police: bool,
    pub lights: bool,
    pub burn: f32,
    /// Half the wheelbase used for collision circles (longer for buses).
    pub half: f32,
    /// Height above ground (bridge decks > 0, tunnels < 0). NaN until first placed.
    pub y: f32,
    pub vy: f32,
    pub max_speed: f32,
}

#[derive(Component)]
pub struct CarAi {
    edge: u32,
    fwd: bool,
    idx: usize,
    lane_seed: f32,
    lane: f32,
    speed_k: f32,
    cruise: f32,
    stuck: f32,
    shoot_cd: f32,
    panic: f32,
    done: bool,
    /// Set for buses: they follow a fixed route instead of wandering the road graph.
    bus: Option<BusPath>,
}

pub struct BusPath {
    route: u16,
    idx: usize,
    stop: usize,
    dwell: f32,
}

#[derive(Component)]
pub struct Bus {
    pub key: (u16, i32),
}

/// Long vehicles: others keep a bigger gap.
#[derive(Component)]
pub struct Long;

#[derive(Component)]
pub struct Ped {
    /// Which person model, and which half of the walking stride is showing.
    var: usize,
    frame: u8,
    edge: u32, // u32::MAX = not on a footpath yet
    fwd: bool,
    idx: usize,
    off: f32,
    speed: f32,
    flee: f32,
    flee_dir: Vec2,
    dead: f32,
}

#[derive(Component)]
pub struct Bullet {
    vel: Vec2,
    life: f32,
    from_player: bool,
}

#[derive(Component)]
pub struct Particle {
    vel: Vec3,
    life: f32,
    max: f32,
    size: f32,
}

#[derive(Component)]
pub struct Pickup {
    pub kind: usize, // 0 health, 1 ammo, 2 cash
    pub cell: Option<IVec2>,
    pub value: i64,
}

#[derive(Component)]
pub struct Beacon;

fn p2(t: &Transform) -> Vec2 {
    Vec2::new(t.translation.x, t.translation.z)
}
fn wrap(a: f32) -> f32 {
    let mut a = (a + PI) % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a - PI
}
fn right_of(d: Vec2) -> Vec2 {
    Vec2::new(-d.y, d.x)
}
fn yaw(heading: f32) -> Quat {
    Quat::from_rotation_y(-heading)
}

// ---- spawning helpers -----------------------------------------------------
fn add_car_lights(commands: &mut Commands, art: &Art, car: Entity) {
    commands.spawn((
        Mesh3d(art.car_glow.clone()),
        MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
        Transform::default(),
        ChildOf(car),
    ));
}

pub fn spawn_bus(commands: &mut Commands, art: &Art, mesh: Handle<Mesh>, pos: Vec2, heading: f32, route: u16, idx: usize, stop: usize, key: (u16, i32)) {
    let id = commands
        .spawn((
            Car {
                vel: Vec2::from_angle(heading) * 6.0,
                heading,
                health: 260.0,
                steer: 0.0,
                throttle: 0.0,
                handbrake: false,
                player: false,
                police: false,
                lights: true,
                burn: 0.0,
                half: 4.2,
                y: f32::NAN,
                vy: 0.0,
                max_speed: 17.0,
            },
            CarAi {
                edge: 0,
                fwd: true,
                idx: 1,
                lane_seed: 0.0,
                lane: 0.0,
                speed_k: 1.0,
                cruise: 10.5,
                stuck: 0.0,
                shoot_cd: 1.0,
                panic: 0.0,
                done: false,
                bus: Some(BusPath { route, idx, stop, dwell: 0.0 }),
            },
            Bus { key },
            Long,
            Mesh3d(mesh),
            MeshMaterial3d(art.mat.clone()),
            Transform::from_xyz(pos.x, 0.0, pos.y).with_rotation(yaw(heading)),
        ))
        .id();
    commands.spawn((
        Mesh3d(art.car_glow.clone()),
        MeshMaterial3d(art.glow.clone()),
        NotShadowCaster,
        Transform::from_xyz(3.9, 0.0, 0.0),
        ChildOf(id),
    ));
}

pub fn spawn_car(commands: &mut Commands, art: &Art, pos: Vec2, heading: f32, kind: usize, ai: Option<CarAi>) -> Entity {
    let police = kind == POLICE_KIND;
    let driven = ai.is_some();
    let id = commands
        .spawn((
            Car {
                vel: Vec2::ZERO,
                heading,
                health: 100.0,
                steer: 0.0,
                throttle: 0.0,
                handbrake: !driven,
                player: false,
                police,
                lights: driven,
                burn: 0.0,
                half: 1.25,
                y: f32::NAN,
                vy: 0.0,
                max_speed: if police { 25.0 } else { 22.0 + (kind as f32) * 0.8 },
            },
            Mesh3d(if police {
                art.police_car.clone()
            } else {
                // body style varies from car to car
                art.cars[kind % 8 + 8 * (hash2((pos.x * 3.0) as i32, (pos.y * 3.0) as i32, 31) % 3) as usize].clone()
            }),
            MeshMaterial3d(art.mat.clone()),
            Transform::from_xyz(pos.x, 0.0, pos.y).with_rotation(yaw(heading)),
        ))
        .id();
    if let Some(ai) = ai {
        commands.entity(id).insert(ai);
        add_car_lights(commands, art, id);
    }
    if police {
        commands.spawn((
            Beacon,
            Mesh3d(art.police_glow.clone()),
            MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
            Transform::default(),
            ChildOf(id),
        ));
    }
    id
}

fn lane_for(e: &Edge, seed: f32) -> (f32, f32) {
    let lanes = e.lanes.max(1) as f32;
    let lane = if e.oneway {
        let li = (seed * lanes).floor().min(lanes - 1.0);
        -e.width / 2.0 + (li + 0.5) * e.width / lanes
    } else {
        let per = (lanes / 2.0).floor().max(1.0);
        let li = (seed * per).floor().min(per - 1.0);
        (li + 0.5) * (e.width / 2.0) / per
    };
    let cruise = if e.lanes >= 3 || (e.oneway && e.lanes >= 2) { 15.5 } else if e.width >= 6.9 { 11.0 } else { 8.5 };
    (lane, cruise)
}

/// Pick a random spot on a real road somewhere in a ring around `near`.
fn road_spawn(city: &City, rng: &mut Rng, near: Vec2, dmin: f32, dmax: f32) -> Option<(Vec2, f32, CarAi)> {
    let p = near + Vec2::from_angle(rng.range(0.0, TAU)) * rng.range(dmin, dmax);
    let list = city.drive_in(cell_of(p));
    if list.is_empty() || !city.area_ready(p) {
        return None;
    }
    let ei = list[rng.int(list.len() as u32) as usize];
    let e = &city.drive.edges[ei as usize];
    let n = e.pts.len();
    let fwd = e.oneway || rng.chance(0.5);
    let k = 1 + rng.int((n - 1) as u32) as usize;
    let (a, b) = (e.pt(fwd, k - 1), e.pt(fwd, k));
    let d = (b - a).normalize_or(Vec2::X);
    let seed = rng.f();
    let (lane, cruise) = lane_for(e, seed);
    let speed_k = rng.range(0.85, 1.15);
    let pos = a.lerp(b, rng.f()) + right_of(d) * lane;
    Some((
        pos,
        d.y.atan2(d.x),
        CarAi { edge: ei, fwd, idx: k, lane_seed: seed, lane, speed_k, cruise: cruise * speed_k, stuck: 0.0, shoot_cd: 1.0, panic: 0.0, done: false, bus: None },
    ))
}

fn nearest_walk(city: &City, pos: Vec2) -> Option<(u32, usize)> {
    let src = city.src(cell_of(pos))?;
    let mut best: Option<(f32, u32, usize)> = None;
    for &ei in &src.walk {
        let e = &city.walk.edges[ei as usize];
        for k in 1..e.pts.len() {
            let d = e.pts[k].distance_squared(pos);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, ei, k));
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

fn spawn_ped(commands: &mut Commands, art: &Art, rng: &mut Rng, pos: Vec2, on: Option<(u32, bool, usize)>, flee_from: Option<Vec2>) {
    let var = rng.int(art.peds.len() as u32) as usize;
    let mesh = art.peds[var][0].clone();
    let (edge, fwd, idx) = on.unwrap_or((u32::MAX, true, 1));
    let mut ped = Ped {
        edge,
        fwd,
        idx,
        off: rng.range(-0.8, 0.8),
        var,
        frame: 0,
        speed: rng.range(1.5, 2.6),
        flee: 0.0,
        flee_dir: Vec2::X,
        dead: 0.0,
    };
    if let Some(f) = flee_from {
        ped.flee = 6.0;
        ped.flee_dir = (pos - f).normalize_or(Vec2::X);
    }
    commands.spawn((ped, Mesh3d(mesh), MeshMaterial3d(art.mat.clone()), Transform::from_xyz(pos.x, 0.0, pos.y)));
}

fn puff(commands: &mut Commands, mesh: Handle<Mesh>, mat: Handle<StandardMaterial>, pos: Vec3, vel: Vec3, life: f32, size: f32) {
    commands.spawn((
        Particle { vel, life, max: life, size },
        NotShadowCaster,
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        // callers give heights above the ground; put the puff on the terrain
        Transform::from_translation(pos + Vec3::Y * terrain(pos.xz())).with_scale(Vec3::splat(size)),
    ));
}

pub fn burst(commands: &mut Commands, art: &Art, rng: &mut Rng, kind: usize, pos: Vec3, n: usize, speed: f32, life: f32, size: f32) {
    for _ in 0..n {
        let a = rng.range(0.0, TAU);
        let v = Vec3::new(a.cos(), rng.range(0.0, 0.6), a.sin()) * speed * rng.range(0.25, 1.0);
        puff(commands, art.puffs[kind].clone(), art.glow.clone(), pos, v, life * rng.range(0.5, 1.0), size * rng.range(0.6, 1.2));
    }
}

// ---- player ---------------------------------------------------------------
pub fn player_control(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    art: Res<Art>,
    world: Res<City>,
    mut game: ResMut<Game>,
    player: Single<(&mut Transform, &mut Visibility, &mut Mesh3d), With<Player>>,
    mut cars: Query<(Entity, &Transform, &mut Car, Has<CarAi>), Without<Player>>,
) {
    let dt = time.delta_secs();
    let (mut pt, mut pvis, mut pmesh) = player.into_inner();
    let game = &mut *game;
    game.fire_cd -= dt;
    if game.dead_t > 0.0 {
        return;
    }
    let use_key = keys.just_pressed(KeyCode::KeyF) || keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::Enter);
    let ax = (keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight)) as i32 as f32
        - (keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft)) as i32 as f32;
    let ay = (keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp)) as i32 as f32
        - (keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown)) as i32 as f32;
    if keys.just_pressed(KeyCode::KeyQ) {
        game.smg = !game.smg;
    }

    if let Some(ce) = game.in_car {
        let Ok((_, ct, mut car, _)) = cars.get_mut(ce) else {
            game.in_car = None;
            *pvis = Visibility::Inherited;
            return;
        };
        car.throttle = ay;
        car.steer = ax;
        car.handbrake = keys.pressed(KeyCode::Space);
        let cp = p2(ct);
        game.pos = cp;
        game.y = if car.y.is_nan() { 0.0 } else { car.y };
        game.vel = car.vel;
        game.heading = car.heading;
        pt.translation = Vec3::new(cp.x, game.y + terrain(cp), cp.y);
        if use_key && car.vel.length() < 9.0 {
            let r = right_of(Vec2::from_angle(car.heading));
            let mut out = cp - r * 2.1;
            if world.solid(out) {
                out = cp + r * 2.1;
            }
            car.player = false;
            car.throttle = 0.0;
            car.steer = 0.0;
            car.handbrake = true;
            game.in_car = None;
            game.pos = out;
            game.vel = Vec2::ZERO;
            pt.translation = Vec3::new(out.x, game.y + terrain(out), out.y);
            *pvis = Visibility::Inherited;
        }
        return;
    }

    // on foot
    let dir = Vec2::new(ax, -ay).normalize_or_zero();
    // walking animation: alternate the two stride meshes while moving
    let stride = if dir == Vec2::ZERO { 0 } else { ((game.time * 5.5) as usize) & 1 };
    if pmesh.0 != art.player[stride] {
        pmesh.0 = art.player[stride].clone();
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) { 10.5 } else { 6.5 };
    game.vel = dir * speed;
    let want = game.pos + game.vel * dt;
    let (np, _) = if game.y.abs() < 2.5 { world.push_out(want, 0.5) } else { (want, false) };
    game.travel_hold -= dt;
    if world.area_ready(np) {
        game.y = world.ground_y(np, game.y);
    }
    game.pos = np;
    let aim = game.aim - game.pos;
    if aim.length_squared() > 0.01 {
        game.heading = aim.y.atan2(aim.x);
    }
    pt.translation = Vec3::new(game.pos.x, game.y + terrain(game.pos) + 0.2, game.pos.y);
    pt.rotation = yaw(game.heading);

    if use_key {
        let mut best: Option<(f32, Entity)> = None;
        for (e, ct, car, _) in cars.iter() {
            let d = p2(ct).distance(game.pos);
            if d < 4.5 && car.health > 0.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, e));
            }
        }
        if let Some((_, e)) = best {
            let (_, ct, mut car, had_ai) = cars.get_mut(e).unwrap();
            if had_ai {
                commands.entity(e).remove::<CarAi>();
                let side = p2(ct) + right_of(Vec2::from_angle(car.heading)) * 2.4;
                spawn_ped(&mut commands, &art, &mut game.rng, side, None, Some(p2(ct)));
                game.crime(if car.police { 30.0 } else { 9.0 });
            }
            if !car.lights {
                car.lights = true;
                add_car_lights(&mut commands, &art, e);
            }
            car.player = true;
            car.handbrake = false;
            game.in_car = Some(e);
            *pvis = Visibility::Hidden;
            return;
        }
    }

    if mouse.pressed(MouseButton::Left) && game.fire_cd <= 0.0 && game.started && !game.map_view && !game.ui_hover {
        let smg = game.smg && game.ammo > 0;
        if smg {
            game.ammo -= 1;
        }
        game.fire_cd = if smg { 0.075 } else { 0.3 };
        let spread = if smg { 0.07 } else { 0.015 };
        let a = game.heading + game.rng.range(-spread, spread);
        let f = Vec2::from_angle(a);
        let o = game.pos + f * 0.9;
        commands.spawn((
            Bullet { vel: f * 70.0, life: 0.9, from_player: true },
            Mesh3d(art.bullet.clone()),
            MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
            Transform::from_xyz(o.x, terrain(o) + game.y + 1.2, o.y).with_rotation(yaw(a)),
        ));
        puff(&mut commands, art.puffs[3].clone(), art.glow.clone(), Vec3::new(o.x, game.y + 1.3, o.y), Vec3::ZERO, 0.06, 2.6);
        let pos = game.pos;
        game.fear.push((pos, 22.0));
        game.crime(0.6);
    }
}

// ---- traffic / police AI --------------------------------------------------
pub fn car_ai(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    city: Res<City>,
    transit: Res<crate::transit::Transit>,
    mut game: ResMut<Game>,
    all: Query<(Entity, &Transform, Has<Long>), With<Car>>,
    peds: Query<(&Transform, &Ped)>,
    mut q: Query<(Entity, &Transform, &mut Car, &mut CarAi)>,
) {
    let dt = time.delta_secs();
    let obstacles: Vec<(Entity, Vec2, f32, f32)> =
        all.iter().map(|(e, t, long)| (e, p2(t), t.translation.y, if long { 5.0 } else { 0.0 })).collect();
    let ped_pos: Vec<Vec2> = peds.iter().filter(|(_, p)| p.dead <= 0.0).map(|(t, _)| p2(t)).collect();
    let wanted = game.wanted();
    let ppos = game.pos;
    let on_foot = game.in_car.is_none();
    for (e, t, mut car, mut ai) in &mut q {
        let pos = p2(t);
        let f = Vec2::from_angle(car.heading);
        let r = right_of(f);
        let vf = car.vel.dot(f);
        let chasing = car.police && wanted > 0 && game.dead_t <= 0.0;
        let to_player = ppos - pos;
        let pdist = to_player.length();
        let direct = chasing && pdist < 34.0;

        // follow the road polyline
        let mut target = pos + f * 5.0;
        let mut near_turn = false;
        let mut hold = false;
        let mut bus = ai.bus.take();
        if let Some(bp) = bus.as_mut() {
            // scheduled service: follow the route path and call at each stop
            let rt = &transit.routes[bp.route as usize];
            if bp.idx >= rt.pts.len() {
                ai.done = true;
            } else {
                let cur = rt.pts[bp.idx];
                let d = (cur - rt.pts[bp.idx - 1]).normalize_or(Vec2::X);
                target = cur + right_of(d) * 1.6;
                if (target - pos).dot(d) < 2.5 {
                    bp.idx += 1;
                }
                if let Some(sp) = rt.stops.get(bp.stop) {
                    let ds = sp.distance(pos);
                    if ds < 13.0 {
                        if bp.dwell == 0.0 {
                            bp.dwell = 5.5;
                        }
                        bp.dwell -= dt;
                        if bp.dwell > 0.4 {
                            hold = true;
                        } else {
                            bp.stop += 1;
                            bp.dwell = 0.0;
                        }
                    } else if ds < 60.0 && (*sp - pos).dot(f) < -6.0 {
                        bp.stop += 1; // overshot or bypassed
                    }
                }
            }
        }
        let is_bus = bus.is_some();
        ai.bus = bus;
        if !ai.done && !is_bus {
            let ed = &city.drive.edges[ai.edge as usize];
            let n = ed.pts.len();
            let cur = ed.pt(ai.fwd, ai.idx);
            let d = (cur - ed.pt(ai.fwd, ai.idx - 1)).normalize_or(Vec2::X);
            target = cur + right_of(d) * ai.lane;
            near_turn = ai.idx == n - 1 && target.distance(pos) < 13.0;
            if (target - pos).dot(d) < 1.5 {
                ai.idx += 1;
                if ai.idx >= n {
                    let node = ed.end(ai.fwd);
                    let exits = city.drive.exits(node, ai.edge);
                    if exits.is_empty() {
                        ai.done = true;
                    } else {
                        let far = |x: &(u32, bool)| {
                            let e2 = &city.drive.edges[x.0 as usize];
                            e2.pt(x.1, e2.pts.len() - 1).distance_squared(ppos)
                        };
                        let pick = if chasing {
                            *exits.iter().min_by(|a, b| far(a).total_cmp(&far(b))).unwrap()
                        } else {
                            exits[game.rng.int(exits.len() as u32) as usize]
                        };
                        ai.edge = pick.0;
                        ai.fwd = pick.1;
                        ai.idx = 1;
                        let (lane, cruise) = lane_for(&city.drive.edges[pick.0 as usize], ai.lane_seed);
                        ai.lane = lane;
                        ai.cruise = cruise * ai.speed_k;
                    }
                }
            }
        }

        let aim_at = if direct { ppos } else { target };
        let to = aim_at - pos;
        let diff = wrap(to.y.atan2(to.x) - car.heading);
        car.steer = (diff * 2.2).clamp(-1.0, 1.0);
        ai.panic -= dt;
        let mut want = if chasing { 22.0 } else if ai.panic > 0.0 { 18.0 } else { ai.cruise };
        if diff.abs() > 0.35 {
            want = want.min(if chasing { 9.0 } else { 6.0 });
        }
        if near_turn && !chasing {
            want = want.min(8.0);
        }
        if ai.done && !direct {
            want = 0.0;
        }
        if hold {
            want = 0.0;
        }
        // brake for things ahead
        let ignore = ai.stuck > 5.0 || ai.panic > 0.0;
        if !ignore && !direct {
            let reach = 5.0 + vf.max(0.0) * 0.9;
            let mut blocked = false;
            for (oe, op, oy, olen) in &obstacles {
                if (*oy - t.translation.y).abs() > 2.5 {
                    continue;
                }
                if *oe == e {
                    continue;
                }
                let dv = *op - pos;
                let ahead = dv.dot(f);
                if ahead > 0.5 && ahead < reach + olen + car.half - 1.25 && dv.dot(r).abs() < 1.9 {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                let me = on_foot.then_some(&ppos);
                for op in ped_pos.iter().chain(me) {
                    let dv = *op - pos;
                    let ahead = dv.dot(f);
                    if ahead > 0.5 && ahead < reach && dv.dot(r).abs() < 1.5 {
                        blocked = true;
                        break;
                    }
                }
            }
            if blocked {
                want = 0.0;
            }
        }
        car.throttle = if vf < want - 0.5 { if chasing { 1.0 } else { 0.6 } } else if vf > want + 1.5 { -1.0 } else { 0.0 };
        car.handbrake = want == 0.0 && vf.abs() < 2.0;
        if vf.abs() < 0.6 {
            ai.stuck += dt;
            if ai.stuck > 8.0 {
                ai.stuck = 0.0;
            }
            // police that are wedged against something back up
            if chasing && ai.stuck > 1.5 && ai.stuck < 3.0 {
                car.throttle = -1.0;
                car.steer = -car.steer;
                car.handbrake = false;
            }
        } else if vf > 2.0 {
            ai.stuck = 0.0;
        }

        // police gunfire
        if chasing && wanted >= 2 {
            ai.shoot_cd -= dt;
            if ai.shoot_cd <= 0.0 && pdist < 24.0 && pdist > 3.5 {
                ai.shoot_cd = (1.3 - wanted as f32 * 0.18).max(0.35) * game.rng.range(0.7, 1.3);
                let a = to_player.y.atan2(to_player.x) + game.rng.range(-0.12, 0.12);
                let bf = Vec2::from_angle(a);
                let o = pos + bf * 3.3;
                commands.spawn((
                    Bullet { vel: bf * 55.0, life: 0.8, from_player: false },
                    Mesh3d(art.bullet.clone()),
                    MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
                    Transform::from_xyz(o.x, t.translation.y + 1.2, o.y).with_rotation(yaw(a)),
                ));
            }
        }
    }
}

// ---- vehicle physics ------------------------------------------------------
pub fn car_physics(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    world: Res<City>,
    mut game: ResMut<Game>,
    mut q: Query<(Entity, &mut Transform, &mut Car)>,
) {
    let dt = time.delta_secs().min(0.05);
    let game = &mut *game;
    for (e, mut t, mut car) in &mut q {
        let mut pos = p2(&t);
        let f = Vec2::from_angle(car.heading);
        let r = right_of(f);
        let mut vf = car.vel.dot(f);
        let mut vl = car.vel.dot(r);
        let dead = car.health <= 0.0;
        let thr = if dead { 0.0 } else { car.throttle };
        let max = car.max_speed * if car.health < 35.0 { 0.7 } else { 1.0 };
        if thr > 0.0 {
            if vf < 0.0 {
                vf += 26.0 * dt;
            } else {
                vf += 15.0 * thr * dt * (1.0 - vf / max).max(0.0);
            }
        } else if thr < 0.0 {
            if vf > 0.5 {
                vf -= 30.0 * dt;
            } else {
                vf += 9.0 * thr * dt * (1.0 + vf / (max * 0.4)).max(0.0);
            }
        } else {
            vf -= vf.signum() * (2.5 * dt).min(vf.abs());
        }
        vf -= vf * 0.12 * dt;
        let grip = if car.handbrake { 2.2 } else { 8.0 };
        vl *= (-grip * dt).exp();
        if car.handbrake {
            vf *= (-1.6 * dt).exp();
        }
        car.vel = f * vf + r * vl;
        let turn = car.steer * 2.7 * (vf / 7.0).clamp(-1.0, 1.0) * (1.0 - 0.45 * (vf.abs() / 26.0).min(1.0));
        car.heading = wrap(car.heading + turn * dt);
        pos += car.vel * dt;

        // static collisions: two circles along the body
        for off in [car.half, -car.half] {
            let c = pos + f * off;
            let (np, hit) = if car.y.abs() < 2.5 { world.push_out(c, 1.05) } else { (c, false) };
            if hit {
                let delta = np - c;
                pos += delta;
                let n = delta.normalize_or_zero();
                let vn = car.vel.dot(n);
                if vn < 0.0 {
                    car.vel -= n * vn * 1.35;
                    let impact = -vn;
                    if impact > 5.0 {
                        car.health -= (impact - 5.0) * 2.2;
                        burst(&mut commands, &art, &mut game.rng, 0, Vec3::new(c.x, car.y + 0.8, c.y), 5, 6.0, 0.3, 0.5);
                    }
                    // nudge heading along the wall
                    let tang = right_of(n);
                    let side = f.dot(tang).signum();
                    car.heading = wrap(car.heading + wrap((tang * side).to_angle() - car.heading) * (3.0 * dt).min(1.0));
                }
            }
        }
        // height: follow bridge decks and tunnel floors, fall if driven off an edge
        if car.y.is_nan() {
            car.y = world.surface_for(pos, f);
        }
        let gy = world.ground_y(pos, car.y);
        if car.y > gy + 0.4 {
            car.vy -= 25.0 * dt;
            car.y += car.vy * dt;
            if car.y <= gy {
                if car.vy < -9.0 {
                    car.health -= (-car.vy - 9.0) * 5.0;
                }
                car.y = gy;
                car.vy = 0.0;
            }
        } else {
            car.y = gy;
            car.vy = 0.0;
        }
        t.translation = Vec3::new(pos.x, car.y + terrain(pos) + 0.3, pos.y);
        t.rotation = yaw(car.heading);

        // damage effects
        if car.health < 40.0 && game.rng.chance(dt * 14.0) {
            let p = pos + f * 1.6;
            puff(&mut commands, art.puffs[2].clone(), art.glow.clone(), Vec3::new(p.x, car.y + 1.6, p.y), Vec3::new(0.5, 2.0, 0.3), 1.0, 2.2);
        }
        if dead {
            car.burn += dt;
            if game.rng.chance(dt * 30.0) {
                let p = pos + f * game.rng.range(-1.0, 1.8);
                puff(&mut commands, art.puffs[1].clone(), art.glow.clone(), Vec3::new(p.x, car.y + 1.4, p.y), Vec3::new(0.0, 3.0, 0.0), 0.5, 2.4);
            }
            if car.burn > 2.6 {
                game.booms.push(pos);
                if game.in_car == Some(e) {
                    game.in_car = None;
                    game.health -= 75.0;
                }
                commands.entity(e).despawn();
            }
        }
    }
}

pub fn car_collisions(mut game: ResMut<Game>, mut q: Query<(Entity, &mut Transform, &mut Car)>) {
    let snap: Vec<(Entity, Vec2, Vec2, Vec2)> =
        q.iter().map(|(e, t, c)| (e, p2(t), Vec2::from_angle(c.heading), c.vel)).collect();
    let ys: Vec<f32> = q.iter().map(|(_, t, _)| t.translation.y).collect();
    let hs: Vec<f32> = q.iter().map(|(_, _, c)| c.half).collect();
    let mut fix: Vec<(Entity, Vec2, Vec2, f32)> = vec![];
    for i in 0..snap.len() {
        for j in (i + 1)..snap.len() {
            let (ea, pa, fa, va) = snap[i];
            let (eb, pb, fb, vb) = snap[j];
            if pa.distance(pb) > 3.0 + hs[i] + hs[j] || (ys[i] - ys[j]).abs() > 2.5 {
                continue;
            }
            for oa in [hs[i], -hs[i], 0.0] {
                for ob in [hs[j], -hs[j], 0.0] {
                    let ca = pa + fa * oa;
                    let cb = pb + fb * ob;
                    let d = ca - cb;
                    let l = d.length();
                    if l < 2.1 && l > 1e-3 {
                        let n = d / l;
                        let pen = 2.1 - l;
                        let rel = (va - vb).dot(n);
                        let imp = if rel < 0.0 { -rel } else { 0.0 };
                        fix.push((ea, n * pen * 0.5, n * imp * 0.55, imp));
                        fix.push((eb, -n * pen * 0.5, -n * imp * 0.55, imp));
                    }
                }
            }
        }
    }
    for (e, dp, dv, imp) in fix {
        if let Ok((_, mut t, mut car)) = q.get_mut(e) {
            t.translation += Vec3::new(dp.x, 0.0, dp.y) * 0.5;
            car.vel += dv * 0.5;
            if imp > 6.0 {
                car.health -= (imp - 6.0) * 0.8;
            }
        }
    }
    // player on foot vs cars
    if game.in_car.is_none() && game.dead_t <= 0.0 {
        for (k, (_, p, f, v)) in snap.iter().enumerate() {
            if (ys[k] - (game.y + terrain(game.pos))).abs() > 2.0 {
                continue;
            }
            for o in [1.25f32, -1.25] {
                let c = *p + *f * o;
                let d = game.pos - c;
                let l = d.length();
                if l < 1.5 && l > 1e-3 {
                    let n = d / l;
                    game.pos += n * (1.5 - l);
                    let sp = v.dot(n);
                    if sp > 5.0 {
                        game.health -= (sp - 4.0) * 3.0;
                        game.pos += n * 1.5;
                    }
                }
            }
        }
    }
}

// ---- pedestrians ----------------------------------------------------------
fn kill_ped(commands: &mut Commands, art: &Art, rng: &mut Rng, t: &mut Transform, ped: &mut Ped) {
    ped.dead = 14.0;
    t.scale = Vec3::new(1.4, 0.12, 1.2);
    let p = t.translation;
    puff(commands, art.blood.clone(), art.mat.clone(), Vec3::new(p.x, 0.04, p.z), Vec3::ZERO, 25.0, rng.range(1.6, 2.6));
    if rng.chance(0.45) {
        commands.spawn((
            Pickup { kind: 2, cell: None, value: 20 + rng.int(120) as i64 },
            Mesh3d(art.pickups[2].clone()),
            MeshMaterial3d(art.mat.clone()),
            Transform::from_xyz(p.x + 1.0, terrain(Vec2::new(p.x + 1.0, p.z + 0.6)), p.z + 0.6).with_scale(Vec3::splat(0.6)),
        ));
    }
}

pub fn peds(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    world: Res<City>,
    mut game: ResMut<Game>,
    cars: Query<(&Transform, &Car), Without<Ped>>,
    mut q: Query<(Entity, &mut Transform, &mut Ped, &mut Mesh3d), Without<Car>>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let carv: Vec<(Vec2, Vec2, f32, bool, f32)> = cars
        .iter()
        .map(|(t, c)| (p2(t), Vec2::from_angle(c.heading), c.vel.length(), c.player, t.translation.y))
        .collect();
    for (e, mut t, mut ped, mut pmesh) in &mut q {
        if ped.dead > 0.0 {
            ped.dead -= dt;
            if ped.dead <= 0.0 {
                commands.entity(e).despawn();
            }
            continue;
        }
        let mut pos = p2(&t);
        for (fp, fr) in &game.fear {
            if fp.distance(pos) < *fr {
                ped.flee = game.rng.range(4.0, 8.0);
                ped.flee_dir = (pos - *fp).normalize_or(Vec2::X);
                ped.edge = u32::MAX;
            }
        }
        let mut dir = Vec2::ZERO;
        if ped.flee > 0.0 {
            ped.flee -= dt;
            dir = ped.flee_dir;
            let (np, hit) = world.push_out(pos + dir * 6.5 * dt, 0.45);
            if hit {
                ped.flee_dir = right_of(ped.flee_dir);
            }
            pos = np;
        } else if ped.edge == u32::MAX {
            match nearest_walk(&world, pos) {
                Some((edge, idx)) => {
                    ped.edge = edge;
                    ped.idx = idx;
                    ped.fwd = true;
                }
                None => {
                    ped.flee = 3.0;
                    ped.flee_dir = Vec2::from_angle(game.rng.range(0.0, TAU));
                }
            }
        } else {
            let ed = &world.walk.edges[ped.edge as usize];
            let cur = ed.pt(ped.fwd, ped.idx);
            let d = (cur - ed.pt(ped.fwd, ped.idx - 1)).normalize_or(Vec2::X);
            let to = cur + right_of(d) * ped.off - pos;
            if to.length() < 0.9 || to.dot(d) < 0.0 {
                ped.idx += 1;
                if ped.idx >= ed.pts.len() {
                    let exits = world.walk.exits(ed.end(ped.fwd), ped.edge);
                    if exits.is_empty() {
                        ped.fwd = !ped.fwd;
                    } else {
                        let pick = exits[game.rng.int(exits.len() as u32) as usize];
                        ped.edge = pick.0;
                        ped.fwd = pick.1;
                    }
                    ped.idx = 1;
                }
            }
            dir = to.normalize_or_zero();
            pos += dir * ped.speed * dt;
        }
        let py = world.ground_y(pos, t.translation.y - terrain(p2(&t)));
        let py_abs = py + terrain(pos) + 0.2;
        t.translation = Vec3::new(pos.x, py_abs, pos.y);
        if dir != Vec2::ZERO {
            t.rotation = yaw(dir.y.atan2(dir.x));
        }
        // walking animation (faster when running)
        let rate = if ped.flee > 0.0 { 7.0 } else { 3.2 };
        let frame = if dir == Vec2::ZERO { 0 } else { (((game.time + ped.off) * rate) as u8) & 1 };
        if frame != ped.frame {
            ped.frame = frame;
            pmesh.0 = art.peds[ped.var][frame as usize].clone();
        }
        for (cp, cf, sp, by_player, cy) in &carv {
            if *sp > 4.0 && (*cy - py_abs).abs() < 2.0 {
                let d = pos - *cp;
                if d.dot(*cf).abs() < 2.6 && d.dot(right_of(*cf)).abs() < 1.35 {
                    kill_ped(&mut commands, &art, &mut game.rng, &mut t, &mut ped);
                    if *by_player {
                        game.crime(14.0);
                        game.fear.push((pos, 18.0));
                    }
                    break;
                }
            }
        }
    }
    game.fear.clear();
}

// ---- bullets, explosions, particles ---------------------------------------
pub fn bullets(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    world: Res<City>,
    mut game: ResMut<Game>,
    mut q: Query<(Entity, &mut Transform, &mut Bullet)>,
    mut cars: Query<(Entity, &Transform, &mut Car, Option<&mut CarAi>), Without<Bullet>>,
    mut peds: Query<(&mut Transform, &mut Ped), (Without<Bullet>, Without<Car>)>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    for (e, mut t, mut b) in &mut q {
        b.life -= dt;
        let step = b.vel * dt;
        let mut pos = p2(&t);
        let mut done = b.life <= 0.0;
        // sub-step so fast bullets don't tunnel
        for _ in 0..2 {
            if done {
                break;
            }
            pos += step * 0.5;
            if world.solid(pos) {
                done = true;
                break;
            }
            for (ce, ct, mut car, ai) in &mut cars {
                if b.from_player && game.in_car == Some(ce) {
                    continue;
                }
                if (ct.translation.y + 1.2 - t.translation.y).abs() > 2.5 {
                    continue;
                }
                let d = pos - p2(ct);
                let f = Vec2::from_angle(car.heading);
                if d.dot(f).abs() < 2.3 && d.dot(right_of(f)).abs() < 1.1 {
                    car.health -= if b.from_player { 13.0 } else { 6.0 };
                    if b.from_player {
                        game.crime(if car.police { 8.0 } else { 3.0 });
                        if let Some(mut ai) = ai {
                            ai.panic = 6.0;
                        }
                    }
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
            for (mut pt, mut ped) in &mut peds {
                if ped.dead <= 0.0 && p2(&pt).distance_squared(pos) < 0.6 {
                    kill_ped(&mut commands, &art, &mut game.rng, &mut pt, &mut ped);
                    if b.from_player {
                        game.crime(16.0);
                    }
                    done = true;
                    break;
                }
            }
            if !b.from_player && game.in_car.is_none() && game.dead_t <= 0.0 && pos.distance_squared(game.pos) < 0.6 {
                game.health -= 9.0;
                done = true;
            }
        }
        // bullets skim the terrain rather than flying dead level
        let lift = terrain(pos) - terrain(p2(&t));
        t.translation = Vec3::new(pos.x, t.translation.y + lift, pos.y);
        if done {
            burst(&mut commands, &art, &mut game.rng, 0, Vec3::new(pos.x, t.translation.y - terrain(pos), pos.y), 4, 7.0, 0.25, 0.45);
            commands.entity(e).despawn();
        }
    }
}

pub fn explosions(
    mut commands: Commands,
    art: Res<Art>,
    mut game: ResMut<Game>,
    mut cars: Query<(&Transform, &mut Car)>,
    mut peds: Query<(&mut Transform, &mut Ped), Without<Car>>,
) {
    let game = &mut *game;
    let booms = std::mem::take(&mut game.booms);
    for b in booms {
        let c = Vec3::new(b.x, 1.5, b.y);
        burst(&mut commands, &art, &mut game.rng, 1, c, 26, 16.0, 0.9, 4.5);
        burst(&mut commands, &art, &mut game.rng, 0, c, 20, 28.0, 0.6, 0.8);
        burst(&mut commands, &art, &mut game.rng, 2, c + Vec3::Y, 10, 5.0, 2.2, 6.0);
        puff(&mut commands, art.puffs[3].clone(), art.glow.clone(), c, Vec3::ZERO, 0.18, 30.0);
        game.fear.push((b, 45.0));
        game.shake = 0.5;
        for (t, mut car) in &mut cars {
            let d = p2(t) - b;
            let l = d.length();
            if l < 10.0 && l > 0.1 {
                car.health -= 70.0 * (1.0 - l / 10.0);
                car.vel += d / l * 14.0 * (1.0 - l / 10.0);
            }
        }
        for (mut t, mut ped) in &mut peds {
            if ped.dead <= 0.0 && p2(&t).distance(b) < 8.0 {
                kill_ped(&mut commands, &art, &mut game.rng, &mut t, &mut ped);
            }
        }
        let l = game.pos.distance(b);
        if l < 9.0 && game.in_car.is_none() {
            game.health -= 80.0 * (1.0 - l / 9.0);
        }
    }
}

pub fn particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transform, &mut Particle)>) {
    let dt = time.delta_secs();
    for (e, mut t, mut p) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        t.translation += p.vel * dt;
        p.vel *= (-2.5 * dt).exp();
        let k = (p.life / p.max * 3.0).min(1.0);
        t.scale = Vec3::splat(p.size * k.max(0.02));
    }
}

// ---- population -----------------------------------------------------------
pub fn population(
    mut commands: Commands,
    art: Res<Art>,
    city: Res<City>,
    mut game: ResMut<Game>,
    time: Res<Time>,
    cars: Query<(Entity, &Transform, &Car, Option<&CarAi>)>,
    peds: Query<(Entity, &Transform), With<Ped>>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.1;
    let game = &mut *game;
    let focus = game.pos;
    let vh = game.view_half + Vec2::splat(10.0);
    let offscreen = |p: Vec2| (p.x - focus.x).abs() > vh.x || (p.y - focus.y).abs() > vh.y;

    let (mut traffic, mut police) = (0, 0);
    for (e, t, car, ai) in &cars {
        let d = p2(t).distance(focus);
        if car.player {
            continue;
        }
        let stranded = ai.is_some_and(|a| a.done) && offscreen(p2(t));
        if d > 175.0 || (ai.is_some() && !car.police && d > 150.0) || stranded {
            commands.entity(e).despawn();
            continue;
        }
        if ai.is_some() {
            if car.police {
                police += 1;
            } else {
                traffic += 1;
            }
        }
    }
    if traffic < 20 {
        for _ in 0..3 {
            if let Some((pos, h, ai)) = road_spawn(&city, &mut game.rng, focus, 60.0, 140.0) {
                let clear = cars.iter().all(|(_, t, _, _)| p2(t).distance_squared(pos) > 120.0);
                if offscreen(pos) && clear {
                    let kind = game.rng.int(8) as usize;
                    spawn_car(&mut commands, &art, pos, h, kind, Some(ai));
                    break;
                }
            }
        }
    }
    let want_police = if game.dead_t > 0.0 { 0 } else { (game.wanted() as i32).min(4) };
    if police < want_police {
        if let Some((pos, h, ai)) = road_spawn(&city, &mut game.rng, focus, 70.0, 120.0) {
            if offscreen(pos) {
                spawn_car(&mut commands, &art, pos, h, POLICE_KIND, Some(ai));
            }
        }
    }

    let mut np = 0;
    for (e, t) in &peds {
        if p2(t).distance(focus) > 125.0 {
            commands.entity(e).despawn();
        } else {
            np += 1;
        }
    }
    if np < 45 {
        for _ in 0..4 {
            let p = focus + Vec2::from_angle(game.rng.range(0.0, TAU)) * game.rng.range(40.0, 110.0);
            let Some(src) = city.src(cell_of(p)) else { continue };
            if src.walk.is_empty() {
                continue;
            }
            let ei = src.walk[game.rng.int(src.walk.len() as u32) as usize];
            let ed = &city.walk.edges[ei as usize];
            let fwd = game.rng.chance(0.5);
            let k = 1 + game.rng.int((ed.pts.len() - 1) as u32) as usize;
            let pos = ed.pt(fwd, k - 1).lerp(ed.pt(fwd, k), game.rng.f());
            if offscreen(pos) && pos.distance(focus) < 120.0 {
                spawn_ped(&mut commands, &art, &mut game.rng, pos, Some((ei, fwd, k)), None);
            }
        }
    }
}

// ---- pickups, jobs, wanted, death ----------------------------------------
#[derive(Component)]
pub struct TargetMarker;
#[derive(Component)]
pub struct GuideArrow;

pub fn pickups_and_jobs(
    mut commands: Commands,
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut world: ResMut<City>,
    mut q: Query<(Entity, &mut Transform, &Pickup)>,
    mut marker: Single<(&mut Transform, &mut Visibility), (With<TargetMarker>, Without<Pickup>, Without<GuideArrow>)>,
    mut arrow: Single<(&mut Transform, &mut Visibility), (With<GuideArrow>, Without<Pickup>, Without<TargetMarker>)>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let reach = if game.in_car.is_some() { 2.8 } else { 1.6 };
    for (e, mut t, pk) in &mut q {
        t.rotate_y(dt * 2.5);
        if p2(&t).distance(game.pos) < reach && game.dead_t <= 0.0 {
            match pk.kind {
                0 => {
                    if game.health >= 100.0 {
                        continue;
                    }
                    game.health = (game.health + 50.0).min(100.0);
                    game.say("MEDKIT +50", 1.5);
                }
                1 => {
                    game.ammo += 60;
                    game.smg = true;
                    game.say("SMG AMMO +60", 1.5);
                }
                _ => {
                    let v = if pk.value > 0 { pk.value } else { 150 + game.rng.int(350) as i64 };
                    game.cash += v;
                    game.say(&format!("+${v}"), 1.2);
                }
            }
            if let Some(c) = pk.cell {
                world.collected.insert(c);
            }
            commands.entity(e).despawn();
        }
    }

    // courier jobs
    let mut guide: Option<Vec2> = None;
    match game.mission {
        Mission::None => {
            let pc = cell_of(game.pos);
            let mut best = f32::MAX;
            for dz in -8..=8 {
                for dx in -8..=8 {
                    let c = pc + IVec2::new(dx, dz);
                    if world.has_job(c) {
                        let jp = world.src(c).unwrap().poi.unwrap();
                        let d = jp.distance(game.pos);
                        if d < best {
                            best = d;
                            guide = Some(jp);
                        }
                    }
                }
            }
            if best < 3.0 && game.dead_t <= 0.0 && game.job_cd <= 0.0 {
                for _ in 0..60 {
                    let a = game.rng.range(0.0, TAU);
                    let dist = game.rng.range(4.0, 9.0) + (game.jobs_done as f32 * 0.6).min(10.0);
                    let tc = pc + IVec2::new((a.cos() * dist).round() as i32, (a.sin() * dist).round() as i32);
                    let list = world.drive_in(tc);
                    if list.is_empty() {
                        continue;
                    }
                    let ed = &world.drive.edges[list[game.rng.int(list.len() as u32) as usize] as usize];
                    let target = ed.pts[ed.pts.len() / 2];
                    let meters = target.distance(game.pos);
                    game.mission = Mission::Deliver { target, reward: (meters * 2.2) as i64 + 200, time: meters / 8.0 + 30.0 };
                    game.dest = Some(target);
                    game.route.clear();
                    let street = world.street(target).to_string();
                    game.say(&format!("DELIVER TO {street}"), 3.5);
                    break;
                }
            }
            game.job_cd -= dt;
        }
        Mission::Deliver { target, reward, time } => {
            guide = Some(target);
            let left = time - dt;
            if game.pos.distance(target) < 4.0 {
                game.cash += reward;
                game.jobs_done += 1;
                game.say(&format!("DELIVERED  +${reward}"), 3.0);
                game.mission = Mission::None;
                game.dest = None;
                game.route.clear();
                game.job_cd = 3.0;
            } else if left <= 0.0 {
                game.say("JOB FAILED: TOO SLOW", 3.0);
                game.mission = Mission::None;
                game.dest = None;
                game.route.clear();
                game.job_cd = 3.0;
            } else {
                game.mission = Mission::Deliver { target, reward, time: left };
            }
        }
    }
    // with a destination set, the arrow follows the GPS route instead of pointing through buildings
    if game.dest.is_some() && game.route.len() > 1 {
        let mut left = 28.0;
        let mut at = game.route[game.route.len() - 1];
        for w in game.route.windows(2) {
            let l = w[0].distance(w[1]);
            if l >= left {
                at = w[0].lerp(w[1], left / l.max(0.01));
                break;
            }
            left -= l;
        }
        guide = Some(at);
    }
    let pulse = 1.0 + (game.time * 5.0).sin() * 0.15;
    if let Mission::Deliver { target, .. } = game.mission {
        *marker.1 = Visibility::Inherited;
        marker.0.translation = Vec3::new(target.x, terrain(target) + 0.55, target.y);
        marker.0.scale = Vec3::splat(pulse);
    } else {
        *marker.1 = Visibility::Hidden;
    }
    match guide {
        Some(g) if g.distance(game.pos) > 9.0 => {
            let d = (g - game.pos).normalize();
            let p = game.pos + d * (5.0 + pulse);
            *arrow.1 = Visibility::Inherited;
            arrow.0.translation = Vec3::new(p.x, terrain(p) + game.y + 2.5, p.y);
            arrow.0.rotation = yaw(d.y.atan2(d.x));
        }
        _ => *arrow.1 = Visibility::Hidden,
    }
}

pub fn wanted_and_death(
    time: Res<Time>,
    mut game: ResMut<Game>,
    world: Res<City>,
    mut player: Single<&mut Visibility, With<Player>>,
    mut beacons: Query<&mut Transform, With<Beacon>>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    game.time += dt;
    game.msg_t -= dt;
    game.shake = (game.shake - dt).max(0.0);
    game.crime_t -= dt;
    if game.crime_t <= 0.0 {
        game.heat = (game.heat - dt * 2.5).max(0.0);
    }
    for mut t in &mut beacons {
        t.rotate_y(dt * 9.0);
    }
    if game.dead_t > 0.0 {
        game.dead_t -= dt;
        if game.dead_t <= 0.0 {
            // respawn on the nearest sidewalk corner
            game.pos = world.push_out(game.pos, 0.6).0;
            game.health = 100.0;
            game.heat = 0.0;
            game.cash -= game.cash / 10;
            **player = Visibility::Inherited;
            game.say("PATCHED UP. 10% CLINIC FEE.", 3.0);
        }
    } else if game.health <= 0.0 {
        game.dead_t = 3.5;
        game.in_car = None;
        game.mission = Mission::None;
        game.say("FLATLINED", 3.5);
        **player = Visibility::Hidden;
    }
}

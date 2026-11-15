//! Minimap, full-screen city map (M), destination marker and GPS routing.
//! Both maps are drawn on the CPU into small images, so they stay crisp and
//! pixel-styled at any zoom and need no pre-rendered map artwork.

use crate::actors::{Bus, Car};
use crate::art::*;
use crate::world::*;
use crate::{Game, Mission};
use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::FontSize;

const MINI: i32 = 132; // minimap pixels (shown at 1.5x)
const MINI_MPP: f32 = 2.4; // metres per minimap pixel

const C_BG: [u8; 3] = [9, 10, 22];
const C_BLD: [u8; 3] = [44, 40, 82];
const C_LOCAL: [u8; 3] = [78, 86, 128];
const C_MAIN: [u8; 3] = [230, 110, 190];
const C_ART: [u8; 3] = [90, 225, 255];
const C_WATER: [u8; 3] = [14, 70, 92];
const C_ROUTE: [u8; 3] = [255, 60, 200];
const C_ME: [u8; 3] = [255, 230, 90];
const C_COP: [u8; 3] = [255, 50, 50];

struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Self {
        Canvas { w, h, px: vec![255; (w * h * 4) as usize] }
    }
    fn clear(&mut self, c: [u8; 3]) {
        for p in self.px.chunks_exact_mut(4) {
            p[0] = c[0];
            p[1] = c[1];
            p[2] = c[2];
            p[3] = 255;
        }
    }
    fn set(&mut self, x: i32, y: i32, c: [u8; 3]) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let o = ((y * self.w + x) * 4) as usize;
            self.px[o..o + 3].copy_from_slice(&c);
        }
    }
    fn dot(&mut self, p: Vec2, t: i32, c: [u8; 3]) {
        let (x, y) = (p.x.round() as i32 - t / 2, p.y.round() as i32 - t / 2);
        for oy in 0..t {
            for ox in 0..t {
                self.set(x + ox, y + oy, c);
            }
        }
    }
    fn line(&mut self, a: Vec2, b: Vec2, t: i32, c: [u8; 3]) {
        // clip the parameter range to the canvas (plus the stroke width)
        let m = t as f32 + 1.0;
        let (lo, hi) = (Vec2::splat(-m), Vec2::new(self.w as f32 + m, self.h as f32 + m));
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
            if p.abs() < 1e-6 {
                if q < 0.0 {
                    return;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if t0 > t1 {
            return;
        }
        let (a, b) = (a + d * t0, a + d * t1);
        let d = b - a;
        let n = d.x.abs().max(d.y.abs()).ceil().max(1.0) as i32;
        for i in 0..=n {
            self.dot(a + d * (i as f32 / n as f32), t, c);
        }
    }
    fn poly(&mut self, pts: &[Vec2], c: [u8; 3]) {
        let (mut y0, mut y1) = (f32::MAX, f32::MIN);
        for p in pts {
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
        let mut xs: Vec<f32> = vec![];
        for y in (y0.floor() as i32).max(0)..=(y1.ceil() as i32).min(self.h - 1) {
            let fy = y as f32 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.y > fy) != (b.y > fy) {
                    xs.push(a.x + (b.x - a.x) * (fy - a.y) / (b.y - a.y));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks_exact(2) {
                for x in (pair[0].round() as i32).max(0)..(pair[1].round() as i32).min(self.w) {
                    self.set(x, y, c);
                }
            }
        }
    }
    fn arrow(&mut self, p: Vec2, heading: f32, size: f32, c: [u8; 3]) {
        let f = Vec2::from_angle(heading);
        let r = Vec2::new(-f.y, f.x);
        self.poly(&[p + f * size, p - f * size * 0.7 + r * size * 0.7, p - f * size * 0.7 - r * size * 0.7], c);
    }
    fn ring(&mut self, p: Vec2, r: f32, c: [u8; 3]) {
        let n = (r * 6.0).max(12.0) as i32;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            self.set((p.x + a.cos() * r).round() as i32, (p.y + a.sin() * r).round() as i32, c);
        }
    }
}

#[derive(Resource)]
pub struct MapUi {
    mini: Handle<Image>,
    big: Handle<Image>,
    big_size: IVec2,
    center: Vec2,
    mpp: f32,
    dirty: bool,
    base: Vec<u8>,
    was_open: bool,
    press: Option<Vec2>,
    last: Vec2,
    dragged: bool,
    timer: f32,
    route_t: f32,
    line_t: f32,
    line: Option<Entity>,
}

/// Every street address in the city (see `tools/build_addresses.py`).
static ADDRESSES: &[u8] = include_bytes!("../data/addresses.bin");

/// Address search on the city map: streets (sorted by name) with their house numbers.
#[derive(Resource)]
pub struct Search {
    streets: Vec<(String, Vec<(String, Vec2)>)>,
    active: bool,
    query: String,
    results: Vec<(String, Vec2)>,
    sel: usize,
    shown: String,
}

impl Search {
    pub fn load() -> Search {
        let d = ADDRESSES;
        let mut streets = vec![];
        if d.len() > 6 && &d[0..4] == b"VLA1" {
            let mut o = 4;
            let u16_ = |o: &mut usize| {
                *o += 2;
                u16::from_le_bytes([d[*o - 2], d[*o - 1]])
            };
            let text = |o: &mut usize| {
                let n = d[*o] as usize;
                *o += 1 + n;
                String::from_utf8_lossy(&d[*o - n..*o]).into_owned()
            };
            for _ in 0..u16_(&mut o) {
                let name = text(&mut o);
                let n = u16_(&mut o);
                let list = (0..n)
                    .map(|_| {
                        let num = text(&mut o);
                        let x = u16_(&mut o) as i16 as f32;
                        (num, Vec2::new(x, u16_(&mut o) as i16 as f32))
                    })
                    .collect();
                streets.push((name, list));
            }
        }
        Search { streets, active: false, query: String::new(), results: vec![], sel: 0, shown: String::new() }
    }

    /// Up to eight matches for what has been typed: "street" or "street number".
    fn find(&mut self) {
        self.results.clear();
        self.sel = 0;
        let mut words: Vec<&str> = self.query.split_whitespace().collect();
        // a last word starting with a digit is the house number
        let num = if words.len() > 1 && words[words.len() - 1].starts_with(|c: char| c.is_ascii_digit()) { words.pop() } else { None };
        if words.is_empty() {
            return;
        }
        let street = words.join(" ");
        // best first: name starts with the text, then every word found at the start of a word, then anywhere
        let mut hits: Vec<(u8, usize)> = vec![];
        for (i, (name, _)) in self.streets.iter().enumerate() {
            let rank = if name.starts_with(&street) {
                0
            } else if words.iter().all(|w| name.split(' ').any(|n| n.starts_with(w))) {
                1
            } else if words.iter().all(|w| name.contains(w)) {
                2
            } else {
                continue;
            };
            hits.push((rank, i));
        }
        hits.sort();
        for (_, i) in hits {
            let (name, list) = &self.streets[i];
            match num {
                Some(n) => {
                    // the exact number first, then those that begin with it
                    for exact in [true, false] {
                        for (hn, p) in list.iter().filter(|(hn, _)| if exact { hn == n } else { hn != n && hn.starts_with(n) }) {
                            if self.results.len() < 8 {
                                self.results.push((format!("{name} {hn}"), *p));
                            }
                        }
                    }
                }
                None => {
                    if let Some((_, p)) = list.get(list.len() / 2) {
                        self.results.push((format!("{name}   ({} addresses)", list.len()), *p));
                    }
                }
            }
            if self.results.len() >= 8 {
                break;
            }
        }
    }
}

/// Upper case without Lithuanian diacritics, the way street names are stored.
fn plain(c: char) -> Option<char> {
    let c = c.to_uppercase().next()?;
    Some(match c {
        'Ą' => 'A',
        'Č' => 'C',
        'Ę' | 'Ė' => 'E',
        'Į' => 'I',
        'Š' => 'S',
        'Ų' | 'Ū' => 'U',
        'Ž' => 'Z',
        c if c.is_ascii_alphanumeric() || c == ' ' || c == '.' || c == '-' => c,
        _ => return None,
    })
}

/// Typing an address on the city map: / or Enter starts, arrows pick, Enter goes, Esc cancels.
/// While typing, no other part of the game sees the keyboard.
pub fn map_search(
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    city: Res<City>,
    mut game: ResMut<Game>,
    mut search: ResMut<Search>,
    mut ui: ResMut<MapUi>,
    mut label: Single<&mut Text, With<SearchLabel>>,
) {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::Key;
    let search = &mut *search;
    if !game.map_view {
        search.active = false;
        typed.clear();
        return;
    }
    let mut changed = false;
    let mut go = false;
    for ev in typed.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if !search.active {
            if matches!(ev.key_code, KeyCode::Slash | KeyCode::Enter | KeyCode::NumpadEnter) {
                search.active = true;
                search.query.clear();
                search.results.clear();
                keys.reset_all();
            }
            continue;
        }
        match &ev.logical_key {
            Key::Escape => search.active = false,
            Key::Enter => go = true,
            Key::Backspace => {
                search.query.pop();
                changed = true;
            }
            Key::ArrowDown => search.sel = (search.sel + 1).min(search.results.len().saturating_sub(1)),
            Key::ArrowUp => search.sel = search.sel.saturating_sub(1),
            Key::Space => {
                search.query.push(' ');
                changed = true;
            }
            _ => {
                if let Some(t) = &ev.text {
                    for c in t.chars().filter_map(plain) {
                        if search.query.len() < 40 {
                            search.query.push(c);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    if search.active {
        keys.reset_all();
    }
    if changed {
        search.find();
    }
    if go {
        if let Some((name, p)) = search.results.get(search.sel).cloned() {
            // mark it, route to it, and show it
            game.dest = Some(city.nearest_drive(p).map_or(p, |x| if x.1.distance(p) < 120.0 { x.1 } else { p }));
            game.route.clear();
            ui.route_t = 0.0;
            ui.center = p;
            ui.mpp = ui.mpp.min(2.0);
            ui.dirty = true;
            let name = name.split("   (").next().unwrap_or("").to_string();
            game.say(&format!("DESTINATION: {name}"), 3.0);
            search.active = false;
        }
    }
    let txt = if search.active {
        let mut t = format!("SEARCH: {}_", search.query);
        for (i, (name, _)) in search.results.iter().enumerate() {
            t += &format!("\n{} {}", if i == search.sel { ">" } else { " " }, name);
        }
        if search.results.is_empty() && search.query.len() > 1 {
            t += "\n  nothing found";
        }
        t
    } else {
        "/ SEARCH ADDRESS".to_string()
    };
    if search.shown != txt {
        search.shown = txt.clone();
        label.0 = txt;
    }
}

#[derive(Component)]
pub struct SearchLabel;
#[derive(Component)]
pub struct BigMapRoot;
#[derive(Component)]
pub struct BigMapLabel;
#[derive(Component)]
pub struct MiniRoot;

fn canvas_image(w: i32, h: i32) -> Image {
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (w * h * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::nearest();
    img
}

pub fn setup_maps(mut commands: Commands, mut images: ResMut<Assets<Image>>, game: Res<Game>) {
    let mini = images.add(canvas_image(MINI, MINI));
    let big_size = IVec2::new(640, 360);
    let big = images.add(canvas_image(big_size.x, big_size.y));
    let font = |s: f32| TextFont { font_size: FontSize::Px(s), ..default() };

    // minimap, bottom-left like a dashboard instrument
    commands
        .spawn((
            MiniRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                bottom: px(62),
                width: px(206),
                height: px(206),
                padding: UiRect::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.35, 0.9, 1.0, 0.55)),
        ))
        .with_children(|p| {
            p.spawn((
                Node { width: percent(100), height: percent(100), ..default() },
                ImageNode { image: mini.clone(), image_mode: NodeImageMode::Stretch, ..default() },
            ));
        });

    // full-screen city map, hidden until M is pressed
    commands
        .spawn((
            BigMapRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                display: Display::None,
                ..default()
            },
            ImageNode { image: big.clone(), image_mode: NodeImageMode::Stretch, ..default() },
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("CLICK set destination   RIGHT-CLICK clear   SCROLL zoom   DRAG pan   / search   M close"),
                font(15.0),
                TextColor(Color::srgb(0.6, 0.95, 1.0)),
                Node { position_type: PositionType::Absolute, top: px(14), left: px(18), ..default() },
            ));
            p.spawn((
                SearchLabel,
                Text::new("/ SEARCH ADDRESS"),
                font(20.0),
                TextColor(Color::srgb(1.0, 0.95, 0.6)),
                Node { position_type: PositionType::Absolute, top: px(42), left: px(18), padding: UiRect::all(px(6)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.02, 0.08, 0.8)),
            ));
            p.spawn((
                BigMapLabel,
                Text::new(""),
                font(22.0),
                TextColor(Color::srgb(1.0, 0.85, 0.3)),
                Node { position_type: PositionType::Absolute, bottom: px(18), left: px(18), ..default() },
            ));
        });

    commands.insert_resource(MapUi {
        mini,
        big,
        big_size,
        center: game.pos,
        mpp: 8.0,
        dirty: true,
        base: vec![],
        was_open: false,
        press: None,
        last: Vec2::ZERO,
        dragged: false,
        timer: 0.0,
        route_t: 0.0,
        line_t: 0.0,
        line: None,
    });
}

/// Open/close, pan, zoom, and click-to-set-destination on the big map.
pub fn map_input(
    window: Single<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    city: Res<City>,
    mut game: ResMut<Game>,
    mut ui: ResMut<MapUi>,
    mut root: Single<&mut Node, (With<BigMapRoot>, Without<MiniRoot>)>,
    mut mini: Single<&mut Node, (With<MiniRoot>, Without<BigMapRoot>)>,
    mut label: Single<&mut Text, With<BigMapLabel>>,
) {
    let open = game.map_view;
    if open != ui.was_open {
        ui.was_open = open;
        root.display = if open { Display::Flex } else { Display::None };
        mini.display = if open { Display::None } else { Display::Flex };
        if open {
            ui.center = game.pos;
            ui.dirty = true;
            ui.press = None;
        }
    }
    if !open {
        return;
    }
    // keep the canvas at half the window resolution for a chunky look
    let want = IVec2::new((window.width() * 0.5) as i32, (window.height() * 0.5) as i32).clamp(IVec2::splat(160), IVec2::splat(1400));
    if want != ui.big_size {
        ui.big_size = want;
        ui.dirty = true;
    }
    let scale = ui.big_size.x as f32 / window.width().max(1.0);
    let Some(cur) = window.cursor_position() else { return };
    let cpx = cur * scale;
    let half = ui.big_size.as_vec2() * 0.5;
    let world = ui.center + (cpx - half) * ui.mpp;

    let dy = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if dy != 0.0 {
        let mpp = (ui.mpp * (-dy * 0.18).exp()).clamp(0.6, 48.0);
        // zoom about the cursor
        ui.center = world - (cpx - half) * mpp;
        ui.mpp = mpp;
        ui.dirty = true;
    }
    if mouse.just_pressed(MouseButton::Left) {
        ui.press = Some(cur);
        ui.last = cur;
        ui.dragged = false;
    }
    if mouse.pressed(MouseButton::Left) {
        if let Some(p0) = ui.press {
            if cur.distance(p0) > 5.0 {
                ui.dragged = true;
            }
            if ui.dragged {
                let d = (cur - ui.last) * scale * ui.mpp;
                ui.center -= d;
                ui.dirty = true;
            }
            ui.last = cur;
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if ui.press.take().is_some() && !ui.dragged {
            let target = city.nearest_drive(world).map_or(world, |x| x.1);
            game.dest = Some(target);
            game.route.clear();
            ui.route_t = 0.0;
            let name = city.street(target).to_string();
            game.say(&format!("DESTINATION: {}", if name.is_empty() { "MARKED" } else { &name }), 2.5);
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        game.dest = None;
        game.route.clear();
    }
    let name = city.street(world);
    let dist = world.distance(game.pos);
    let txt = format!("{}   {:.1} km away", if name.is_empty() { "-" } else { name }, dist / 1000.0);
    if label.0 != txt {
        label.0 = txt;
    }
}

fn off_route(route: &[Vec2], p: Vec2) -> f32 {
    let mut best = f32::MAX;
    for w in route.windows(2).take(40) {
        let ab = w[1] - w[0];
        let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        best = best.min((w[0] + ab * t).distance(p));
    }
    best
}

/// Keeps the GPS route fresh: plans it, trims what has been driven, re-plans after a wrong turn.
pub fn route_update(time: Res<Time>, city: Res<City>, mut game: ResMut<Game>, mut ui: ResMut<MapUi>) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let Some(dest) = game.dest else {
        if !game.route.is_empty() {
            game.route.clear();
        }
        return;
    };
    if game.pos.distance(dest) < 14.0 && matches!(game.mission, Mission::None) {
        game.dest = None;
        game.route.clear();
        game.say("YOU HAVE ARRIVED", 2.5);
        return;
    }
    ui.route_t -= dt;
    if ui.route_t > 0.0 {
        return;
    }
    ui.route_t = 0.4;
    if game.route.len() < 2 || off_route(&game.route, game.pos) > 38.0 {
        game.route = city.route(game.pos, dest);
        ui.route_t = 1.2;
        return;
    }
    // drop the part already behind us
    let mut best = (f32::MAX, 0usize);
    for (i, w) in game.route.windows(2).take(40).enumerate() {
        let ab = w[1] - w[0];
        let t = ((game.pos - w[0]).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let d = (w[0] + ab * t).distance(game.pos);
        if d < best.0 {
            best = (d, i);
        }
    }
    if best.1 > 0 {
        game.route.drain(0..best.1);
    }
    game.route[0] = game.pos;
}

pub fn draw_maps(
    time: Res<Time>,
    city: Res<City>,
    game: Res<Game>,
    mut ui: ResMut<MapUi>,
    mut images: ResMut<Assets<Image>>,
    cars: Query<(&Transform, &Car)>,
    buses: Query<&Transform, With<Bus>>,
) {
    ui.timer -= time.delta_secs();
    if ui.timer > 0.0 && !(game.map_view && ui.dirty) {
        return;
    }
    ui.timer = 0.1;
    let cops: Vec<Vec2> = if game.wanted() > 0 {
        cars.iter().filter(|(_, c)| c.police).map(|(t, _)| Vec2::new(t.translation.x, t.translation.z)).collect()
    } else {
        vec![]
    };

    if !game.map_view {
        // ---- minimap: everything loaded around the player ----
        let mut cv = Canvas::new(MINI, MINI);
        cv.clear(C_BG);
        let half = Vec2::splat(MINI as f32 * 0.5);
        let to = |p: Vec2| (p - game.pos) / MINI_MPP + half;
        let c0 = cell_of(game.pos);
        let r = 3;
        for dz in -r..=r {
            for dx in -r..=r {
                let c = c0 + IVec2::new(dx, dz);
                for &bi in city.own_blds(c) {
                    let pts: Vec<Vec2> = city.blds[bi as usize].pts.iter().map(|p| to(*p)).collect();
                    cv.poly(&pts, C_BLD);
                }
            }
        }
        for pass in 0..2 {
            for dz in -r..=r {
                for dx in -r..=r {
                    let Some(src) = city.src(c0 + IVec2::new(dx, dz)) else { continue };
                    for (a, b, hw) in &src.segs {
                        let big = *hw > 3.2;
                        if big == (pass == 1) {
                            let t = ((hw * 2.0 / MINI_MPP).round() as i32).clamp(1, 6);
                            cv.line(to(*a), to(*b), t, if big { [96, 104, 150] } else { C_LOCAL });
                        }
                    }
                }
            }
        }
        // arterials from the global graph keep bridges and tunnels visible too
        for dz in -r..=r {
            for dx in -r..=r {
                for &e in city.drive_in(c0 + IVec2::new(dx, dz)) {
                    let ed = &city.drive.edges[e as usize];
                    if ed.class <= 1 {
                        for w in ed.pts.windows(2) {
                            cv.line(to(w[0]), to(w[1]), 1, if ed.class == 0 { C_ART } else { C_MAIN });
                        }
                    }
                }
            }
        }
        for w in game.route.windows(2) {
            cv.line(to(w[0]), to(w[1]), 2, C_ROUTE);
        }
        if let Some(d) = game.dest {
            // clamp the pin to the rim so it always shows the direction
            let p = to(d);
            let v = p - half;
            let lim = MINI as f32 * 0.5 - 5.0;
            let p = if v.abs().max_element() > lim { half + v * (lim / v.abs().max_element()) } else { p };
            cv.dot(p, 5, C_ROUTE);
            cv.ring(p, 5.0, [255, 255, 255]);
        }
        for dz in -r..=r {
            for dx in -r..=r {
                if let Some(src) = city.src(c0 + IVec2::new(dx, dz)) {
                    for (p, _) in &src.cams {
                        cv.dot(to(*p), 4, [255, 170, 30]);
                        cv.ring(to(*p), 4.0, [255, 255, 255]);
                    }
                }
            }
        }
        for t in &buses {
            cv.dot(to(Vec2::new(t.translation.x, t.translation.z)), 4, [80, 255, 120]);
        }
        for c in &cops {
            cv.dot(to(*c), 3, C_COP);
        }
        cv.arrow(half, game.heading, 5.0, C_ME);
        if let Some(mut img) = images.get_mut(&ui.mini) {
            img.data = Some(cv.px);
        }
        return;
    }

    // ---- big map: the whole road network ----
    let size = ui.big_size;
    let half = size.as_vec2() * 0.5;
    let (center, mpp) = (ui.center, ui.mpp);
    let to = |p: Vec2| (p - center) / mpp + half;
    if ui.dirty || ui.base.len() != (size.x * size.y * 4) as usize {
        ui.dirty = false;
        let mut cv = Canvas::new(size.x, size.y);
        cv.clear(C_BG);
        // terrain relief as a shaded backdrop
        for y in (0..size.y).step_by(2) {
            for x in (0..size.x).step_by(2) {
                let wp = center + (Vec2::new(x as f32, y as f32) - half) * mpp;
                let k = (relief_shade(wp) - 0.55) * 1.6;
                let c = [(8.0 + 26.0 * k) as u8, (10.0 + 30.0 * k) as u8, (22.0 + 44.0 * k) as u8];
                for (ox, oy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    cv.set(x + ox, y + oy, c);
                }
            }
        }
        let view = half * mpp + 80.0;
        let visible = |pts: &[Vec2]| {
            let (mut lo, mut hi) = (Vec2::MAX, Vec2::MIN);
            for p in pts {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            hi.x > center.x - view.x && lo.x < center.x + view.x && hi.y > center.y - view.y && lo.y < center.y + view.y
        };
        for (w, pts) in &city.rivers {
            if visible(pts) {
                let t = ((*w / mpp).round() as i32).clamp(1, 60);
                for s in pts.windows(2) {
                    cv.line(to(s[0]), to(s[1]), t, C_WATER);
                }
            }
        }
        // airport: runway and taxiways
        for (w, pts) in &city.airstrips {
            if visible(pts) {
                let t = ((*w / mpp).round() as i32).clamp(1, 60);
                for s in pts.windows(2) {
                    cv.line(to(s[0]), to(s[1]), t, C_LOCAL);
                }
            }
        }
        // buildings appear once zoomed in far enough (only the streamed neighbourhood is known)
        if mpp < 3.0 {
            let (lo, hi) = (cell_of(center - view), cell_of(center + view));
            for cz in lo.y..=hi.y {
                for cx in lo.x..=hi.x {
                    for &bi in city.own_blds(IVec2::new(cx, cz)) {
                        let pts: Vec<Vec2> = city.blds[bi as usize].pts.iter().map(|p| to(*p)).collect();
                        cv.poly(&pts, C_BLD);
                    }
                }
            }
        }
        for class in [2u8, 1, 0] {
            if class == 2 && mpp > 22.0 {
                continue;
            }
            let (col, base_w) = match class {
                0 => (C_ART, 16.0),
                1 => (C_MAIN, 10.0),
                _ => (C_LOCAL, 6.0),
            };
            let t = ((base_w / mpp).round() as i32).clamp(1, 14);
            for ed in &city.drive.edges {
                if ed.class == class && visible(&ed.pts) {
                    for s in ed.pts.windows(2) {
                        cv.line(to(s[0]), to(s[1]), t, col);
                    }
                }
            }
        }
        ui.base = cv.px;
    }
    let mut cv = Canvas { w: size.x, h: size.y, px: ui.base.clone() };
    for w in game.route.windows(2) {
        cv.line(to(w[0]), to(w[1]), 3, C_ROUTE);
    }
    if let Some(d) = game.dest {
        let p = to(d);
        let pulse = 5.0 + (game.time * 5.0).sin() * 2.0;
        cv.dot(p, 5, C_ROUTE);
        cv.ring(p, pulse + 3.0, [255, 255, 255]);
        cv.ring(p, pulse + 4.0, C_ROUTE);
    }
    for c in &cops {
        cv.dot(to(*c), 3, C_COP);
    }
    cv.arrow(to(game.pos), game.heading, 6.0, C_ME);
    cv.ring(to(game.pos), 9.0, C_ME);
    if let Some(mut img) = images.get_mut(&ui.big) {
        if img.texture_descriptor.size.width != size.x as u32 || img.texture_descriptor.size.height != size.y as u32 {
            img.resize(Extent3d { width: size.x as u32, height: size.y as u32, depth_or_array_layers: 1 });
        }
        img.data = Some(cv.px);
    }
}

/// A glowing line painted on the road for the next stretch of the route.
pub fn gps_line(
    mut commands: Commands,
    time: Res<Time>,
    city: Res<City>,
    game: Res<Game>,
    art: Res<Art>,
    mut ui: ResMut<MapUi>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    ui.line_t -= time.delta_secs();
    if ui.line_t > 0.0 {
        return;
    }
    ui.line_t = 0.3;
    if let Some(e) = ui.line.take() {
        commands.entity(e).despawn();
    }
    if game.route.len() < 2 {
        return;
    }
    let mut mb = MB::default();
    let col = [0.55, 0.06, 0.4, 1.0];
    let mut y = game.y;
    let mut travelled = 0.0;
    for w in game.route.windows(2) {
        let len = w[0].distance(w[1]);
        if len < 0.05 {
            continue;
        }
        let d = (w[1] - w[0]) / len;
        let n = Vec2::new(-d.y, d.x) * 0.45;
        // sample the surface every few metres so the line climbs ramps and dips into tunnels
        let steps = (len / 6.0).ceil().max(1.0) as i32;
        for k in 0..steps {
            let (a, b) = (w[0] + d * (len * k as f32 / steps as f32), w[0] + d * (len * (k + 1) as f32 / steps as f32));
            let ya = y;
            y = city.ground_y(b, y);
            let seg = len / steps as f32;
            travelled += seg;
            if travelled > 4.0 && travelled < 320.0 {
                let v = |p: Vec2, h: f32| Vec3::new(p.x, terrain(p) + h + 0.6, p.y);
                mb.quad([v(a + n, ya), v(a - n, ya), v(b - n, y), v(b + n, y)], T_SOLID, 0, col);
            }
        }
        if travelled > 320.0 {
            break;
        }
    }
    if mb.is_empty() {
        return;
    }
    ui.line = Some(
        commands
            .spawn((Mesh3d(meshes.add(mb.build())), MeshMaterial3d(art.glow.clone()), Transform::default(), NotShadowCaster))
            .id(),
    );
}

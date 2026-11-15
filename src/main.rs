mod actors;
mod art;
mod landmarks;
mod maps;
mod transit;
mod world;

use actors::*;
use art::*;
use bevy::camera::RenderTarget;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::text::FontSize;
use world::*;

/// Vertical resolution of the pixel-art render target.
/// Tallest frame the game will render, in real pixels.
const PIX_H: f32 = 1440.0;
/// Camera lean from straight-down, in radians: enough to see facades and depth.
const TILT: f32 = 0.3;
pub const LIGHT_POWER: f32 = 1_500_000.0;
const FOV: f32 = 1.0;

#[derive(Clone, Copy)]
pub enum Mission {
    None,
    Deliver { target: Vec2, reward: i64, time: f32 },
}

#[derive(Resource)]
pub struct Game {
    pub pos: Vec2,
    pub vel: Vec2,
    pub heading: f32,
    pub aim: Vec2,
    pub cam_focus: Vec2,
    pub view_half: Vec2,
    pub in_car: Option<Entity>,
    pub health: f32,
    pub cash: i64,
    pub heat: f32,
    pub crime_t: f32,
    pub ammo: u32,
    pub smg: bool,
    pub fire_cd: f32,
    pub msg: String,
    pub msg_t: f32,
    pub dead_t: f32,
    pub mission: Mission,
    pub job_cd: f32,
    pub jobs_done: u32,
    pub rng: Rng,
    pub fear: Vec<(Vec2, f32)>,
    pub booms: Vec<Vec2>,
    pub shake: f32,
    pub time: f32,
    pub started: bool,
    pub map_view: bool,
    /// Height of the player (bridges > 0, tunnels < 0).
    pub y: f32,
    /// Destination marked on the map, and the driving route to it.
    pub dest: Option<Vec2>,
    pub route: Vec<Vec2>,
    /// Seconds to keep the height fixed after fast travel while the area streams in.
    pub travel_hold: f32,
    /// Camera height above the ground beneath it.
    pub cam_h: f32,
    /// Camera zoom (1 = default, bigger = further out) and whether the mouse is over a HUD button.
    pub zoom: f32,
    pub ui_hover: bool,
    /// Render scale: 1 = native resolution, lower = faster and chunkier.
    pub quality: f32,
    /// When each speed camera last flashed, keyed by position.
    pub cam_flash: bevy::platform::collections::HashMap<(i32, i32), f32>,
    /// Weather: rain 0..1, fog 0..1, storm 0..1, wind in m/s, and a label for the HUD.
    pub rain: f32,
    pub fog: f32,
    pub storm: f32,
    pub wind: Vec2,
    pub weather: &'static str,
    /// Time of day in hours (0..24) and how dark it is (0 = full day, 1 = night).
    pub clock: f32,
    pub night: f32,
}

impl Game {
    pub fn wanted(&self) -> u32 {
        if self.heat < 4.0 { 0 } else { ((self.heat / 20.0).ceil() as u32).clamp(1, 5) }
    }
    pub fn crime(&mut self, amount: f32) {
        self.heat = (self.heat + amount).min(100.0);
        self.crime_t = 9.0;
    }
    pub fn say(&mut self, s: &str, t: f32) {
        self.msg = s.to_string();
        self.msg_t = t;
    }
}

#[derive(Resource)]
struct Screen {
    image: Handle<Image>,
    size: UVec2,
}

#[derive(Component)]
struct MainCam;
#[derive(Component)]
struct CarLamp;
#[derive(Component)]
struct Rain(f32);
#[derive(Component)]
struct CarBeam;
#[derive(Component)]
struct ZoomBtn(f32);
#[derive(Component)]
struct CopLight;
#[derive(Component)]
struct Mist(Vec2, bool, f32);
#[derive(Component)]
struct Ripple(f32, f32);
#[derive(Component)]
enum Hud {
    Cash,
    Heat,
    Health,
    Place,
    Job,
    Msg,
    Speed,
    Clock,
}

fn main() {
    let city = City::load();
    let spawn = city.spawn;
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Neon Vilnius".into(),
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin { meta_check: bevy::asset::AssetMetaCheck::Never, ..default() }),
        )
        .init_asset::<TileBlob>()
        .init_asset_loader::<TileLoader>()
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(transit::Transit::load())
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.45, 0.5, 1.0), brightness: 90.0, ..default() })
        .insert_resource(city)
        .insert_resource(Game {
            pos: spawn,
            vel: Vec2::ZERO,
            heading: 0.0,
            aim: Vec2::ZERO,
            cam_focus: spawn,
            view_half: Vec2::new(60.0, 35.0),
            in_car: None,
            health: 100.0,
            cash: 0,
            heat: 0.0,
            crime_t: 0.0,
            ammo: 0,
            smg: false,
            fire_cd: 0.0,
            msg: "NEON VILNIUS".into(),
            msg_t: 4.0,
            dead_t: 0.0,
            mission: Mission::None,
            job_cd: 0.0,
            jobs_done: 0,
            rng: Rng::new(0xC0FFEE),
            fear: vec![],
            booms: vec![],
            shake: 0.0,
            time: 0.0,
            started: false,
            map_view: false,
            y: 0.0,
            dest: None,
            route: vec![],
            travel_hold: 0.0,
            cam_h: 50.0,
            zoom: 1.0,
            ui_hover: false,
            quality: 1.0,
            cam_flash: Default::default(),
            rain: 0.0,
            fog: 0.0,
            storm: 0.0,
            wind: Vec2::new(5.0, 2.0),
            weather: "SUNNY",
            clock: 9.0,
            night: 0.0,
        })
        .add_systems(Startup, (setup_art, setup_lights, setup, maps::setup_maps).chain())
        .add_plugins(bevy::pbr::MaterialPlugin::<WaterMaterial>::default())
        .add_systems(Update, (landmark_tour, water_update))
        .insert_resource(maps::Search::load())
        .add_systems(PreUpdate, maps::map_search.after(bevy::input::InputSystems))
        .add_systems(
            Update,
            (
                resize_target,
                aim,
                player_control,
                car_ai,
                car_physics,
                car_collisions,
                peds,
                bullets,
                explosions,
                particles,
                population,
                pickups_and_jobs,
                wanted_and_death,
                camera_follow,
                stream_world,
                assign_lights,
                underground_view,
                deck_fade,
                (maps::map_input, maps::route_update, maps::draw_maps, maps::gps_line).chain(),
                (rain, weather, car_lights, spray, transit::bus_spawner, zoom_buttons, speed_cameras, hud).chain(),
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>, art: Res<Art>, game: Res<Game>) {
    let size = UVec2::new(711, PIX_H as u32);
    let mut img = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    img.sampler = bevy::image::ImageSampler::nearest();
    let image = images.add(img);

    commands.spawn((
        MainCam,
        Camera3d::default(),
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::srgb(0.004, 0.004, 0.012)), ..default() },
        RenderTarget::Image(image.clone().into()),
        Projection::Perspective(PerspectiveProjection { fov: FOV, near: 1.0, far: 500.0, ..default() }),
        Tonemapping::AcesFitted,
        ColorGrading { global: ColorGradingGlobal { post_saturation: 1.32, ..default() }, ..default() },
        // smooth geometry edges (the sharpening pass is not available on WebGL2)
        bevy::anti_alias::fxaa::Fxaa::default(),
        // atmospheric depth: distant blocks sink into the night haze
        bevy::pbr::DistanceFog {
            color: Color::srgb(0.02, 0.03, 0.07),
            falloff: bevy::pbr::FogFalloff::Linear { start: 70.0, end: 300.0 },
            ..default()
        },
        bevy::camera::Exposure { ev100: 7.6 },
        Bloom { intensity: 0.26, ..Bloom::NATURAL },
        Msaa::Off,
        Transform::from_xyz(game.pos.x, 50.0, game.pos.y).looking_at(Vec3::new(game.pos.x, 0.0, game.pos.y), Vec3::NEG_Z),
    ));
    commands.spawn((Camera2d, Camera { order: 1, ..default() }, Msaa::Off, bevy::ui::IsDefaultUiCamera));

    // player
    commands.spawn((
        Player,
        Mesh3d(art.player[4].clone()),
        MeshMaterial3d(art.mat.clone()),
        Transform::from_xyz(game.pos.x, 0.0, game.pos.y),
        Visibility::Inherited,
    ));
    // moonlight through the overcast: gives every building and tree a long cool shadow
    commands.spawn((
        DirectionalLight { illuminance: 680.0, color: Color::srgb(0.6, 0.72, 1.0), shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(0.5, -1.0, 0.38).normalize(), Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 1, minimum_distance: 1.0, maximum_distance: 80.0, ..default() }.build(),
    ));
    // drifting ground mist that catches the neon
    for i in 0..40usize {
        let dense = i >= 16;
        let k = if dense { 2.6 } else { 1.0 };
        let c = [[0.018, 0.008, 0.026], [0.005, 0.018, 0.022], [0.014, 0.014, 0.026]][i % 3];
        let mut m = MB::default();
        m.ground(-34.0, -34.0, 34.0, 34.0, 0.0, T_GLOW, 0, [c[0] * k, c[1] * k, c[2] * k, 1.0]);
        commands.spawn((
            Mist(Vec2::from_angle(i as f32 * 2.4) * (0.6 + (i % 4) as f32 * 0.35), dense, if dense { 3.0 + (i % 6) as f32 * 1.6 } else { 7.0 + (i % 5) as f32 * 3.0 }),
            Mesh3d(meshes.add(m.build())),
            MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
            Transform::from_xyz(
                game.pos.x + (i as f32 * 37.0) % 200.0 - 100.0,
                if dense { 3.0 + (i % 6) as f32 * 1.6 } else { 7.0 + (i % 5) as f32 * 3.0 },
                game.pos.y + (i as f32 * 53.0) % 150.0 - 75.0,
            ),
            if dense { Visibility::Hidden } else { Visibility::Inherited },
        ));
    }
    // real light from the headlights of nearby traffic, and police flashers
    for _ in 0..8 {
        commands.spawn((
            CarBeam,
            PointLight { intensity: 0.0, range: 24.0, color: Color::srgb(1.0, 0.93, 0.78), shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(0.0, -100.0, 0.0),
        ));
    }
    for _ in 0..2 {
        commands.spawn((
            CopLight,
            PointLight { intensity: 0.0, range: 30.0, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(0.0, -100.0, 0.0),
        ));
    }
    // a ride waiting next to the spawn point
    spawn_car(&mut commands, &art, game.pos + Vec2::new(3.5, 2.5), 0.0, 5, None);
    // headlamp that follows the player's car, plus a faint personal light
    commands.spawn((
        CarLamp,
        PointLight { intensity: 0.0, range: 30.0, color: Color::srgb(1.0, 0.92, 0.75), shadow_maps_enabled: false, ..default() },
        Transform::default(),
    ));
    commands.spawn((
        TargetMarker,
        Mesh3d(art.marker.clone()),
        MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        GuideArrow,
        Mesh3d(art.arrow.clone()),
        MeshMaterial3d(art.glow.clone()),
            NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
    for i in 0..520 {
        commands.spawn((
            Rain(i as f32 / 520.0),
            NotShadowCaster,
            Mesh3d(art.rain.clone()),
            MeshMaterial3d(art.glow.clone()),
            Transform::from_xyz(0.0, -50.0, 0.0),
        ));
    }
    for i in 0..170 {
        commands.spawn((
            Ripple(i as f32 / 170.0, i as f32 / 170.0),
            NotShadowCaster,
            Mesh3d(art.ripple.clone()),
            MeshMaterial3d(art.glow.clone()),
            Transform::from_xyz(0.0, -50.0, 0.0),
        ));
    }

    // screen quad + HUD
    let font = |s: f32| TextFont { font_size: FontSize::Px(s), ..default() };
    let abs = |n: Node| Node { position_type: PositionType::Absolute, ..n };
    commands
        .spawn((
            Node { width: percent(100), height: percent(100), ..default() },
            ImageNode { image: image.clone(), image_mode: NodeImageMode::Stretch, ..default() },
        ))
        .with_children(|p| {
            p.spawn((Hud::Cash, Text::new("$0"), font(30.0), TextColor(Color::srgb(0.4, 1.0, 0.5)), abs(Node { top: px(14), left: px(18), ..default() })));
            p.spawn((
                abs(Node { top: px(54), left: px(18), width: px(204), height: px(12), ..default() }),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|b| {
                b.spawn((Hud::Health, Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::srgb(1.0, 0.2, 0.45))));
            });
            p.spawn((Hud::Heat, Text::new(""), font(24.0), TextColor(Color::srgb(1.0, 0.25, 0.3)), abs(Node { top: px(14), right: px(18), ..default() })));
            p.spawn((Hud::Place, Text::new(""), font(16.0), TextColor(Color::srgb(0.5, 0.9, 1.0)), abs(Node { bottom: px(34), left: px(18), ..default() })));
            // clock and time of day, top centre
            p.spawn((abs(Node { top: px(12), width: percent(100), justify_content: JustifyContent::Center, ..default() }),)).with_children(|c| {
                c.spawn((Hud::Clock, Text::new(""), font(26.0), TextColor(Color::srgb(1.0, 0.9, 0.55))));
            });
            p.spawn((
                Hud::Speed,
                Text::new(""),
                font(26.0),
                TextColor(Color::srgb(0.6, 0.95, 1.0)),
                abs(Node { bottom: px(40), right: px(18), ..default() }),
            ));
            p.spawn((Hud::Job, Text::new(""), font(18.0), TextColor(Color::srgb(1.0, 0.85, 0.3)), abs(Node { top: px(78), left: px(18), ..default() })));
            p.spawn((
                abs(Node { top: percent(30), width: percent(100), justify_content: JustifyContent::Center, ..default() }),
            ))
            .with_children(|c| {
                c.spawn((Hud::Msg, Text::new(""), font(44.0), TextColor(Color::srgb(1.0, 0.3, 0.75))));
            });
            p.spawn((
                Text::new("WASD  MOUSE shoot  F car  SPACE brake  SHIFT run  Q gun  M map  T +1h  +/- zoom  G quality  0-9 travel  L sights"),
                font(11.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.45)),
                abs(Node { bottom: px(8), right: px(18), ..default() }),
            ));
        });
    // zoom buttons on the right edge
    for (i, (label, factor)) in [("+", 0.8f32), ("-", 1.25)].into_iter().enumerate() {
        commands
            .spawn((
                ZoomBtn(factor),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(18),
                    top: px(120 + i as i32 * 58),
                    width: px(48),
                    height: px(48),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.1, 0.18, 0.8)),
            ))
            .with_children(|b| {
                b.spawn((Text::new(label), TextFont { font_size: FontSize::Px(30.0), ..default() }, TextColor(Color::srgb(0.5, 0.95, 1.0))));
            });
    }
    commands.insert_resource(Screen { image, size });
}

fn resize_target(window: Single<&Window>, game: Res<Game>, mut screen: ResMut<Screen>, mut images: ResMut<Assets<Image>>) {
    // render at the display's real pixel resolution (capped), not a low-res frame stretched up
    let (w, h) = (window.physical_width().max(1) as f32, window.physical_height().max(1) as f32);
    let ph = (h * game.quality).min(PIX_H).max(180.0);
    let size = UVec2::new(((ph * w / h).round() as u32).clamp(64, 2880), ph as u32);
    if size != screen.size {
        screen.size = size;
        if let Some(mut img) = images.get_mut(&screen.image) {
            img.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 });
        }
    }
}

fn aim(
    window: Single<&Window>,
    cam: Single<(&Camera, &GlobalTransform), With<MainCam>>,
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut game: ResMut<Game>,
) {
    // zoom: mouse wheel, or the + and - keys
    if !game.map_view {
        let wheel = match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        let mut z = game.zoom * (-wheel * 0.12).exp();
        if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
            z *= 0.8;
        }
        if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
            z *= 1.25;
        }
        game.zoom = z.clamp(0.45, 2.2);
    }
    if mouse.just_pressed(MouseButton::Left) {
        game.started = true;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        game.map_view = !game.map_view;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        // graphics quality: native -> 75% -> 50% -> native
        game.quality = if game.quality > 0.9 { 0.75 } else if game.quality > 0.6 { 0.5 } else { 1.0 };
        let q = (game.quality * 100.0) as u32;
        game.say(&format!("RENDER SCALE {q}%"), 1.5);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        game.clock = (game.clock + 1.0) % 24.0; // skip an hour
    }
    // fast travel between districts (on foot only)
    const PLACES: [(KeyCode, &str, f32, f32, f32); 10] = [
        (KeyCode::Digit0, "PO SESKINES VIADUKU", 75.0, -192.0, 0.0),
        (KeyCode::Digit1, "SESKINE", 57.0, -2.0, 0.0),
        (KeyCode::Digit2, "SENAMIESTIS", 2746.0, 3229.0, 0.0),
        (KeyCode::Digit3, "ZVERYNAS", 500.0, 2600.0, 0.0),
        (KeyCode::Digit4, "KAROLINISKES", -1955.0, 3085.0, 0.0),
        (KeyCode::Digit5, "ORO UOSTAS", 2395.0, 7750.0, 0.0),
        (KeyCode::Digit6, "NAUJOJI VILNIA", 10609.0, 2433.0, 0.0),
        (KeyCode::Digit7, "SESKINES VIADUKAS", 75.0, -192.0, 6.0),
        (KeyCode::Digit8, "GELEZINIO VILKO TUNELIS", 1133.0, 2725.0, -6.0),
        (KeyCode::Digit9, "OZO ZIEDAS", 394.0, 689.0, 0.0),
    ];
    for (k, name, x, z, y) in PLACES {
        if keys.just_pressed(k) && game.in_car.is_none() && game.dead_t <= 0.0 {
            game.pos = Vec2::new(x, z);
            game.y = y;
            game.travel_hold = 3.0;
            game.say(name, 2.5);
        }
    }
    let Some(c) = window.cursor_position() else { return };
    let vp = c / Vec2::new(window.width(), window.height()) * screen.size.as_vec2();
    let (camera, ct) = *cam;
    if let Ok(ray) = camera.viewport_to_world(ct, vp) {
        if ray.direction.y < -0.01 {
            let t = (terrain(game.pos) + game.y + 1.2 - ray.origin.y) / ray.direction.y;
            let p = ray.origin + *ray.direction * t;
            game.aim = Vec2::new(p.x, p.z);
        }
    }
}

/// Feeds the water shader: camera, time, where the light is and what colour the sky is.
fn water_update(
    game: Res<Game>,
    art: Res<Art>,
    mut waters: ResMut<Assets<WaterMaterial>>,
    cam: Single<&Transform, With<MainCam>>,
    light: Single<(&DirectionalLight, &Transform), Without<MainCam>>,
) {
    let Some(mut m) = waters.get_mut(&art.water) else { return };
    let day = 1.0 - game.night;
    let dull = 1.0 - 0.45 * game.rain.max(game.fog);
    let to_light = -light.1.forward().as_vec3();
    let lc = light.0.color.to_linear();
    // glitter fades under cloud; the moon gives a faint silver one
    let k = (light.0.illuminance / 3000.0).min(1.3) * (1.0 - 0.8 * game.rain) * (0.25 + 0.75 * day) + 0.12 * (1.0 - day);
    let sky = Vec3::new(0.05, 0.07, 0.13).lerp(Vec3::new(0.42, 0.56, 0.74), day) * dull;
    m.p.cam = cam.translation.extend(game.time % 3600.0);
    m.p.sun = to_light.extend(day);
    m.p.sky = sky.extend(game.rain);
    m.p.sunc = (Vec3::new(lc.red, lc.green, lc.blue) * k).extend((game.wind.length() / 24.0).min(1.0));
}

/// L: visit the next famous place (on foot only).
fn landmark_tour(keys: Res<ButtonInput<KeyCode>>, city: Res<City>, mut game: ResMut<Game>, mut next: Local<usize>) {
    if keys.just_pressed(KeyCode::KeyL) && game.in_car.is_none() && game.dead_t <= 0.0 && !city.landmarks.is_empty() {
        let l = &city.landmarks[*next % city.landmarks.len()];
        *next += 1;
        // arrive on its south side, so the camera looks across you at the building
        let r = l.pts.iter().map(|p| (p.y - l.c.y).abs()).fold(6.0, f32::max);
        game.pos = l.c + Vec2::new(0.0, r + 7.0);
        game.y = 0.0;
        game.travel_hold = 3.0;
        game.say(landmarks::name(l.kind), 3.0);
    }
}

fn camera_follow(
    time: Res<Time>,
    mut game: ResMut<Game>,
    screen: Res<Screen>,
    mut cam: Single<&mut Transform, With<MainCam>>,
    mut lamp: Single<(&mut Transform, &mut PointLight), (With<CarLamp>, Without<MainCam>)>,
) {
    let dt = time.delta_secs();
    let speed = game.vel.length();
    // look further ahead the faster the car goes, so there is time to react
    let mut want = game.pos + (game.vel * 0.7).clamp_length_max(42.0);
    if game.in_car.is_none() {
        want += (game.aim - game.pos).clamp_length_max(40.0) * 0.12;
    }
    let want_h = game.y.max(0.0) + game.zoom * if game.in_car.is_some() { 52.0 + (speed * 1.0).min(46.0) } else { 44.0 };
    let k = 1.0 - (-4.0 * dt).exp();
    let cur = game.cam_focus;
    let np = if cur.distance(want) > 300.0 { want } else { cur.lerp(want, k) };
    // the camera keeps its height above whatever ground is beneath it
    let rel = cam.translation.y - terrain(cur);
    let h = rel + (want_h - rel) * (1.0 - (-3.0 * dt).exp());
    game.cam_h = h;
    let sh = if game.shake > 0.0 {
        Vec2::new((game.time * 91.0).sin(), (game.time * 77.0).cos()) * game.shake * 1.6
    } else {
        Vec2::ZERO
    };
    // lean back a little: the camera sits south of the action and looks down at an angle
    let ground = terrain(np);
    cam.translation = Vec3::new(np.x + sh.x, ground + h, np.y + sh.y + h * TILT.tan());
    cam.look_at(Vec3::new(np.x + sh.x, ground, np.y + sh.y), Vec3::NEG_Z);
    game.cam_focus = np;
    let half_h = h / TILT.cos() * (FOV * 0.5).tan();
    game.view_half = Vec2::new(half_h * screen.size.x as f32 / screen.size.y as f32, half_h * 1.35);

    let f = Vec2::from_angle(game.heading);
    if game.in_car.is_some() {
        let p = game.pos + f * 7.0;
        lamp.0.translation = Vec3::new(p.x, terrain(p) + game.y + 2.2, p.y);
        lamp.1.intensity = LIGHT_POWER * 0.9 * game.night; // no pool of headlight on the road by day
    } else {
        lamp.0.translation = Vec3::new(game.pos.x, terrain(game.pos) + game.y + 3.0, game.pos.y);
        lamp.1.intensity = LIGHT_POWER * 0.12 * game.night;
    }
}

fn rain(
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut drops: Query<(&mut Transform, &Rain), Without<Ripple>>,
    mut ripples: Query<(&mut Transform, &mut Ripple), Without<Rain>>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let f = game.cam_focus;
    let ext = game.view_half + Vec2::splat(8.0);
    // wind slants the streaks; heavier weather puts more of the drop pool in the air
    let vel = Vec3::new(game.wind.x, -62.0, game.wind.y);
    let rot = Quat::from_rotation_arc(Vec3::Y, -vel.normalize());
    let amount = if game.rain < 0.03 { -1.0 } else { 0.12 + game.rain * 0.88 };
    for (mut t, r) in &mut drops {
        if r.0 > amount {
            if t.translation.y > -40.0 {
                t.translation.y = -50.0;
            }
            continue;
        }
        t.translation += vel * dt;
        t.rotation = rot;
        if t.translation.y < terrain(Vec2::new(t.translation.x, t.translation.z)) + game.y.min(0.0) {
            let h = game.rng.range(14.0, 42.0);
            // start upwind so the drop lands inside the view
            let lead = Vec2::new(vel.x, vel.z) * (h / 62.0);
            t.translation = Vec3::new(
                f.x + game.rng.range(-ext.x, ext.x) - lead.x,
                h + terrain(f),
                f.y + game.rng.range(-ext.y, ext.y) - lead.y,
            );
        }
    }
    for (mut t, mut r) in &mut ripples {
        if r.1 > amount {
            if t.translation.y > -40.0 {
                t.translation.y = -50.0;
            }
            continue;
        }
        r.0 += dt * 2.2;
        if r.0 >= 1.0 || t.translation.y < -40.0 {
            r.0 = 0.0;
            let p = f + Vec2::new(game.rng.range(-ext.x, ext.x), game.rng.range(-ext.y, ext.y));
            t.translation = Vec3::new(p.x, terrain(p) + game.y.min(0.0) + 0.45, p.y);
        }
        t.scale = Vec3::splat(0.2 + r.0 * (0.6 + game.rain * 0.5));
    }
}

fn hud(game: Res<Game>, city: Res<City>, mut texts: Query<(&Hud, Option<&mut Text>, Option<&mut Node>, Option<&mut TextColor>)>) {
    for (h, text, node, col) in &mut texts {
        match (h, text, node) {
            (Hud::Cash, Some(mut t), _) => {
                let gun = if game.smg && game.ammo > 0 { format!("SMG {}", game.ammo) } else { "PISTOL".into() };
                t.0 = format!("${}   {}", game.cash, gun);
            }
            (Hud::Heat, Some(mut t), _) => {
                let w = game.wanted() as usize;
                t.0 = if w > 0 { format!("HEAT [{}{}]", "#".repeat(w), "-".repeat(5 - w)) } else { String::new() };
            }
            (Hud::Health, _, Some(mut n)) => n.width = percent(game.health.clamp(0.0, 100.0)),
            (Hud::Place, Some(mut t), _) => {
                t.0 = format!(
                    "{}  //  {}  //  {:.0} m",
                    city.street(game.pos),
                    game.weather,
                    relief_height(game.pos)
                );
            }
            (Hud::Clock, Some(mut t), _) => {
                let c = game.clock;
                let phase = match c as u32 {
                    5..=7 => "DAWN",
                    8..=16 => "DAY",
                    17..=19 => "DUSK",
                    20..=21 => "EVENING",
                    _ => "NIGHT",
                };
                let txt = format!("{:02}:{:02}  {}", c as u32, (c.fract() * 60.0) as u32, phase);
                if t.0 != txt {
                    t.0 = txt;
                }
            }
            (Hud::Speed, Some(mut t), _) => {
                if game.in_car.is_some() {
                    let kmh = game.vel.length() * 3.6;
                    let limit = city.limit_at(game.pos);
                    t.0 = format!("{kmh:.0} km/h   LIMIT {limit}");
                    if let Some(mut c) = col {
                        let want = if kmh > limit as f32 + 9.0 { Color::srgb(1.0, 0.25, 0.3) } else { Color::srgb(0.6, 0.95, 1.0) };
                        if c.0 != want {
                            c.0 = want;
                        }
                    }
                } else if !t.0.is_empty() {
                    t.0.clear();
                }
            }
            (Hud::Job, Some(mut t), _) => {
                t.0 = match game.mission {
                    Mission::Deliver { target, reward, time } => {
                        format!("DELIVERY ${}  {:.0}m  {:.0}s", reward, target.distance(game.pos), time)
                    }
                    Mission::None => "Find a yellow terminal for courier work".into(),
                };
            }
            (Hud::Msg, Some(mut t), _) => {
                t.0 = if game.msg_t > 0.0 { game.msg.clone() } else { String::new() };
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct WeatherState {
    shadow: f32,
    phase: usize,
    phase_t: f32,
    strike_t: f32,
    flash: f32,
    wet: f32,
    night: f32,
    cloud: f32,
    started: bool,
    /// Whether the light materials have been set for the time of day yet.
    lit: bool,
}

/// Weather front: (label, rain, fog, storm, cloud cover). The order is a plausible progression;
/// each change steps one or two entries forward.
const FRONTS: [(&str, f32, f32, f32, f32); 11] = [
    ("CLEAR", 0.0, 0.0, 0.0, 0.0),
    ("PARTLY CLOUDY", 0.0, 0.0, 0.0, 0.35),
    ("OVERCAST", 0.0, 0.08, 0.0, 0.85),
    ("DRIZZLE", 0.28, 0.25, 0.0, 0.8),
    ("RAIN", 0.6, 0.1, 0.1, 0.9),
    ("DOWNPOUR", 0.95, 0.15, 0.45, 1.0),
    ("THUNDERSTORM", 1.0, 0.0, 1.0, 1.0),
    ("RAIN", 0.55, 0.2, 0.2, 0.9),
    ("PARTLY CLOUDY", 0.0, 0.0, 0.0, 0.4),
    ("CLEAR", 0.0, 0.0, 0.0, 0.0),
    ("FOG", 0.05, 1.0, 0.0, 0.7),
];

/// Changing weather, lightning, mist, wet-road shine and the shadow range that follows the camera.
fn weather(
    time: Res<Time>,
    mut game: ResMut<Game>,
    art: Res<Art>,
    cam: Single<(&mut Bloom, &mut bevy::pbr::DistanceFog), With<MainCam>>,
    mut mist: Query<(&mut Transform, &Mist, &mut Visibility), (Without<MainCam>, Without<DirectionalLight>)>,
    mut moon: Single<(&mut DirectionalLight, &mut CascadeShadowConfig, &mut Transform), (Without<MainCam>, Without<Mist>)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut st: Local<WeatherState>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let (mut bloom, mut haze) = cam.into_inner();

    // drift between fronts
    st.phase_t -= dt;
    if st.phase_t <= 0.0 {
        // the first front is the clear one the game starts in
        if st.started {
            st.phase = (st.phase + 1 + game.rng.int(2) as usize) % FRONTS.len();
        }
        st.started = true;
        st.phase_t = game.rng.range(40.0, 75.0);
    }
    let (label, rain, fog, storm, cloud) = FRONTS[st.phase];
    game.weather = if label == "CLEAR" && game.night < 0.5 { "SUNNY" } else { label };
    let k = (dt * 0.12).min(1.0);
    st.cloud += (cloud - st.cloud) * k;
    game.rain += (rain - game.rain) * k;
    game.fog += (fog - game.fog) * k;
    game.storm += (storm - game.storm) * k;
    game.wind = Vec2::from_angle(game.time * 0.03) * (2.0 + st.cloud * 2.0 + game.storm * 20.0 + game.rain * 4.0);

    // mist drifts with the wind; fog banks roll in when it turns foggy
    let f = game.cam_focus;
    let foggy = game.fog > 0.45;
    for (mut t, m, mut vis) in &mut mist {
        let v = m.0 + game.wind * 0.12;
        t.translation.x += v.x * dt;
        t.translation.z += v.y * dt;
        t.translation.y = terrain(Vec2::new(t.translation.x, t.translation.z)) + m.2;
        let d = Vec2::new(t.translation.x, t.translation.z) - f;
        if d.x.abs() > 110.0 {
            t.translation.x = f.x - d.x.signum() * 105.0;
        }
        if d.y.abs() > 80.0 {
            t.translation.z = f.y - d.y.signum() * 75.0;
        }
        // fog banks only in fog; the thin ground mist clears on dry days
        let show = if m.1 { foggy } else { game.rain.max(game.fog) > 0.1 || game.night > 0.6 };
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }

    // shadow range follows the camera height so shadows stay sharp
    let want = game.cam_h * 1.6 + 30.0;
    if (want - st.shadow).abs() > st.shadow * 0.03 {
        st.shadow = want;
        *moon.1 = CascadeShadowConfigBuilder { num_cascades: 1, minimum_distance: 1.0, maximum_distance: want, ..default() }.build();
    }

    // lightning: the whole city flashes white-blue and every building throws a hard shadow
    st.strike_t -= dt * game.storm * 1.75; // no lightning out of a dry sky
    if st.strike_t <= 0.0 {
        st.strike_t = game.rng.range(5.0, 12.0);
        st.flash = 1.0;
        if game.storm > 0.5 {
            game.shake = game.shake.max(0.12);
        }
    }
    st.flash = (st.flash - dt * 2.4).max(0.0);
    let fl = if st.flash > 0.0 { st.flash * (0.5 + 0.5 * (st.flash * 38.0).sin()) } else { 0.0 };
    // in daylight a strike is a flicker, not a white-out of the whole screen
    let fl = fl * (0.08 + 0.92 * game.night);
    // ---- day / night: one in-game hour per real minute ----
    game.clock = (game.clock + dt / 60.0) % 24.0;
    let c = game.clock;
    let sun = if c > 6.0 && c < 20.0 { ((c - 6.0) / 14.0 * std::f32::consts::PI).sin() } else { 0.0 };
    let dayf = (sun / 0.3).clamp(0.0, 1.0);
    let dayf = dayf * dayf * (3.0 - 2.0 * dayf);
    game.night = 1.0 - dayf;
    // the sun crosses from east to west; at night the moon sits in a fixed spot
    let a = (c - 6.0) / 14.0 * std::f32::consts::PI;
    // (it stands in the south, as it does over Vilnius: fronts facing the camera are sunlit and
    // shadows fall away behind the buildings)
    let sun_dir = Vec3::new(-a.cos() * 0.9, -(0.3 + sun * 0.9), -0.42).normalize();
    let moon_dir = Vec3::new(0.5, -1.0, 0.38).normalize();
    let dir = moon_dir.lerp(sun_dir, dayf).normalize();
    *moon.2 = Transform::default().looking_to(dir, Vec3::Y);
    let overcast = 1.0 - 0.45 * st.cloud.max(game.fog);
    // direct sunlight is much stronger under an open sky, and the moon a little
    let open = (1.0 - st.cloud) * (1.0 - game.fog);
    let direct = (0.45 + 0.55 * overcast) * (1.0 + 0.7 * open);
    let warm = 1.0 - sun; // low sun is orange
    let (lr, lg, lb) = (1.0, 0.95 - warm * 0.35, 0.86 - warm * 0.5);
    moon.0.illuminance = (680.0 * (1.0 - game.fog * 0.5) * (0.85 + 0.3 * open)) * (1.0 - dayf) + 1900.0 * (0.3 + 0.7 * sun) * direct * dayf + fl * 16000.0;
    moon.0.color = Color::srgb(
        (0.6 + (lr - 0.6) * dayf + fl * 0.35).min(1.0),
        (0.72 + (lg - 0.72) * dayf + fl * 0.25).min(1.0),
        1.0 + (lb - 1.0) * dayf,
    );
    ambient.brightness = (90.0 * (1.0 + game.fog * 0.9)) * (1.0 - dayf) + 330.0 * overcast * dayf + fl * 700.0;
    ambient.color = Color::srgb(0.45 + 0.3 * dayf, 0.5 + 0.34 * dayf, 1.0);
    // neon, windows and glows fade with daylight
    if (game.night - st.night).abs() > 0.015 || !st.lit {
        st.lit = true;
        st.night = game.night;
        if let Some(mut m) = mats.get_mut(&art.mat) {
            let e = 2.6 * (0.16 + 0.84 * game.night);
            m.emissive = LinearRgba::rgb(e, e, e);
        }
        if let Some(mut m) = mats.get_mut(&art.glow) {
            let g = 1.7 * game.night;
            m.base_color = Color::linear_rgb(g, g, g);
        }
    }
    // haze thickens in fog and takes the colour of the sky
    let (hs, he) = (game.cam_h * 1.5, game.cam_h * (7.0 - game.fog * 3.5 - game.rain * 1.0));
    haze.falloff = bevy::pbr::FogFalloff::Linear { start: hs, end: he.max(hs + 40.0) };
    let d = dayf * overcast;
    haze.color = Color::srgb(0.02 + 0.5 * d + fl * 0.3, 0.03 + 0.56 * d + fl * 0.3, 0.07 + 0.62 * d + fl * 0.3);
    let b = (0.26 + game.fog * 0.12) * (0.4 + 0.6 * game.night);
    if (bloom.intensity - b).abs() > 0.004 {
        bloom.intensity = b;
    }

    // the harder it rains, the glossier the streets
    if (game.rain - st.wet).abs() > 0.04 {
        st.wet = game.rain;
        for (m, dry, wet) in [(M_ASPHALT, 1.15, 0.62), (M_PAVING, 1.1, 0.7), (M_CONCRETE, 1.1, 0.75)] {
            if let Some(mut mat) = mats.get_mut(&art.tiled[m]) {
                mat.perceptual_roughness = (dry + (wet - dry) * st.wet).min(1.0);
            }
        }
        if let Some(mut mat) = mats.get_mut(&art.deck) {
            mat.perceptual_roughness = 1.0 - 0.38 * st.wet;
        }
    }
}

/// Headlights of the nearest traffic light the road for real; police cars strobe red and blue.
fn car_lights(
    game: Res<Game>,
    cars: Query<(&Transform, &Car)>,
    mut beams: Query<(&mut Transform, &mut PointLight), (With<CarBeam>, Without<Car>, Without<CopLight>)>,
    mut cops: Query<(&mut Transform, &mut PointLight), (With<CopLight>, Without<Car>, Without<CarBeam>)>,
) {
    let f = game.cam_focus;
    let mut list: Vec<(f32, Vec3, f32, bool)> = cars
        .iter()
        .filter(|(_, c)| c.lights && !c.player && c.health > 0.0)
        .map(|(t, c)| (Vec2::new(t.translation.x, t.translation.z).distance(f), t.translation, c.heading, c.police))
        .filter(|x| x.0 < 95.0)
        .collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut it = list.iter();
    for (mut t, mut l) in &mut beams {
        if let Some((d, pos, heading, _)) = it.next() {
            let fwd = Vec2::from_angle(*heading) * 6.5;
            t.translation = *pos + Vec3::new(fwd.x, 1.6, fwd.y);
            l.intensity = LIGHT_POWER * 0.5 * ((95.0 - d) / 30.0).clamp(0.0, 1.0) * game.night;
        } else {
            l.intensity = 0.0;
        }
    }
    let mut police = list.iter().filter(|x| x.3);
    for (i, (mut t, mut l)) in cops.iter_mut().enumerate() {
        match police.next() {
            Some((_, pos, _, _)) if game.wanted() > 0 => {
                let red = ((game.time * 7.0) as usize + i) % 2 == 0;
                t.translation = *pos + Vec3::Y * 2.6;
                l.color = if red { Color::srgb(1.0, 0.08, 0.08) } else { Color::srgb(0.1, 0.25, 1.0) };
                // (by day the beacons flash but do not flood the street with colour)
                l.intensity = LIGHT_POWER * 1.3 * (0.12 + 0.88 * game.night);
            }
            _ => l.intensity = 0.0,
        }
    }
}

/// Tyres throw up spray on wet roads.
fn spray(mut commands: Commands, time: Res<Time>, art: Res<Art>, mut game: ResMut<Game>, cars: Query<(&Transform, &Car)>) {
    let dt = time.delta_secs();
    let game = &mut *game;
    let f = game.cam_focus;
    for (t, c) in &cars {
        let speed = c.vel.length();
        let p = Vec2::new(t.translation.x, t.translation.z);
        if speed < 8.0 || p.distance(f) > 70.0 {
            continue;
        }
        if game.rng.chance(dt * speed * game.rain * 1.2) {
            let back = p - Vec2::from_angle(c.heading) * 2.2;
            burst(&mut commands, &art, &mut game.rng, 4, Vec3::new(back.x, if c.y.is_nan() { 0.4 } else { c.y + 0.4 }, back.y), 1, speed * 0.25, 0.55, 2.0);
        }
    }
}

/// Click handling for the on-screen zoom buttons (hit-tested against their fixed positions).
fn zoom_buttons(
    window: Single<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut game: ResMut<Game>,
    mut btns: Query<(&ZoomBtn, &mut BackgroundColor)>,
) {
    let cur = window.cursor_position();
    let mut hover = false;
    for (i, (b, mut bg)) in btns.iter_mut().enumerate() {
        // matches the layout in `setup`: 48px squares, 18px from the right, stacked from y = 120
        let (x0, y0) = (window.width() - 66.0, 120.0 + i as f32 * 58.0);
        let on = cur.is_some_and(|c| c.x >= x0 && c.x <= x0 + 48.0 && c.y >= y0 && c.y <= y0 + 48.0);
        hover |= on;
        if on && mouse.just_pressed(MouseButton::Left) {
            game.zoom = (game.zoom * b.0).clamp(0.45, 2.2);
        }
        let c = if on { Color::srgba(0.1, 0.3, 0.45, 0.95) } else { Color::srgba(0.05, 0.1, 0.18, 0.8) };
        if bg.0 != c {
            bg.0 = c;
        }
    }
    if game.ui_hover != hover {
        game.ui_hover = hover;
    }
}

/// Real Vilnius speed cameras: pass one too fast and it flashes and fines you.
fn speed_cameras(mut commands: Commands, art: Res<Art>, city: Res<City>, mut game: ResMut<Game>) {
    let game = &mut *game;
    if game.in_car.is_none() || game.dead_t > 0.0 {
        return;
    }
    let kmh = game.vel.length() * 3.6;
    let c0 = cell_of(game.pos);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let Some(src) = city.src(c0 + IVec2::new(dx, dz)) else { continue };
            for (p, limit) in &src.cams {
                if p.distance(game.pos) > 17.0 || kmh < *limit as f32 + 9.0 {
                    continue;
                }
                let key = ((p.x * 2.0) as i32, (p.y * 2.0) as i32);
                if game.cam_flash.get(&key).is_some_and(|t| game.time - *t < 10.0) {
                    continue;
                }
                game.cam_flash.insert(key, game.time);
                let over = kmh - *limit as f32;
                let fine = ((over * 6.0 / 10.0).ceil() as i64 * 10).max(50);
                game.cash -= fine;
                game.say(&format!("SPEED CAMERA  {kmh:.0} in a {limit} zone  -${fine}"), 3.0);
                // the flash
                burst(&mut commands, &art, &mut game.rng, 3, Vec3::new(p.x, 4.2, p.y), 1, 0.0, 0.22, 34.0);
                burst(&mut commands, &art, &mut game.rng, 3, Vec3::new(game.pos.x, 2.0, game.pos.y), 1, 0.0, 0.16, 22.0);
            }
        }
    }
}

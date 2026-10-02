//! VOID: the main game, as the TS game's `src/main.ts` (lab/flight). Aurelia inside the Sol
//! system, the two-stage demo rocket on scenery's layered terrain, Rapier contact physics near the
//! ground and orbit propagation in flight, staging, time warp with on-rails coasting, and lab/view's
//! single view from the pad out to the map.
//!
//! `--planet aurelia|aurelia-fast|terra|luna|pebble`, `--terrain layered|hills`.
//!
//! Space: ignite the booster, then separate and ignite the upper stage | Shift / Ctrl: throttle,
//! X: cut | W/S pitch, A/D yaw, Q/E roll | `,` `.`: time rate | P: pause | R: reset | drag: orbit
//! the camera, wheel: zoom out into the map | Tab or a label: focus | G: path frame | K: AGL / ALT
//! | L: SURFACE / ORBIT | F1: keys.
//!
//! Drawn in the planet's body-fixed axes with the camera at the origin: terrain tiles and the
//! rocket's parts (body-fixed in the physics) need no turning; bodies and the map (f64 in the
//! ecliptic) are turned by the planet's orientation each frame.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use glam::DVec3;
use void_app::flight::{
    GamePlanet, PARTS, PHYSICS_MAX_RATE, TIME_RATES, distance_text, game_planet_by_id,
    mission_time, rate_text, warp_limit,
};
use void_app::map::{
    MapMarker, PATH_COLOR, color, draw_map_lines, label_click, place_map_labels, spawn_map_labels,
};
use void_app::parts::spawn_shape;
use void_app::tiles::{Tile, TileField};
use void_landing::{
    CoastPrediction, DemoRocket, FrameState, LanderControl, PartJointRocket, PhysicsMode,
    RocketPart, demo_rocket, landing_lod_options, planet_ephemeris, predict_coast,
};
use void_lod::{LodCamera, LodView};
use void_orbit::{CelestialBody, DominanceTree, Ephemeris, body_orientation, osculating_orbit};
use void_view::{
    FocusGeometry, FocusKind, LabelKind, MapFrame, MapOrbits, MapPath, OrbitCamera, PathFrameKind,
    PlottingFrame, ViewMode, ViewState, camera_spin, map_labels, view_state,
};

/// The coast forecast is recomputed this often in simulated time, and at every staging.
const PREDICTION_INTERVAL_SECONDS: f64 = 2.0;
/// Long enough for one low orbit; a suborbital coast ends at the ground first.
const PREDICTION_HORIZON_SECONDS: f64 = 6000.0;
const THROTTLE_RATE_PERCENT_PER_SECOND: f64 = 50.0;
const VESSEL_DISTANCE: f64 = 45.0;
const FOV_DEGREES: f32 = 58.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb_u8(0xcb, 0xe7, 0xff),
            brightness: 40.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, simulate, terrain, draw, labels, hud).chain(),
        )
        .run();
}

fn argument(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Vessel,
    Body(usize),
}

#[derive(Resource)]
struct Game {
    planet: GamePlanet,
    ephemeris: Ephemeris,
    home: usize,
    bodies: Vec<CelestialBody>,
    dominance: DominanceTree,
    demo: DemoRocket,
    rocket: PartJointRocket,
    /// 0 on the pad, 1 booster burning, 2 upper stage on its own.
    stage: u8,
    engine_armed: bool,
    throttle_percent: f64,
    paused: bool,
    prediction: Option<CoastPrediction>,
    prediction_at: f64,
    prediction_generation: u64,
    time_rate: f64,
    warp_note: Option<(String, std::time::Instant)>,
    altitude_agl: bool,
    speed_surface: bool,
    path_frame: PathFrameKind,
    focus: Focus,
    camera: OrbitCamera,
    state: Option<ViewState>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    orbits: MapOrbits,
    path: MapPath,
    started: std::time::Instant,
    help: bool,
    // This frame's geometry: the camera (ecliptic, barycentric), the render axes (the planet's
    // body-fixed axes), the upper stage, the focus, its reference body, the dominant body.
    eye: DVec3,
    axes: [DVec3; 3],
    upper: FrameState,
    origin: DVec3,
    reference: usize,
    navigation: usize,
    spin: (usize, f64),
}

impl Game {
    fn launch(&mut self) -> PartJointRocket {
        PartJointRocket::landed(
            &mut self.ephemeris,
            self.home,
            self.planet.planet.terrain.clone(),
            self.demo.full.clone(),
            self.demo.upper.clone(),
            self.demo.booster.clone(),
            self.demo.options,
            self.demo.launch_site,
        )
    }

    /// An ecliptic vector in render axes (the planet's body-fixed axes).
    fn render(&self, v: DVec3) -> Vec3 {
        DVec3::new(
            v.dot(self.axes[0]),
            v.dot(self.axes[1]),
            v.dot(self.axes[2]),
        )
        .as_vec3()
    }

    /// A barycentric point in the planet's body-fixed frame.
    fn body_fixed(&self, p: DVec3) -> DVec3 {
        let d = p - self.positions[self.home];
        DVec3::new(
            d.dot(self.axes[0]),
            d.dot(self.axes[1]),
            d.dot(self.axes[2]),
        )
    }

    fn part_inertial(&self, which: RocketPart) -> FrameState {
        let t = self.rocket.time();
        self.rocket.frame.to_inertial(
            &self.ephemeris,
            t,
            self.rocket.part_state(&self.ephemeris, which),
        )
    }

    fn live_parts(&self) -> Vec<RocketPart> {
        PARTS
            .into_iter()
            .filter(|&p| self.rocket.part_mode(p) != PhysicsMode::Destroyed)
            .collect()
    }

    fn engine_throttle(&self) -> f64 {
        if self.engine_armed {
            self.throttle_percent / 100.0
        } else {
            0.0
        }
    }

    fn warp_limit(&self) -> (f64, Option<String>) {
        warp_limit(
            &self.rocket,
            &self.ephemeris,
            self.engine_throttle(),
            self.bodies[self.home].radius_meters,
        )
    }

    fn note(&mut self, text: String) {
        self.warp_note = Some((text, std::time::Instant::now()));
    }

    /// Set a rate no higher than the current limit; asking for more says why it is held lower. An
    /// on-rails rate that is not allowed leaves a physics rate as it is rather than raising it.
    fn set_time_rate(&mut self, requested: f64) {
        let (limit, reason) = self.warp_limit();
        let rate = if requested <= limit {
            requested
        } else if limit > PHYSICS_MAX_RATE {
            limit
        } else {
            self.time_rate.min(limit)
        };
        if rate < requested
            && let Some(reason) = reason
        {
            self.note(format!("{}x needs {reason}", requested));
        }
        self.time_rate = rate;
    }

    fn step_time_rate(&mut self, step: i32) {
        let index = TIME_RATES
            .iter()
            .position(|&r| r == self.time_rate)
            .expect("time rate in the row") as i32;
        let next = (index + step).clamp(0, TIME_RATES.len() as i32 - 1) as usize;
        self.set_time_rate(TIME_RATES[next]);
    }

    fn set_focus(&mut self, next: Focus) {
        self.focus = next;
        self.camera.distance = match next {
            Focus::Vessel => VESSEL_DISTANCE,
            Focus::Body(i) => self.bodies[i].radius_meters * 4.0,
        };
    }

    fn stage(&mut self) {
        match self.stage {
            0 => {
                self.stage = 1;
                self.engine_armed = true;
            }
            1 => {
                self.rocket.separate(&self.ephemeris);
                self.stage = 2;
                self.engine_armed = true;
                self.prediction_at = f64::NEG_INFINITY;
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.rocket = self.launch();
        self.stage = 0;
        self.engine_armed = false;
        self.throttle_percent = 0.0;
        self.paused = false;
        self.time_rate = 1.0;
        self.prediction = None;
        self.prediction_at = f64::NEG_INFINITY;
        self.prediction_generation += 1;
        self.focus = Focus::Vessel;
        self.camera.distance = VESSEL_DISTANCE;
    }

    /// The focus's geometry, position and reference body.
    fn focus_geometry(&self, vessel: DVec3) -> (FocusGeometry, DVec3, usize) {
        match self.focus {
            Focus::Vessel => {
                let reference = self.dominance.dominant(&self.positions, vessel);
                let body = &self.bodies[reference];
                let from_centre = vessel - self.positions[reference];
                let r = from_centre.length();
                (
                    FocusGeometry {
                        kind: FocusKind::Vessel,
                        radial: Some(from_centre / r),
                        north: body.rotation.axis(),
                        reference_radius: body.radius_meters,
                        altitude: r - body.radius_meters,
                        focus_radius: 0.0,
                    },
                    vessel,
                    reference,
                )
            }
            Focus::Body(i) => {
                let body = &self.bodies[i];
                (
                    FocusGeometry {
                        kind: FocusKind::Body,
                        radial: None,
                        north: body.rotation.axis(),
                        reference_radius: body.radius_meters,
                        altitude: 0.0,
                        focus_radius: body.radius_meters,
                    },
                    self.positions[i],
                    i,
                )
            }
        }
    }

    fn map_frame(&self) -> MapFrame<'_> {
        MapFrame {
            time: self.rocket.time(),
            positions: &self.positions,
            velocities: &self.velocities,
            origin: self.eye,
            vessel: self.upper.position,
            vessel_velocity: self.upper.velocity,
            plotting: PlottingFrame {
                kind: self.path_frame,
                reference: self.navigation,
            },
            wall_ms: self.started.elapsed().as_secs_f64() * 1e3,
        }
    }
}

#[derive(Resource)]
struct Ground(TileField);

#[derive(Component)]
struct BodySphere(usize);

#[derive(Component)]
struct Part(RocketPart);

#[derive(Component)]
struct Flame(RocketPart);

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Hud;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let planet_id = argument("--planet").unwrap_or_else(|| "aurelia".into());
    let planet = game_planet_by_id(&planet_id, argument("--terrain").as_deref());
    let (mut ephemeris, home) = planet_ephemeris(&planet.planet);
    let bodies = ephemeris.bodies().to_vec();
    let mut demo = demo_rocket(&planet.planet.terrain);
    if let Some(site) = planet.launch_site {
        demo.launch_site = site;
    }
    let rocket = PartJointRocket::landed(
        &mut ephemeris,
        home,
        planet.planet.terrain.clone(),
        demo.full.clone(),
        demo.upper.clone(),
        demo.booster.clone(),
        demo.options,
        demo.launch_site,
    );

    let ground = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.insert_resource(Ground(TileField::new(
        landing_lod_options(&planet.planet.terrain, &demo.options.contact),
        Some(planet.planet.terrain.clone()),
        ground,
    )));

    // Start looking at the rocket from the side, a little above the horizon.
    let start = rocket.frame.to_inertial(
        &ephemeris,
        0.0,
        rocket.part_state(&ephemeris, RocketPart::Upper),
    );
    let radial = (start.position - ephemeris.body_position(home, 0.0)).normalize();
    let side = DVec3::new(-radial.y, radial.x, 0.0).normalize();
    let camera = OrbitCamera::new((side + 0.3 * radial).normalize(), VESSEL_DISTANCE);
    let n = bodies.len();
    spawn_map_labels(&mut commands, &bodies);

    // Bodies other than the home planet are plain spheres; the star is unlit.
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    for body in &bodies {
        if body.index == home {
            continue;
        }
        let material = materials.add(StandardMaterial {
            base_color: color(&body.color),
            unlit: body.parent_index.is_none(),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.spawn((
            BodySphere(body.index),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }

    // The rocket's parts, drawn from their colliders, with a flame under each nozzle.
    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.93, 0.95),
        perceptual_roughness: 0.5,
        ..default()
    });
    let booster_hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.77, 0.8),
        perceptual_roughness: 0.6,
        ..default()
    });
    let flame = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.6, 0.2),
        emissive: LinearRgba::rgb(8.0, 3.0, 0.6),
        unlit: true,
        ..default()
    });
    let flame_mesh = meshes.add(Cone {
        radius: 0.4,
        height: 1.0,
    });
    for (which, shape, material, nozzle) in [
        (RocketPart::Upper, &demo.upper_shape, &hull, -1.05),
        (
            RocketPart::Booster,
            &demo.booster_shape,
            &booster_hull,
            -1.4,
        ),
    ] {
        commands
            .spawn((Part(which), Transform::default(), Visibility::default()))
            .with_children(|parent| {
                spawn_shape(parent, shape, &mut meshes, material);
                parent.spawn((
                    Flame(which),
                    Mesh3d(flame_mesh.clone()),
                    MeshMaterial3d(flame.clone()),
                    Transform::from_xyz(0.0, nozzle as f32, 0.0)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
                    Visibility::Hidden,
                ));
            });
    }

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            far: 1e14,
            ..default()
        }),
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 10_000.0,
            ..default()
        },
        Transform::default(),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));

    let dominance = DominanceTree::new(&bodies);
    commands.insert_resource(Game {
        orbits: MapOrbits::new(&bodies),
        path: MapPath::new(),
        planet,
        ephemeris,
        home,
        dominance,
        demo,
        rocket,
        stage: 0,
        engine_armed: false,
        throttle_percent: 0.0,
        paused: false,
        prediction: None,
        prediction_at: f64::NEG_INFINITY,
        prediction_generation: 0,
        time_rate: 1.0,
        warp_note: None,
        altitude_agl: true,
        speed_surface: true,
        path_frame: PathFrameKind::Inertial,
        focus: Focus::Vessel,
        camera,
        state: None,
        positions: vec![DVec3::ZERO; n],
        velocities: vec![DVec3::ZERO; n],
        started: std::time::Instant::now(),
        help: false,
        eye: DVec3::ZERO,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        upper: start,
        origin: start.position,
        reference: home,
        navigation: home,
        spin: (home, 0.0),
        bodies,
    });
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    keys.pressed(positive) as i32 as f64 - keys.pressed(negative) as i32 as f64
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    markers: Query<(&Interaction, &MapMarker)>,
    mut game: ResMut<Game>,
    mut dragging: Local<bool>,
) {
    let game = &mut *game;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::Space) {
        game.stage();
    }
    if keys.just_pressed(KeyCode::KeyX) {
        game.throttle_percent = 0.0;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        game.paused = !game.paused;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        game.path_frame = match game.path_frame {
            PathFrameKind::Inertial => PathFrameKind::Surface,
            PathFrameKind::Surface => PathFrameKind::Inertial,
        };
    }
    if keys.just_pressed(KeyCode::KeyK) {
        game.altitude_agl = !game.altitude_agl;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        game.speed_surface = !game.speed_surface;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        game.reset();
    }
    if keys.just_pressed(KeyCode::Comma) {
        game.step_time_rate(-1);
    }
    if keys.just_pressed(KeyCode::Period) {
        game.step_time_rate(1);
    }
    if keys.just_pressed(KeyCode::F1) {
        game.help = !game.help;
    }
    if keys.just_pressed(KeyCode::Tab) {
        let mut order = vec![Focus::Vessel];
        order.extend((0..game.bodies.len()).map(Focus::Body));
        let current = order.iter().position(|&f| f == game.focus).unwrap_or(0);
        let step = if shift { order.len() - 1 } else { 1 };
        game.set_focus(order[(current + step) % order.len()]);
    }

    let map_weight = game.state.map_or(0.0, |s| s.map_weight);
    let (over_label, clicked) = label_click(&markers, &buttons, map_weight);
    match clicked {
        Some(LabelKind::Vessel) => game.set_focus(Focus::Vessel),
        Some(LabelKind::Body(i)) => game.set_focus(Focus::Body(i)),
        Some(LabelKind::Star) => {
            let star = game.bodies.iter().position(|b| b.parent_index.is_none());
            game.set_focus(Focus::Body(star.expect("a star")));
        }
        Some(LabelKind::Apsis) | None => {}
    }
    let any = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
    if buttons.any_just_pressed(any) && !over_label {
        *dragging = true;
    }
    if !buttons.any_pressed(any) {
        *dragging = false;
    }
    if let Some(state) = game.state {
        if *dragging && motion.delta != Vec2::ZERO {
            game.camera
                .drag(motion.delta.x as f64, motion.delta.y as f64, state.up);
        }
        let pixels = match scroll.unit {
            MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
            MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
        };
        if pixels != 0.0 {
            game.camera.zoom(
                (pixels * 0.0012).exp(),
                state.min_distance,
                state.max_distance,
            );
        }
    }
}

fn simulate(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>) {
    let game = &mut *game;
    // At most 50 ms of wall time per frame: a stalled frame does not become a physics leap.
    let wall = time.delta_secs_f64().clamp(0.0, 0.05);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let delta = shift as i32 as f64 - ctrl as i32 as f64;
    if delta != 0.0 && !keys.just_pressed(KeyCode::Tab) {
        game.throttle_percent = (game.throttle_percent
            + delta * wall * THROTTLE_RATE_PERCENT_PER_SECOND)
            .clamp(0.0, 100.0);
    }
    let before = game.rocket.time();
    // Burning, waking on the ground or coming down lowers the rate at once, as KSP does. Falling out
    // of on-rails goes straight to 1x, so there is time to react.
    let (limit, reason) = game.warp_limit();
    if game.time_rate > limit {
        if let Some(reason) = reason {
            game.note(format!("{}x dropped: needs {reason}", game.time_rate));
        }
        game.time_rate = if limit > PHYSICS_MAX_RATE { limit } else { 1.0 };
    }
    if !game.paused {
        let dt = wall * game.time_rate;
        if game.time_rate > PHYSICS_MAX_RATE {
            game.rocket.advance_on_rails(&mut game.ephemeris, dt);
        } else {
            let control = LanderControl {
                throttle: game.engine_throttle(),
                up: 1.0,
                turn: Some(DVec3::new(
                    axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
                    axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
                    axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
                )),
                ..Default::default()
            };
            game.rocket.advance(&mut game.ephemeris, dt, &control, None);
        }
    }
    // A booster lost while attached leaves the upper stage flying on its own.
    if game.stage == 1 && game.rocket.separated() {
        game.stage = 2;
    }
    let t = game.rocket.time();
    game.ephemeris
        .states_at(t, &mut game.positions, Some(&mut game.velocities));
    update_prediction(game);

    game.axes = body_orientation(&game.bodies[game.home].rotation, t);
    game.upper = game.part_inertial(RocketPart::Upper);
    let (geometry, origin, reference) = game.focus_geometry(game.upper.position);
    let state = view_state(ViewMode::Single, false, &geometry, game.camera.distance);
    game.camera.distance =
        game.camera
            .clamp_distance(game.camera.distance, state.min_distance, state.max_distance);
    // The camera turns with the ground near it, or with the path frame on the map.
    let navigation = game
        .dominance
        .dominant(&game.positions, game.upper.position);
    let spin = camera_spin(&state, game.path_frame, reference, navigation);
    if spin.1 > 0.0 && t > before {
        let rotation = game.bodies[spin.0].rotation;
        game.camera
            .corotate(rotation.axis(), rotation.rate() * (t - before) * spin.1);
    }
    game.camera.clamp_to_up(state.up);
    game.eye = origin + game.camera.direction * game.camera.distance;
    game.state = Some(state);
    game.origin = origin;
    game.reference = reference;
    game.navigation = navigation;
    game.spin = spin;
}

fn update_prediction(game: &mut Game) {
    let clearance = game.rocket.clearance(&game.ephemeris);
    if clearance < game.rocket.spec().half_extents.y {
        game.prediction = None;
        return;
    }
    let t = game.rocket.time();
    if t - game.prediction_at < PREDICTION_INTERVAL_SECONDS {
        return;
    }
    game.prediction_at = t;
    let state = game.rocket.body_fixed_state(&game.ephemeris);
    game.prediction = Some(predict_coast(
        &mut game.ephemeris,
        &game.rocket.frame,
        &game.planet.planet.terrain,
        game.rocket.options.tolerances,
        t,
        state,
        game.rocket.mass_kg(),
        PREDICTION_HORIZON_SECONDS,
    ));
    game.prediction_generation += 1;
}

/// lab/lod's observers: every live part, plus the camera, which also alone decides horizon
/// culling. The camera splits with the same table, stopping one level above the collision level;
/// the rocket's own detail stops where its cells would be under 2 px on screen.
fn terrain(game: Res<Game>, mut ground: ResMut<Ground>, window: Single<&Window>) {
    let observers = game
        .live_parts()
        .into_iter()
        .map(|p| game.rocket.part_state(&game.ephemeris, p).position)
        .collect();
    let height = window.physical_height().max(1) as f64;
    let focal_pixels = height / 2.0 / (f64::from(FOV_DEGREES).to_radians() / 2.0).tan();
    let max_level = ground.0.lod.options.max_level.saturating_sub(1);
    ground.0.finish_builds();
    ground.0.select(&LodView {
        observer_positions: observers,
        camera: Some(LodCamera {
            position: game.body_fixed(game.eye),
            distance_scale: 1.0,
            max_level,
            focal_pixels,
            min_observer_cell_pixels: 2.0,
        }),
        distance_scale: 1.0,
        horizon_culling: true,
    });
}

type CameraOnly = (
    With<Camera3d>,
    Without<Tile>,
    Without<BodySphere>,
    Without<Part>,
    Without<Flame>,
    Without<Sun>,
);
type SphereOnly = (Without<Tile>, Without<Part>, Without<Flame>, Without<Sun>);
type PartOnly = (Without<Tile>, Without<Flame>, Without<Sun>);
type FlameOnly = (Without<Tile>, Without<Sun>);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Transform, With<Tile>>,
    mut camera: Single<(&mut Transform, &mut Projection), CameraOnly>,
    mut spheres: Query<(&BodySphere, &mut Transform), SphereOnly>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), PartOnly>,
    mut flames: Query<(&Flame, &mut Transform, &mut Visibility), FlameOnly>,
    mut sun: Single<&mut Transform, (With<Sun>, Without<Tile>)>,
    mut gizmos: Gizmos,
) {
    let game = &mut *game;
    let Some(state) = game.state else { return };

    // Camera at the origin, looking at the focus, up as the view state says.
    let (camera_transform, projection) = &mut *camera;
    **camera_transform =
        Transform::default().looking_to(game.render(-game.camera.direction), game.render(state.up));
    if let Projection::Perspective(p) = &mut **projection {
        p.near = (game.camera.distance * 1e-3).max(0.05) as f32;
    }

    // Tiles and parts are body-fixed: relative to the camera in body-fixed axes, already render
    // axes.
    let eye = game.body_fixed(game.eye);
    ground.0.draw(&mut commands, &mut meshes, &mut tiles, eye);
    for (part, mut transform, mut visibility) in &mut parts {
        let alive = game.rocket.part_mode(part.0) != PhysicsMode::Destroyed;
        *visibility = if alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let state = game.rocket.part_state(&game.ephemeris, part.0);
        *transform = Transform::from_translation((state.position - eye).as_vec3())
            .with_rotation(game.rocket.part_orientation(part.0).as_quat());
    }
    let firing = if !game.paused && game.engine_armed && game.rocket.fuel_kg() > 0.0 {
        game.throttle_percent / 100.0
    } else {
        0.0
    };
    for (flame, mut transform, mut visibility) in &mut flames {
        let burning = firing > 0.0
            && match flame.0 {
                RocketPart::Booster => game.stage == 1,
                RocketPart::Upper => game.stage == 2,
            };
        *visibility = if burning {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let length = (2.0 + 6.0 * firing) as f32;
        let nozzle = if flame.0 == RocketPart::Upper {
            -1.05
        } else {
            -1.4
        };
        transform.scale = Vec3::new(1.0, length, 1.0);
        transform.translation.y = nozzle - length / 2.0;
    }

    for (body, mut transform) in &mut spheres {
        let radius = game.bodies[body.0].radius_meters as f32;
        *transform = Transform::from_translation(game.render(game.positions[body.0] - game.eye))
            .with_scale(Vec3::splat(radius));
    }
    // Lone-body planets have a fixed inertial light; solar-system flights use the real Sun.
    let star = game.bodies.iter().position(|b| b.parent_index.is_none());
    let sunward = match star {
        Some(s) if s != game.home => game
            .render(game.positions[s] - game.positions[game.home])
            .normalize(),
        _ => game.render(DVec3::X),
    };
    **sun = Transform::default().looking_to(-sunward, Vec3::Y);

    // The map: bodies' orbits and the coast forecast, at the map weight's opacity.
    let bodies = game.bodies.clone();
    // Taken out while the frame borrows the game.
    let mut orbits = std::mem::replace(&mut game.orbits, MapOrbits::new(&[]));
    let mut path = std::mem::take(&mut game.path);
    let frame = game.map_frame();
    orbits.update(&bodies, &frame);
    match &game.prediction {
        Some(prediction) => path.update(
            &game.ephemeris,
            &prediction.trajectory,
            game.prediction_generation,
            &frame,
            true,
        ),
        None => path.hide(),
    }
    let render = |v: DVec3| game.render(v);
    draw_map_lines(
        &mut gizmos,
        &bodies,
        &orbits,
        &[(&path, color(PATH_COLOR))],
        &frame,
        state.map_weight as f32,
        &render,
    );
    game.orbits = orbits;
    game.path = path;
}

#[allow(clippy::type_complexity)]
fn labels(
    game: Res<Game>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut markers: Query<(&MapMarker, &mut Node, &mut Visibility, &ComputedNode)>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<MapMarker>>,
) {
    let Some(state) = game.state else { return };
    let (camera, camera_transform) = *camera;
    let frame = game.map_frame();
    let focus = match game.focus {
        Focus::Vessel => None,
        Focus::Body(i) => Some(i),
    };
    let apsides = game.path.apsis_positions(&frame);
    let wanted = map_labels(&game.bodies, &frame, focus, &apsides);
    let render = |v: DVec3| game.render(v);
    place_map_labels(
        camera,
        camera_transform,
        &mut markers,
        &mut texts,
        &wanted,
        state.map_weight,
        &render,
    );
}

fn hud(game: Res<Game>, ground: Res<Ground>, mut text: Single<&mut Text, With<Hud>>) {
    let Some(state) = game.state else { return };
    let rocket = &game.rocket;
    let body = &game.bodies[game.navigation];
    let home = &game.bodies[game.home];
    let r = game.upper.position - game.positions[game.navigation];
    let v = game.upper.velocity - game.velocities[game.navigation];
    let osc = osculating_orbit(r, v, body.gm);
    let (limit, reason) = game.warp_limit();

    let rates: String = TIME_RATES
        .iter()
        .map(|&rate| {
            let label = rate_text(rate);
            if rate == game.time_rate {
                format!("[{label}]")
            } else if rate > limit {
                format!(" ({label})")
            } else {
                format!(" {label} ")
            }
        })
        .collect();
    let note = match &game.warp_note {
        Some((note, at)) if at.elapsed().as_secs_f64() < 3.0 => note.clone(),
        _ => String::new(),
    };
    let blocked = reason
        .map(|r| format!("above {}: needs {r}", rate_text(limit)))
        .unwrap_or_default();

    let altitude = if game.altitude_agl {
        format!(
            "AGL  {}",
            distance_text(
                (rocket.clearance(&game.ephemeris) - rocket.spec().half_extents.y).max(0.0)
            )
        )
    } else {
        format!("ALT  {}", distance_text(r.length() - body.radius_meters))
    };
    let speed = if game.speed_surface {
        let ground_speed = rocket.body_fixed_state(&game.ephemeris).velocity.length();
        format!("SURFACE {ground_speed:.1} m/s over {}", home.name)
    } else {
        format!("ORBIT   {:.1} m/s about {}", v.length(), body.name)
    };
    let engine = if !game.engine_armed {
        "unlit"
    } else if game.throttle_percent > 0.0 && rocket.fuel_kg() > 0.0 {
        "firing"
    } else {
        "staged"
    };
    let mut stages = String::new();
    for which in PARTS {
        let (name, order, capacity) = match which {
            RocketPart::Booster => ("booster", 1, game.demo.booster.fuel_mass_kg),
            RocketPart::Upper => ("upper stage", 2, game.demo.upper.fuel_mass_kg),
        };
        let status = if rocket.part_mode(which) == PhysicsMode::Destroyed {
            "lost"
        } else if game.stage > order {
            "separated"
        } else if game.stage == order {
            "ACTIVE"
        } else if game.stage + 1 == order {
            "next"
        } else {
            "waiting"
        };
        let left = rocket.part_fuel_kg(which);
        stages += &format!(
            "  {name:<12} {status:<9} fuel {left:>6.0} kg ({:>3.0}%)  dv {:>5.0} m/s\n",
            100.0 * left / capacity,
            rocket.part_delta_v(which)
        );
    }
    let hint = match game.stage {
        0 => "Space: ignite booster",
        1 => "Space: separate, ignite upper stage",
        _ => "",
    };
    let orbit = if state.map_weight > 0.0 {
        let impact = game
            .prediction
            .as_ref()
            .and_then(|p| p.impact)
            .map(|(at, _)| format!("in {:.0} s", (at - rocket.time()).max(0.0)))
            .unwrap_or_else(|| "-".into());
        let apoapsis = if osc.apoapsis_radius_meters.is_finite() {
            distance_text(osc.apoapsis_radius_meters - body.radius_meters)
        } else {
            "escape".into()
        };
        format!(
            "\nORBIT | {}  ({:.0}% map)\n  Ap {apoapsis}\n  Pe {}\n  impact {impact}\n  PATH {}\n",
            body.name,
            state.map_weight * 100.0,
            distance_text(osc.periapsis_radius_meters - body.radius_meters),
            if game.path_frame == PathFrameKind::Surface {
                format!("with {}'s surface (G)", body.name)
            } else {
                "inertial (G)".into()
            },
        )
    } else {
        String::new()
    };
    let help = if game.help {
        "\nSpace: ignite booster, then separate and ignite the upper stage\n\
         Shift/Ctrl: throttle | X: cut | W/S pitch | A/D yaw | Q/E roll\n\
         , . time rate | P pause | R reset\n\
         drag: orbit camera | wheel: zoom out into the map\n\
         Tab or a label: focus | G: path frame | K: AGL/ALT | L: SURFACE/ORBIT | F1: keys\n"
    } else {
        "\nF1: keys\n"
    };
    let focus = match game.focus {
        Focus::Vessel => format!("vessel (reference {})", game.bodies[game.reference].name),
        Focus::Body(i) => game.bodies[i].name.clone(),
    };
    text.0 = format!(
        "{}  {}{}\n{rates}\n{note}{blocked}\n\n\
         STAGES | {:?}\n{stages}  {hint}\n\n\
         THR {:>3.0}%  {engine}\n\
         {altitude}\n\
         {speed}\n{orbit}\n\
         focus   {focus}\n\
         camera  {} | map {:.0}% | up {:.0}% | co-rotate {:.0}% {}\n\
         tiles   {} drawn, L{}-L{}, {} building\n\
         planet  {}{help}",
        mission_time(rocket.time()),
        rate_text(game.time_rate),
        if game.paused { "  PAUSED" } else { "" },
        rocket.mode(),
        game.throttle_percent,
        distance_text(game.camera.distance),
        state.map_weight * 100.0,
        state.up_weight * 100.0,
        game.spin.1 * 100.0,
        game.bodies[game.spin.0].name,
        ground.0.drawn_count(),
        ground.0.levels.0,
        ground.0.levels.1,
        ground.0.building_count(),
        // The default font has no middle dot or superscripts.
        game.planet.planet.label.replace('·', "|").replace('²', "2"),
    );
}

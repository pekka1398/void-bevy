//! The Sol system from the Rust ephemeris, every object placed through the frame tree relative
//! to the camera. The check is the probe on Aurelia's surface: Aurelia is 1 AU from the root and
//! moves at 30 km/s, yet seen from a few metres the probe must hold still.

use std::f64::consts::FRAC_PI_2;
use std::ops::Not;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use void_frames::{BodyId, FrameId, FrameTree, Motion};
use void_orbit::{Ephemeris, EphemerisOptions, SystemSpec, build_system, suggested_step_seconds};

const SYSTEM: &str = include_str!("../../orbit/systems/sol.json");
const HOME: &str = "aurelia";
/// Probe site on the home planet, degrees.
const PROBE_LATITUDE: f64 = 12.0;
const PROBE_LONGITUDE: f64 = 35.0;
const PROBE_SIZE: f64 = 10.0;
const MAX_DISTANCE: f64 = 2e13;
const WARPS: [f64; 8] = [1.0, 10.0, 100.0, 1e3, 1e4, 1e5, 1e6, 1e7];

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "void · system view".into(), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight { brightness: 30.0, ..default() })
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, advance, place, labels).chain())
        .run();
}

/// An object placed by the frame tree: a position and orientation in `frame`.
#[derive(Component)]
struct Anchor {
    frame: FrameId,
    position: DVec3,
    orientation: DQuat,
    /// Bounding radius, for the near plane.
    radius: f64,
}

/// Something the camera can orbit.
struct Target {
    name: String,
    /// Non-rotating frame the camera uses by default.
    inertial: FrameId,
    /// Frame turning with the target, for co-rotation.
    surface: FrameId,
    radius: f64,
}

#[derive(Resource)]
struct Sim {
    tree: FrameTree,
    ephemeris: Ephemeris,
    time: f64,
    warp: usize,
    paused: bool,
    targets: Vec<Target>,
    sun: FrameId,
}

#[derive(Resource)]
struct View {
    target: usize,
    corotate: bool,
    distance: f64,
    yaw: f64,
    pitch: f64,
}

impl View {
    fn frame(&self, sim: &Sim) -> FrameId {
        let target = &sim.targets[self.target];
        if self.corotate { target.surface } else { target.inertial }
    }

    /// Camera position in the view frame.
    fn eye(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    fn min_distance(&self, sim: &Sim) -> f64 {
        sim.targets[self.target].radius * 1.000_001 + 2.0
    }
}

#[derive(Component)]
struct Hud;

/// A name drawn on screen next to an anchor, since planets are below a pixel at system scale.
#[derive(Component)]
struct Label(Entity);

fn spawn_label(commands: &mut Commands, anchor: Entity, name: &str) {
    commands.spawn((
        Label(anchor),
        Text::new(format!("+ {name}")),
        TextFont { font_size: FontSize::Px(12.0), ..default() },
        TextColor(Color::srgb(0.7, 0.75, 0.8)),
        Node { position_type: PositionType::Absolute, ..default() },
        Visibility::Hidden,
    ));
}

#[derive(Component)]
struct Sunlight;

/// The light, kept apart from the camera and anchor queries.
type SunlightOnly = (With<Sunlight>, Without<Camera>, Without<Anchor>);

fn color(hex: &str) -> Color {
    Srgba::hex(hex).unwrap_or_else(|e| panic!("color {hex}: {e:?}")).into()
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let system = build_system(&SystemSpec::from_json(SYSTEM));
    let step_seconds = suggested_step_seconds(&system.bodies, 256.0);
    let mut ephemeris = Ephemeris::new(&system, EphemerisOptions { step_seconds, chunk_steps: 2048 });
    ephemeris.extend_to(step_seconds);
    let mut tree = FrameTree::new();

    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(96, 48));
    let marker = meshes.add(Sphere::new(1.0).mesh().uv(16, 8));
    let meridian = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.2, 0.2), unlit: true, ..default() });
    let pole = materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.6, 1.0), unlit: true, ..default() });

    let mut targets = Vec::new();
    let mut sun = None;
    for body in &system.bodies {
        let (inertial, surface) = tree.add_body(BodyId(body.index), body.rotation);
        let is_star = body.parent_index.is_none();
        if is_star {
            sun = Some(inertial);
        }
        let material = materials.add(StandardMaterial {
            base_color: color(&body.color),
            unlit: is_star,
            perceptual_roughness: 0.9,
            ..default()
        });
        let radius = body.radius_meters as f32;
        let anchor = commands
            .spawn((
                Name::new(body.name.clone()),
                Mesh3d(sphere.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(radius)),
                Anchor { frame: surface, position: DVec3::ZERO, orientation: DQuat::IDENTITY, radius: body.radius_meters },
            ))
            .with_children(|body| {
                // In the unit sphere's space: prime meridian on the equator (+x), north pole (+z).
                body.spawn((Mesh3d(marker.clone()), MeshMaterial3d(meridian.clone()),
                    Transform::from_xyz(1.0, 0.0, 0.0).with_scale(Vec3::splat(0.03))));
                body.spawn((Mesh3d(marker.clone()), MeshMaterial3d(pole.clone()),
                    Transform::from_xyz(0.0, 0.0, 1.0).with_scale(Vec3::splat(0.03))));
            })
            .id();
        spawn_label(&mut commands, anchor, &body.name);
        targets.push(Target { name: body.name.clone(), inertial, surface, radius: body.radius_meters });
    }

    // The probe: fixed on the home planet's surface, z up, x east.
    let home = system.bodies.iter().find(|b| b.id == HOME).unwrap_or_else(|| panic!("no body {HOME}"));
    let home_surface = targets[home.index].surface;
    let (lat, lon) = (PROBE_LATITUDE.to_radians(), PROBE_LONGITUDE.to_radians());
    let up = DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
    let east = DVec3::Z.cross(up).normalize();
    let north = up.cross(east);
    let turn = DQuat::from_mat3(&glam::DMat3::from_cols(east, north, up));
    let probe = tree.add_fixed(home_surface, Motion::fixed(up * (home.radius_meters + PROBE_SIZE / 2.0), turn));
    let probe_material = materials.add(StandardMaterial { base_color: Color::srgb(0.9, 0.9, 0.95), ..default() });
    let accent = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.55, 0.1), ..default() });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let anchor = commands
        .spawn((
            Name::new("probe"),
            Mesh3d(cube.clone()),
            MeshMaterial3d(probe_material),
            Transform::from_scale(Vec3::splat(PROBE_SIZE as f32)),
            Anchor { frame: probe, position: DVec3::ZERO, orientation: DQuat::IDENTITY, radius: PROBE_SIZE * 0.87 },
        ))
        .with_children(|p| {
            // A 1 m cube 1 m beyond the east face: any wobble shows against the big one.
            p.spawn((Mesh3d(cube), MeshMaterial3d(accent), Transform::from_xyz(0.65, 0.0, 0.0).with_scale(Vec3::splat(0.1))));
        })
        .id();
    spawn_label(&mut commands, anchor, "probe");
    targets.push(Target { name: "probe".into(), inertial: probe, surface: probe, radius: PROBE_SIZE * 0.87 });

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 1e15, ..default() }),
        Transform::default(),
    ));
    commands.spawn((
        Sunlight,
        DirectionalLight { illuminance: light_consts::lux::AMBIENT_DAYLIGHT, shadow_maps_enabled: false, ..default() },
        Transform::default(),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(10), left: px(10), ..default() },
    ));

    let start = StartView::from_args();
    let target = match &start.focus {
        Some(name) => targets
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("--focus {name}: no such target")),
        None => targets.len() - 1,
    };
    let view = View {
        target,
        corotate: start.inertial.not(),
        distance: start.distance.unwrap_or(40.0),
        yaw: start.yaw.unwrap_or(-2.2),
        pitch: start.pitch.unwrap_or(0.35),
    };
    commands.insert_resource(Sim {
        tree,
        ephemeris,
        time: 0.0,
        warp: start.warp,
        paused: false,
        targets,
        sun: sun.expect("the system has a star"),
    });
    commands.insert_resource(view);
}

/// The view to start in: `--focus NAME --distance M --yaw RAD --pitch RAD --warp INDEX --inertial`.
#[derive(Default)]
struct StartView {
    focus: Option<String>,
    distance: Option<f64>,
    yaw: Option<f64>,
    pitch: Option<f64>,
    warp: usize,
    inertial: bool,
}

impl StartView {
    fn from_args() -> Self {
        let mut start = Self::default();
        let mut args = std::env::args().skip(1);
        let number = |flag: &str, value: Option<String>| -> f64 {
            let value = value.unwrap_or_else(|| panic!("{flag} needs a value"));
            value.parse().unwrap_or_else(|e| panic!("{flag} {value}: {e}"))
        };
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--focus" => start.focus = Some(args.next().expect("--focus needs a name")),
                "--distance" => start.distance = Some(number("--distance", args.next())),
                "--yaw" => start.yaw = Some(number("--yaw", args.next())),
                "--pitch" => start.pitch = Some(number("--pitch", args.next())),
                "--warp" => {
                    let index = number("--warp", args.next()) as usize;
                    assert!(index < WARPS.len(), "--warp {index}: at most {}", WARPS.len() - 1);
                    start.warp = index;
                }
                "--inertial" => start.inertial = true,
                other => panic!("unknown argument {other}"),
            }
        }
        start
    }
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut sim: ResMut<Sim>,
    mut view: ResMut<View>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        let count = sim.targets.len();
        let step = if keys.pressed(KeyCode::ShiftLeft) { count - 1 } else { 1 };
        view.target = (view.target + step) % count;
        let target = &sim.targets[view.target];
        view.distance = target.radius * 4.0;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        view.corotate = !view.corotate;
    }
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::Period) {
        sim.warp = (sim.warp + 1).min(WARPS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Comma) {
        sim.warp = sim.warp.saturating_sub(1);
    }
    if buttons.pressed(MouseButton::Left) {
        view.yaw -= f64::from(motion.delta.x) * 0.005;
        view.pitch = (view.pitch + f64::from(motion.delta.y) * 0.005).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => f64::from(scroll.delta.y),
        MouseScrollUnit::Pixel => f64::from(scroll.delta.y) / 40.0,
    };
    if lines != 0.0 {
        // Zoom on the altitude, so the same wheel turn means as much near the ground as far out.
        let floor = view.min_distance(&sim);
        let altitude = (view.distance - floor).max(1.0) * 1.2_f64.powf(-lines);
        view.distance = (floor + altitude).min(MAX_DISTANCE);
    }
    let floor = view.min_distance(&sim);
    view.distance = view.distance.clamp(floor, MAX_DISTANCE);
}

fn advance(time: Res<Time>, mut sim: ResMut<Sim>) {
    if sim.paused {
        return;
    }
    let dt = f64::from(time.delta_secs()) * WARPS[sim.warp];
    sim.time += dt;
    let t = sim.time;
    sim.ephemeris.extend_to(t);
    sim.ephemeris.forget_before(t - 86_400.0);
}

fn place(
    sim: Res<Sim>,
    view: Res<View>,
    mut anchors: Query<(&Anchor, &mut Transform), Without<Camera>>,
    mut camera: Single<(&mut Transform, &mut Projection), With<Camera>>,
    mut light: Single<&mut Transform, SunlightOnly>,
    mut hud: Single<&mut Text, With<Hud>>,
) {
    let snapshot = sim.tree.at(sim.time, &sim.ephemeris);
    let frame = view.frame(&sim);
    let eye = view.eye();

    // Everything relative to the camera, which sits at the origin: f64 until this subtraction.
    let mut nearest_surface = f64::INFINITY;
    for (anchor, mut transform) in &mut anchors {
        let to_view = snapshot.transform(anchor.frame, frame);
        let position = to_view.apply_point(anchor.position) - eye;
        transform.translation = position.as_vec3();
        transform.rotation = (to_view.rotation() * anchor.orientation).as_quat();
        nearest_surface = nearest_surface.min(position.length() - anchor.radius);
    }

    let (camera_transform, projection) = &mut *camera;
    let up = if view.pitch.abs() > 1.5 { DVec3::X } else { DVec3::Z };
    **camera_transform = Transform::default().looking_to((-eye).as_vec3(), up.as_vec3());
    if let Projection::Perspective(perspective) = &mut **projection {
        perspective.near = (nearest_surface * 0.5).clamp(0.05, 1e7) as f32;
    }

    let sun = snapshot.transform(sim.sun, frame).apply_point(DVec3::ZERO);
    let toward_focus = (-sun).normalize_or(DVec3::NEG_Z);
    **light = Transform::default().looking_to(toward_focus.as_vec3(), Vec3::Z);

    let target = &sim.targets[view.target];
    let days = sim.time / 86_400.0;
    hud.0 = format!(
        "T+ {:.0} d {:02}:{:02}:{:02}   warp {}x{}\n\
         focus {} ({})   distance {}   altitude {}\n\
         near plane {}\n\
         Tab / Shift+Tab focus | drag orbit | wheel zoom | C co-rotate | , . warp | Space pause",
        days.floor(),
        (sim.time % 86_400.0 / 3_600.0).floor(),
        (sim.time % 3_600.0 / 60.0).floor(),
        (sim.time % 60.0).floor(),
        WARPS[sim.warp],
        if sim.paused { " (paused)" } else { "" },
        target.name,
        if view.corotate { "turning with it" } else { "inertial" },
        distance(view.distance),
        distance(view.distance - target.radius),
        distance((nearest_surface * 0.5).clamp(0.05, 1e7)),
    );
}

fn distance(m: f64) -> String {
    const AU: f64 = 1.495_978_707e11;
    if m >= 0.01 * AU {
        format!("{:.3} AU", m / AU)
    } else if m >= 1e4 {
        format!("{:.1} km", m / 1e3)
    } else {
        format!("{m:.2} m")
    }
}


/// Moves each label to its anchor's place on screen; the "+" marks the anchor itself.
fn labels(
    camera: Single<(&Camera, &Transform)>,
    anchors: Query<&Transform, With<Anchor>>,
    mut labels: Query<(&Label, &mut Node, &mut Visibility)>,
) {
    let (camera, camera_transform) = *camera;
    let camera_transform = GlobalTransform::from(*camera_transform);
    for (label, mut node, mut visibility) in &mut labels {
        let anchor = anchors.get(label.0).expect("a label's anchor exists");
        match camera.world_to_viewport(&camera_transform, anchor.translation) {
            Ok(at) => {
                // Centre the "+" (about 4 by 8 px at this size) on the anchor.
                node.left = px(at.x - 4.0);
                node.top = px(at.y - 8.0);
                *visibility = Visibility::Visible;
            }
            Err(_) => *visibility = Visibility::Hidden,
        }
    }
}

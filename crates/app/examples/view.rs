//! lab/view's page: one view from the vessel out to the whole Sol system. Zooming out from the
//! vessel fades the map in (orbits, the vessel's path, labels), then turns the camera's up to the
//! planet's north and lets go of the ground's spin. `--view split` is KSP's two views instead (M
//! switches flight and map). `--altitude KM` sets the start orbit about Aurelia (100 km).
//!
//! Drag: orbit the camera | wheel: zoom | Tab / Shift+Tab: focus (map only in split) | click a
//! label to focus | M: flight / map (split) | F: path frame inertial / surface | Space: pause |
//! `,` `.`: warp | Shift / Ctrl: throttle, Z full, X cut | 1–7: prograde, retrograde, normal,
//! antinormal, radial out, radial in, hold.
//!
//! Drawn in Aurelia's body-fixed axes with the camera at the origin: Aurelia's terrain tiles need
//! no turning, and everything else (f64 in the ecliptic) is turned and moved each frame.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use glam::DVec3;
use std::sync::Arc;
use void_app::tiles::{Tile, TileField};
use void_landing::{ContactWorldOptions, landing_lod_options, level_for_tile_size};
use void_lod::LodView;
use void_orbit::{
    AttitudeMode, CelestialBody, EngineSpec, Simulation, SimulationOptions, StartPlane, SystemSpec,
    Tolerances, VesselStartSpec, body_orientation, osculating_orbit,
};
use void_terrain::{HillsOptions, Terrain, TerrainConfig};
use void_view::{
    FocusGeometry, FocusKind, LabelKind, MapFrame, MapOrbits, MapPath, OrbitCamera, PathFrameKind,
    PlottingFrame, ViewMode, ViewState, camera_spin, frame_to_ecliptic, map_labels, view_state,
};

const SYSTEM: &str = include_str!("../../orbit/systems/sol.json");
const HOME_BODY: &str = "aurelia";
const WARPS: [f64; 6] = [1.0, 10.0, 100.0, 1e3, 1e4, 1e5];
const MAX_VESSEL_STEPS_PER_FRAME: u64 = 20_000;
const MAX_PREDICTION_STEPS_PER_FRAME: u64 = 4_000;
const THROTTLE_RATE_PER_SECOND: f64 = 0.5;
const PATH_COLOR: &str = "#4fc8ff";
const LABEL_HEIGHT: f32 = 13.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "void | view".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb_u8(0x03, 0x04, 0x0a)))
        .insert_resource(GlobalAmbientLight {
            brightness: 60.0,
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
struct Lab {
    sim: Simulation,
    mode: ViewMode,
    map_on: bool,
    path_frame: PathFrameKind,
    focus: Focus,
    camera: OrbitCamera,
    state: Option<ViewState>,
    warp: usize,
    paused: bool,
    home: usize,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    orbits: MapOrbits,
    path: MapPath,
    started: std::time::Instant,
    /// This frame's camera position (ecliptic, barycentric) and the render axes: Aurelia's
    /// body-fixed axes at the current time.
    eye: DVec3,
    axes: [DVec3; 3],
    spin: (usize, f64),
    vessel: DVec3,
    origin: DVec3,
    reference: usize,
}

impl Lab {
    fn bodies(&self) -> &[CelestialBody] {
        &self.sim.system.bodies
    }

    /// An ecliptic vector in render axes (Aurelia's body-fixed axes).
    fn render(&self, v: DVec3) -> Vec3 {
        DVec3::new(
            v.dot(self.axes[0]),
            v.dot(self.axes[1]),
            v.dot(self.axes[2]),
        )
        .as_vec3()
    }

    /// A barycentric point relative to the camera, in render axes.
    fn place(&self, p: DVec3) -> Vec3 {
        self.render(p - self.eye)
    }

    fn set_focus(&mut self, next: Focus) {
        if self.mode == ViewMode::Split && !self.map_on && next != Focus::Vessel {
            return;
        }
        self.focus = next;
        self.camera.distance = match next {
            Focus::Vessel if self.map_on => self.camera.distance.max(1e6),
            Focus::Vessel => 30.0,
            Focus::Body(i) => self.bodies()[i].radius_meters * 4.0,
        };
    }

    fn focus_name(&self) -> String {
        match self.focus {
            Focus::Vessel => "vessel".into(),
            Focus::Body(i) => self.bodies()[i].name.clone(),
        }
    }

    /// The focus's geometry, position and reference body.
    fn focus_geometry(&self, vessel: DVec3) -> (FocusGeometry, DVec3, usize) {
        match self.focus {
            Focus::Vessel => {
                let reference = self.sim.navigation_reference();
                let body = &self.bodies()[reference];
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
                let body = &self.bodies()[i];
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
}

#[derive(Resource)]
struct Ground(TileField);

#[derive(Component)]
struct BodySphere(usize);

#[derive(Component)]
struct Vessel;

#[derive(Component)]
struct Plume;

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Hud;

/// A map label: what it names, and its text child.
#[derive(Component)]
struct Label {
    kind: LabelKind,
    /// The apsis slot (0 or 1) for apsis labels.
    slot: usize,
    text: Entity,
}

fn color(hex: &str) -> Color {
    Srgba::hex(hex.trim_start_matches('#'))
        .map(Color::from)
        .unwrap_or(Color::WHITE)
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mode = match argument("--view").as_deref() {
        None | Some("single") => ViewMode::Single,
        Some("split") => ViewMode::Split,
        Some(other) => panic!("--view must be single or split, got {other}"),
    };
    let altitude_km: f64 = argument("--altitude").map_or(100.0, |a| {
        a.parse()
            .ok()
            .filter(|v: &f64| *v > 0.0)
            .unwrap_or_else(|| panic!("--altitude must be a positive number of km, got {a}"))
    });
    let sim = Simulation::new(SimulationOptions {
        system: SystemSpec::from_json(SYSTEM),
        steps_per_orbit: 256.0,
        tolerances: Tolerances {
            position_meters: 1e-4,
            velocity_meters_per_second: 1e-7,
        },
        vessel_start: VesselStartSpec {
            home_body_id: HOME_BODY.into(),
            altitude_meters: altitude_km * 1000.0,
            plane: StartPlane::Equatorial {
                inclination_radians: 0.0,
            },
        },
        // The orbit lab's chemical stage: 250 kN, Isp 350 s, 10 t dry + 30 t propellant.
        engine: EngineSpec {
            thrust_newtons: 250e3,
            specific_impulse_seconds: 350.0,
            dry_mass_kg: 10e3,
            fuel_mass_kg: 30e3,
        },
        retention_seconds: 86_400.0,
        prediction_horizon_seconds: 3.0 * 3600.0,
        plan_coast_seconds: 86_400.0,
    });
    let home = sim.body_index(HOME_BODY);
    let bodies = sim.system.bodies.clone();
    let n = bodies.len();

    // The home planet is the landing lab's terrain (Earth-size hills, as its terra planet) streamed
    // by lab/lod, with the landing lab's tile options.
    let radius = bodies[home].radius_meters;
    let terrain = Arc::new(Terrain::from_config(&TerrainConfig::Hills(HillsOptions {
        name: format!("{} hills", bodies[home].name),
        radius_meters: radius,
        max_height_meters: 8000.0,
        wavelength_meters: 40_000.0,
        octaves: 8,
    })));
    let contact = ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(radius, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    };
    let ground = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.insert_resource(Ground(TileField::new(
        landing_lod_options(&terrain, &contact),
        Some(terrain),
        ground,
    )));

    commands.insert_resource(Lab {
        orbits: MapOrbits::new(&bodies),
        path: MapPath::new(),
        sim,
        mode,
        map_on: false,
        path_frame: PathFrameKind::Inertial,
        focus: Focus::Vessel,
        camera: OrbitCamera::new(DVec3::new(0.3, -1.0, 0.4).normalize(), 30.0),
        state: None,
        warp: 0,
        paused: false,
        home,
        positions: vec![DVec3::ZERO; n],
        velocities: vec![DVec3::ZERO; n],
        started: std::time::Instant::now(),
        eye: DVec3::ZERO,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        spin: (home, 0.0),
        vessel: DVec3::ZERO,
        origin: DVec3::ZERO,
        reference: home,
    });

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 55f32.to_radians(),
            far: 1e14,
            ..default()
        }),
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 8000.0,
            color: Color::srgb_u8(0xff, 0xf4, 0xe0),
            ..default()
        },
        Transform::default(),
    ));

    // Bodies other than Aurelia are plain spheres; the star is unlit.
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    for body in &bodies {
        if body.index == home {
            continue;
        }
        let material = if body.parent_index.is_none() {
            materials.add(StandardMaterial {
                base_color: color(&body.color),
                unlit: true,
                ..default()
            })
        } else {
            materials.add(StandardMaterial {
                base_color: color(&body.color),
                perceptual_roughness: 0.9,
                ..default()
            })
        };
        commands.spawn((
            BodySphere(body.index),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }

    // The lab's vessel: hull, nose, bell and plume, about 6 m along its thrust axis (+Y).
    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0xe8, 0xe4, 0xd8),
        metallic: 0.3,
        perceptual_roughness: 0.5,
        ..default()
    });
    let bell = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x77, 0x89, 0x95),
        metallic: 0.85,
        perceptual_roughness: 0.3,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let plume = materials.add(StandardMaterial {
        base_color: Color::srgba_u8(0xff, 0x9a, 0x4a, 150),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands
        .spawn((Vessel, Transform::default(), Visibility::default()))
        .with_children(|v| {
            v.spawn((
                Mesh3d(meshes.add(ConicalFrustum {
                    radius_top: 1.2,
                    radius_bottom: 1.4,
                    height: 4.0,
                })),
                MeshMaterial3d(hull.clone()),
            ));
            v.spawn((
                Mesh3d(meshes.add(Cone {
                    radius: 1.2,
                    height: 1.8,
                })),
                MeshMaterial3d(hull),
                Transform::from_xyz(0.0, 2.9, 0.0),
            ));
            v.spawn((
                Mesh3d(meshes.add(ConicalFrustum {
                    radius_top: 0.45,
                    radius_bottom: 0.9,
                    height: 1.0,
                })),
                MeshMaterial3d(bell),
                Transform::from_xyz(0.0, -2.5, 0.0),
            ));
            v.spawn((
                Plume,
                Mesh3d(meshes.add(Cone {
                    radius: 0.8,
                    height: 5.0,
                })),
                MeshMaterial3d(plume),
                Transform::from_xyz(0.0, -5.5, 0.0)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::PI)),
            ));
        });

    // Labels: every body, the vessel and two apsides; clickable once the map is half in.
    let font = TextFont {
        font_size: FontSize::Px(12.0),
        ..default()
    };
    let mut kinds: Vec<(LabelKind, usize, String, Color)> = bodies
        .iter()
        .map(|b| {
            (
                if b.parent_index.is_none() {
                    LabelKind::Star
                } else {
                    LabelKind::Body(b.index)
                },
                0,
                b.name.clone(),
                color(&b.color),
            )
        })
        .collect();
    kinds.push((LabelKind::Vessel, 0, "Vessel".into(), color("#7dffb0")));
    kinds.push((LabelKind::Apsis, 0, String::new(), color(PATH_COLOR)));
    kinds.push((LabelKind::Apsis, 1, String::new(), color(PATH_COLOR)));
    for (kind, slot, name, dot) in kinds {
        let text = commands
            .spawn((
                Text::new(name),
                font.clone(),
                TextShadow {
                    offset: Vec2::ONE,
                    color: Color::BLACK.with_alpha(0.8),
                },
            ))
            .id();
        let marker = commands
            .spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|m| {
                m.spawn((
                    Node {
                        width: px(6),
                        height: px(6),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(dot),
                ));
            })
            .id();
        commands.entity(marker).add_child(text);
        commands.entity(marker).insert(Label { kind, slot, text });
    }

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
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    labels: Query<(&Interaction, &Label)>,
    mut lab: ResMut<Lab>,
    mut dragging: Local<bool>,
) {
    let lab = &mut *lab;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let attitude = [
        (KeyCode::Digit1, AttitudeMode::Prograde),
        (KeyCode::Digit2, AttitudeMode::Retrograde),
        (KeyCode::Digit3, AttitudeMode::Normal),
        (KeyCode::Digit4, AttitudeMode::Antinormal),
        (KeyCode::Digit5, AttitudeMode::RadialOut),
        (KeyCode::Digit6, AttitudeMode::RadialIn),
        (KeyCode::Digit7, AttitudeMode::Hold),
    ];
    for (key, mode) in attitude {
        if keys.just_pressed(key) {
            lab.sim.set_attitude(mode);
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::Period) {
        lab.warp = (lab.warp + 1).min(WARPS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Comma) {
        lab.warp = lab.warp.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::KeyZ) && lab.sim.impact.is_none() {
        lab.sim.throttle = 1.0;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        lab.sim.throttle = 0.0;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        lab.path_frame = match lab.path_frame {
            PathFrameKind::Inertial => PathFrameKind::Surface,
            PathFrameKind::Surface => PathFrameKind::Inertial,
        };
    }
    if keys.just_pressed(KeyCode::KeyM) && lab.mode == ViewMode::Split {
        lab.map_on = !lab.map_on;
        // KSP's flight camera only looks at the vessel; leaving the map returns to it.
        if !lab.map_on && lab.focus != Focus::Vessel {
            lab.focus = Focus::Vessel;
            lab.camera.distance = 30.0;
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        let mut order = vec![Focus::Vessel];
        order.extend((0..lab.bodies().len()).map(Focus::Body));
        let current = order.iter().position(|&f| f == lab.focus).unwrap_or(0);
        let step = if shift { order.len() - 1 } else { 1 };
        lab.set_focus(order[(current + step) % order.len()]);
    }
    // Throttle: Shift up, Ctrl down.
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if shift != ctrl && lab.sim.impact.is_none() && !keys.just_pressed(KeyCode::Tab) {
        let sign = if shift { 1.0 } else { -1.0 };
        lab.sim.throttle = (lab.sim.throttle
            + sign * THROTTLE_RATE_PER_SECOND * time.delta_secs_f64())
        .clamp(0.0, 1.0);
    }

    // Labels take clicks once the map is half in, and a click on one never starts a drag.
    let map_weight = lab.state.map_or(0.0, |s| s.map_weight);
    let mut over_label = false;
    for (interaction, label) in &labels {
        if *interaction == Interaction::None || map_weight <= 0.5 {
            continue;
        }
        over_label = true;
        if *interaction == Interaction::Pressed && buttons.just_pressed(MouseButton::Left) {
            match label.kind {
                LabelKind::Vessel => lab.set_focus(Focus::Vessel),
                LabelKind::Star | LabelKind::Body(_) => {
                    if let LabelKind::Body(i) = label.kind {
                        lab.set_focus(Focus::Body(i));
                    } else {
                        let star = lab.bodies().iter().position(|b| b.parent_index.is_none());
                        lab.set_focus(Focus::Body(star.unwrap()));
                    }
                }
                LabelKind::Apsis => {}
            }
        }
    }
    let any = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
    if buttons.any_just_pressed(any) && !over_label {
        *dragging = true;
    }
    if !buttons.any_pressed(any) {
        *dragging = false;
    }
    if let Some(state) = lab.state {
        if *dragging && motion.delta != Vec2::ZERO {
            lab.camera
                .drag(motion.delta.x as f64, motion.delta.y as f64, state.up);
        }
        let pixels = match scroll.unit {
            MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
            MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
        };
        if pixels != 0.0 {
            lab.camera.zoom(
                (pixels * 0.0012).exp(),
                state.min_distance,
                state.max_distance,
            );
        }
    }
}

fn simulate(time: Res<Time>, mut lab: ResMut<Lab>) {
    let lab = &mut *lab;
    let real_dt = time.delta_secs_f64();
    let before = lab.sim.time;
    if !lab.paused && real_dt > 0.0 {
        lab.sim
            .advance(real_dt * WARPS[lab.warp], MAX_VESSEL_STEPS_PER_FRAME);
    }
    lab.sim.extend_prediction(MAX_PREDICTION_STEPS_PER_FRAME);
    let t = lab.sim.time;
    lab.sim
        .ephemeris
        .states_at(t, &mut lab.positions, Some(&mut lab.velocities));

    let vessel = lab.sim.vessel_position_at(t);
    let (geometry, origin, reference) = lab.focus_geometry(vessel);
    let state = view_state(lab.mode, lab.map_on, &geometry, lab.camera.distance);
    lab.camera.distance =
        lab.camera
            .clamp_distance(lab.camera.distance, state.min_distance, state.max_distance);
    // The camera turns with the ground near it, or with the path frame on the map.
    let navigation = lab.sim.navigation_reference();
    let spin = camera_spin(&state, lab.path_frame, reference, navigation);
    if spin.1 > 0.0 && t > before {
        let spinning = &lab.bodies()[spin.0];
        let (axis, rate) = (spinning.rotation.axis(), spinning.rotation.rate());
        lab.camera.corotate(axis, rate * (t - before) * spin.1);
    }
    lab.camera.clamp_to_up(state.up);
    lab.eye = origin + lab.camera.direction * lab.camera.distance;
    lab.axes = body_orientation(&lab.bodies()[lab.home].rotation, t);
    lab.state = Some(state);
    lab.spin = spin;
    lab.vessel = vessel;
    lab.origin = origin;
    lab.reference = reference;
}

/// lab/lod's observers: the vessel (the probe) and the camera, so a camera far out sees the
/// planet's face toward it. Both in Aurelia's body-fixed frame.
fn terrain(lab: Res<Lab>, mut ground: ResMut<Ground>) {
    let centre = lab.positions[lab.home];
    let fixed = |p: DVec3| {
        let d = p - centre;
        DVec3::new(d.dot(lab.axes[0]), d.dot(lab.axes[1]), d.dot(lab.axes[2]))
    };
    ground.0.finish_builds();
    ground.0.select(&LodView {
        observer_positions: vec![fixed(lab.vessel), fixed(lab.eye)],
        camera: None,
        distance_scale: 1.0,
        horizon_culling: true,
    });
}

type CameraOnly = (
    With<Camera3d>,
    Without<Tile>,
    Without<BodySphere>,
    Without<Vessel>,
    Without<Sun>,
    Without<Plume>,
);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    mut lab: ResMut<Lab>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Transform, With<Tile>>,
    mut camera: Single<(&mut Transform, &mut Projection), CameraOnly>,
    mut spheres: Query<
        (&BodySphere, &mut Transform),
        (Without<Tile>, Without<Vessel>, Without<Sun>, Without<Plume>),
    >,
    mut vessel: Single<&mut Transform, (With<Vessel>, Without<Tile>, Without<Sun>, Without<Plume>)>,
    mut plume: Single<
        (&mut Transform, &mut Visibility),
        (With<Plume>, Without<Tile>, Without<Sun>),
    >,
    mut sun: Single<&mut Transform, (With<Sun>, Without<Tile>)>,
    mut gizmos: Gizmos,
) {
    let lab = &mut *lab;
    let Some(state) = lab.state else { return };
    let t = lab.sim.time;

    // Camera at the origin, looking at the focus, up as the view state says.
    let (camera_transform, projection) = &mut *camera;
    **camera_transform =
        Transform::default().looking_to(lab.render(-lab.camera.direction), lab.render(state.up));
    if let Projection::Perspective(p) = &mut **projection {
        p.near = (lab.camera.distance * 1e-3).max(0.05) as f32;
    }

    // Aurelia's tiles, relative to the camera in body-fixed axes.
    let centre = lab.positions[lab.home];
    let d = lab.eye - centre;
    let eye_fixed = DVec3::new(d.dot(lab.axes[0]), d.dot(lab.axes[1]), d.dot(lab.axes[2]));
    ground
        .0
        .draw(&mut commands, &mut meshes, &mut tiles, eye_fixed);

    for (body, mut transform) in &mut spheres {
        let radius = lab.bodies()[body.0].radius_meters as f32;
        *transform = Transform::from_translation(lab.place(lab.positions[body.0]))
            .with_scale(Vec3::splat(radius));
    }
    let star = lab
        .bodies()
        .iter()
        .position(|b| b.parent_index.is_none())
        .expect("a star");
    let sunward = lab.render(lab.positions[star] - lab.origin).normalize();
    **sun = Transform::default().looking_to(-sunward, Vec3::Y);

    let thrust = lab.sim.thrust_direction();
    let throttle = lab.sim.effective_throttle();
    **vessel = Transform::from_translation(lab.place(lab.vessel)).with_rotation(
        Quat::from_rotation_arc(Vec3::Y, lab.render(thrust).normalize()),
    );
    let (plume_transform, plume_visibility) = &mut *plume;
    **plume_visibility = if throttle > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    plume_transform.scale.y = (0.4 + 0.6 * throttle) as f32;

    // The map: bodies' orbits, the vessel's path, at the map weight's opacity.
    let navigation = lab.sim.navigation_reference();
    let frame = MapFrame {
        time: t,
        positions: &lab.positions,
        velocities: &lab.velocities,
        origin: lab.eye,
        vessel: lab.vessel,
        vessel_velocity: lab.sim.vessel().velocity,
        plotting: PlottingFrame {
            kind: lab.path_frame,
            reference: navigation,
        },
        wall_ms: lab.started.elapsed().as_secs_f64() * 1e3,
    };
    let bodies = lab.sim.system.bodies.clone();
    lab.orbits.update(&bodies, &frame);
    if lab.sim.impact.is_some() {
        lab.path.hide();
    } else {
        lab.path.update(
            &lab.sim.ephemeris,
            &lab.sim.prediction,
            lab.sim.prediction_generation,
            &frame,
            true,
        );
    }
    let alpha = state.map_weight as f32;
    if alpha > 0.0 {
        for body in &bodies {
            let Some(placement) = lab.orbits.placement(&bodies, body.index, &frame) else {
                continue;
            };
            let shape = &lab.orbits.shapes[body.index];
            let points = shape.points.iter().map(|&p| {
                let e = match &placement.axes {
                    Some(axes) => frame_to_ecliptic(axes, p),
                    None => p,
                };
                lab.render(placement.anchor + e)
            });
            let c = color(&body.color).with_alpha(alpha);
            if shape.closed {
                let first = shape.points[0];
                let close = std::iter::once(lab.render(placement.anchor + first));
                gizmos.linestrip(points.chain(close), c);
            } else {
                gizmos.linestrip(points, c);
            }
        }
        if lab.path.visible {
            gizmos.linestrip(
                lab.path.points.iter().map(|&p| lab.render(p)),
                color(PATH_COLOR).with_alpha(alpha),
            );
        }
    }
}

#[allow(clippy::type_complexity)]
fn labels(
    lab: Res<Lab>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut markers: Query<(&Label, &mut Node, &mut Visibility, &ComputedNode)>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<Label>>,
) {
    let Some(state) = lab.state else { return };
    let (camera, camera_transform) = *camera;
    let frame = MapFrame {
        time: lab.sim.time,
        positions: &lab.positions,
        velocities: &lab.velocities,
        origin: lab.eye,
        vessel: lab.vessel,
        vessel_velocity: lab.sim.vessel().velocity,
        plotting: PlottingFrame {
            kind: lab.path_frame,
            reference: lab.sim.navigation_reference(),
        },
        wall_ms: 0.0,
    };
    let focus = match lab.focus {
        Focus::Vessel => None,
        Focus::Body(i) => Some(i),
    };
    let apsides = lab.path.apsis_positions(&frame);
    let wanted = map_labels(lab.bodies(), &frame, focus, &apsides);
    // A label overlapping a higher-priority one keeps only its dot.
    let mut shown: Vec<(Vec2, f32)> = Vec::new();
    let mut placed: Vec<(LabelKind, usize, Vec2, bool, String)> = Vec::new();
    let mut apsis_slot = 0;
    for label in &wanted {
        let slot = if label.kind == LabelKind::Apsis {
            apsis_slot += 1;
            apsis_slot - 1
        } else {
            0
        };
        let world = lab.render(label.relative);
        // In front of the camera, and not far off screen.
        let ahead = camera_transform.forward().dot(world) > 0.0;
        let Ok(at) = camera.world_to_viewport(camera_transform, world) else {
            continue;
        };
        let size = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
        let visible = state.map_weight > 0.0
            && ahead
            && at.x > -0.1 * size.x
            && at.x < 1.1 * size.x
            && at.y > -0.1 * size.y
            && at.y < 1.1 * size.y;
        if !visible {
            continue;
        }
        let width = markers
            .iter()
            .find(|(m, ..)| m.kind == label.kind && m.slot == slot)
            .map_or(60.0, |(.., computed)| {
                computed.size().x * computed.inverse_scale_factor()
            });
        let crowded = shown.iter().any(|(p, w)| {
            (p.y - at.y).abs() < LABEL_HEIGHT
                && if at.x >= p.x {
                    at.x - p.x < *w
                } else {
                    p.x - at.x < width
                }
        });
        if !crowded {
            shown.push((at, width));
        }
        placed.push((label.kind, slot, at, crowded, label.text.clone()));
    }
    for (marker, mut node, mut visibility, _) in &mut markers {
        let found = placed
            .iter()
            .find(|(kind, slot, ..)| *kind == marker.kind && *slot == marker.slot);
        match found {
            Some((_, _, at, crowded, text)) => {
                *visibility = Visibility::Inherited;
                node.left = px(at.x - 3.0);
                node.top = px(at.y - 7.0);
                if let Ok((mut t, mut v)) = texts.get_mut(marker.text) {
                    if t.0 != *text {
                        t.0.clone_from(text);
                    }
                    *v = if *crowded {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    };
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn distance_text(m: f64) -> String {
    let a = m.abs();
    if a >= 1e9 {
        format!("{:.3} Gm", m / 1e9)
    } else if a >= 1e4 {
        format!("{:.1} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}

fn hud(lab: Res<Lab>, ground: Res<Ground>, mut text: Single<&mut Text, With<Hud>>) {
    let Some(state) = lab.state else { return };
    let sim = &lab.sim;
    let view = match (lab.mode, lab.map_on) {
        (ViewMode::Single, _) => "single view",
        (ViewMode::Split, true) => "split | MAP",
        (ViewMode::Split, false) => "split | FLIGHT",
    };
    let navigation = sim.navigation_reference();
    let reference = &lab.bodies()[navigation];
    let r = lab.vessel - lab.positions[navigation];
    let v = sim.vessel().velocity - lab.velocities[navigation];
    let osc = osculating_orbit(r, v, reference.gm);
    let focus = match lab.focus {
        Focus::Vessel => format!("vessel (reference {})", lab.bodies()[lab.reference].name),
        Focus::Body(_) => lab.focus_name(),
    };
    let apoapsis = if osc.apoapsis_radius_meters.is_finite() {
        distance_text(osc.apoapsis_radius_meters - reference.radius_meters)
    } else {
        "escape".into()
    };
    let impact = sim
        .impact
        .map(|i| format!("IMPACT on {}", lab.bodies()[i.body_index].name))
        .unwrap_or_default();
    text.0 = format!(
        "VIEW LAB  {view}{}\n\
         T+ {:.0} s   warp {}x\n\n\
         focus      {focus}\n\
         distance   {}  [{} .. {}]\n\
         map        {:.0}%\n\
         up         {:.0}% toward north\n\
         co-rotate  {:.0}% of {}'s spin\n\
         path       {} {}\n\n\
         vessel     about {}\n\
         altitude   {}\n\
         speed      {:.1} m/s\n\
         Pe / Ap    {} / {apoapsis}\n\
         throttle   {:.0}%  {}  fuel {:.2} t\n\
         {impact}\n\n\
         tiles      {} drawn, L{}-L{}, {} building\n\n\
         drag: orbit | wheel: zoom | Tab: focus | click a label | F: path frame{}\n\
         Space: pause | , .: warp | Shift/Ctrl: throttle | Z full | X cut | 1-7: attitude",
        if lab.paused { "  PAUSED" } else { "" },
        sim.time,
        WARPS[lab.warp],
        distance_text(lab.camera.distance),
        distance_text(state.min_distance),
        distance_text(state.max_distance),
        state.map_weight * 100.0,
        state.up_weight * 100.0,
        lab.spin.1 * 100.0,
        lab.bodies()[lab.spin.0].name,
        if lab.path_frame == PathFrameKind::Surface {
            "turning with"
        } else {
            "inertial about"
        },
        reference.name,
        reference.name,
        distance_text(r.length() - reference.radius_meters),
        v.length(),
        distance_text(osc.periapsis_radius_meters - reference.radius_meters),
        sim.effective_throttle() * 100.0,
        sim.attitude_mode().label(),
        sim.fuel_kg() / 1000.0,
        ground.0.drawn_count(),
        ground.0.levels.0,
        ground.0.levels.1,
        ground.0.building_count(),
        if lab.mode == ViewMode::Split {
            " | M: flight / map"
        } else {
            ""
        },
    );
}

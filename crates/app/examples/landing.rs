//! The landing lab's page in Bevy: the two-stage demo rocket on a rotating planet
//! (`--planet pebble|luna|terra|aurelia|aurelia-fast`, Pebble by default), the LOD terrain drawn
//! around every part (at the collision level near them, so what is drawn is what is collided
//! with), and the engine-off coast forecast in cyan. Drawn in the planet's body-fixed frame,
//! relative to the camera.
//!
//! Space: ignite the booster, then separate and ignite the upper stage | Shift / Ctrl: throttle ·
//! W / S pitch, A / D yaw, Q / E roll | drag to orbit the camera, wheel to zoom | 1 / 2 / 3: time
//! rate 1×, 5×, 20× | P: pause | R: reset | B: wireframe.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::WgpuFeatures;
use bevy::render::settings::WgpuSettings;
use glam::{DQuat, DVec3};
use void_app::tiles::{Tile, TileField, anchor};
use void_landing::{
    BodyShape, CoastPrediction, DemoRocket, LanderControl, LandingPlanet, PartJointRocket,
    PhysicsMode, Piece, RocketPart, SimpleShape, demo_rocket, landing_lod_options, planet_by_id,
    planet_ephemeris, predict_coast,
};
use void_lod::LodView;
use void_orbit::Ephemeris;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        features: WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "void · landing".into(),
                        ..default()
                    }),
                    ..default()
                }),
            WireframePlugin::default(),
        ))
        .insert_resource(ClearColor(Color::srgb(0.027, 0.063, 0.106)))
        .insert_resource(WireframeConfig {
            global: false,
            default_color: Color::WHITE,
            ..default()
        })
        .insert_resource(GlobalAmbientLight {
            brightness: 1500.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, physics, terrain, draw).chain())
        .run();
}

#[derive(Resource)]
struct Sim {
    planet: LandingPlanet,
    ephemeris: Ephemeris,
    body_index: usize,
    demo: DemoRocket,
    rocket: PartJointRocket,
    /// 0 on the pad, 1 booster lit, 2 upper stage lit.
    stage: u8,
    engine_armed: bool,
    throttle_percent: f64,
    rate: f64,
    paused: bool,
    prediction: Option<CoastPrediction>,
    prediction_at: f64,
    control: LanderControl,
}

#[derive(Resource)]
struct Terrain(TileField);

#[derive(Resource)]
struct Orbit {
    distance: f64,
    azimuth: f64,
    elevation: f64,
    max_distance: f64,
}

#[derive(Component)]
struct Part(RocketPart);

/// An engine flame, under the part's engine bell.
#[derive(Component)]
struct Flame(RocketPart);

#[derive(Component)]
struct Hud;

/// Rocket, flame and camera transforms, kept apart from the tiles'.
type NotTile = (Without<Tile>, Without<Camera>);
type FlameOnly = (Without<Part>, Without<Tile>, Without<Camera>);
type CameraOnly = (With<Camera>, Without<Tile>);

fn planet_argument() -> String {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => "pebble".into(),
        [flag, id] if flag == "--planet" => id.clone(),
        other => panic!(
            "unknown arguments {other:?}; use --planet pebble|luna|terra|aurelia|aurelia-fast"
        ),
    }
}

fn new_rocket(
    sim_planet: &LandingPlanet,
    ephemeris: &mut Ephemeris,
    body_index: usize,
    demo: &DemoRocket,
) -> PartJointRocket {
    PartJointRocket::landed(
        ephemeris,
        body_index,
        sim_planet.terrain.clone(),
        demo.full.clone(),
        demo.upper.clone(),
        demo.booster.clone(),
        demo.options,
        demo.launch_site,
    )
}

/// Bevy meshes for a compound collider: cylinders, cones and boxes on Y, as Rapier's.
fn spawn_shape(
    parent: &mut ChildSpawnerCommands,
    shape: &BodyShape,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
) {
    let pieces: Vec<Piece> = match shape {
        BodyShape::Compound(pieces) => pieces.clone(),
        BodyShape::Simple(s) => vec![Piece {
            shape: *s,
            position: DVec3::ZERO,
            rotation: None,
            mass: None,
        }],
    };
    for piece in pieces {
        let mesh = match piece.shape {
            SimpleShape::Cylinder {
                radius,
                half_height,
            } => meshes.add(Cylinder::new(radius as f32, 2.0 * half_height as f32)),
            SimpleShape::Cone {
                radius,
                half_height,
            } => meshes.add(Cone {
                radius: radius as f32,
                height: 2.0 * half_height as f32,
            }),
            SimpleShape::Box { half_extents: h } => meshes.add(Cuboid::new(
                2.0 * h.x as f32,
                2.0 * h.y as f32,
                2.0 * h.z as f32,
            )),
            SimpleShape::Ball { radius } => meshes.add(Sphere::new(radius as f32)),
        };
        let rotation = piece.rotation.unwrap_or(DQuat::IDENTITY).as_quat();
        parent.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(piece.position.as_vec3()).with_rotation(rotation),
        ));
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let planet = planet_by_id(&planet_argument());
    let (mut ephemeris, body_index) = planet_ephemeris(&planet);
    let demo = demo_rocket(&planet.terrain);
    let rocket = new_rocket(&planet, &mut ephemeris, body_index, &demo);

    let ground = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    let lod = landing_lod_options(&planet.terrain, &demo.options.contact);
    commands.insert_resource(Terrain(TileField::new(
        lod,
        Some(planet.terrain.clone()),
        ground,
    )));

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
                // The flame's tip points down out of the nozzle; its length follows the throttle.
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

    let max_distance = (3.0 * planet.terrain.radius_meters).max(300_000.0);
    commands.insert_resource(Orbit {
        distance: 45.0,
        azimuth: 0.4,
        elevation: 0.3,
        max_distance,
    });
    commands.insert_resource(Sim {
        planet,
        ephemeris,
        body_index,
        demo,
        rocket,
        stage: 0,
        engine_armed: false,
        throttle_percent: 0.0,
        rate: 1.0,
        paused: false,
        prediction: None,
        prediction_at: f64::NEG_INFINITY,
        control: LanderControl::default(),
    });
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 58f32.to_radians(),
            far: 1e10,
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: light_consts::lux::AMBIENT_DAYLIGHT,
            ..default()
        },
        // The lab's sun at render (3, 7, 4), fixed in the planet: body-fixed (3, -4, 7).
        Transform::default().looking_to(-Vec3::new(3.0, -4.0, 7.0), Vec3::X),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            ..default()
        },
    ));
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    f64::from(u8::from(keys.pressed(positive))) - f64::from(u8::from(keys.pressed(negative)))
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut sim: ResMut<Sim>,
    mut orbit: ResMut<Orbit>,
    mut wireframe: ResMut<WireframeConfig>,
) {
    let sim = &mut *sim;
    if keys.just_pressed(KeyCode::Space) {
        if sim.stage == 0 {
            sim.stage = 1;
            sim.engine_armed = true;
        } else if sim.stage == 1 {
            sim.rocket.separate(&sim.ephemeris);
            sim.stage = 2;
            sim.prediction_at = f64::NEG_INFINITY;
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        sim.rocket = new_rocket(&sim.planet, &mut sim.ephemeris, sim.body_index, &sim.demo);
        (
            sim.stage,
            sim.engine_armed,
            sim.throttle_percent,
            sim.paused,
        ) = (0, false, 0.0, false);
        (sim.prediction, sim.prediction_at) = (None, f64::NEG_INFINITY);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        wireframe.global = !wireframe.global;
    }
    for (key, rate) in [
        (KeyCode::Digit1, 1.0),
        (KeyCode::Digit2, 5.0),
        (KeyCode::Digit3, 20.0),
    ] {
        if keys.just_pressed(key) {
            sim.rate = rate;
        }
    }
    let dt = f64::from(time.delta_secs());
    let throttle = f64::from(u8::from(
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    )) - f64::from(u8::from(
        keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
    ));
    sim.throttle_percent = (sim.throttle_percent + throttle * dt * 50.0).clamp(0.0, 100.0);
    sim.control = LanderControl {
        throttle: if sim.engine_armed {
            sim.throttle_percent / 100.0
        } else {
            0.0
        },
        up: 1.0,
        turn: Some(DVec3::new(
            axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
            axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        )),
        ..Default::default()
    };

    if buttons.pressed(MouseButton::Left) {
        orbit.azimuth -= f64::from(motion.delta.x) * 0.006;
        orbit.elevation = (orbit.elevation + f64::from(motion.delta.y) * 0.006).clamp(-1.3, 1.3);
    }
    // The lab's wheel: × exp(pixels × 0.001), a line counting as a browser's 100 px notch.
    let pixels = match scroll.unit {
        MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
        MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
    };
    orbit.distance = (orbit.distance * (pixels * 0.001).exp()).clamp(10.0, orbit.max_distance);
}

fn physics(time: Res<Time>, mut sim: ResMut<Sim>) {
    let sim = &mut *sim;
    if sim.paused {
        return;
    }
    // At most a tenth of a second of real time per frame, so a stall cannot run ahead.
    let dt = f64::from(time.delta_secs()).min(0.1) * sim.rate;
    let control = sim.control.clone();
    sim.rocket.advance(&mut sim.ephemeris, dt, &control, None);
    if sim.stage == 1 && sim.rocket.separated() {
        sim.stage = 2;
    }
    // The engine-off forecast, refreshed every 2 s of simulated time while clear of the ground.
    let spec_height = sim.rocket.spec().half_extents.y;
    if sim.rocket.clearance(&sim.ephemeris) < spec_height {
        sim.prediction = None;
    } else if sim.rocket.time() - sim.prediction_at >= 2.0 {
        sim.prediction_at = sim.rocket.time();
        let state = sim.rocket.body_fixed_state(&sim.ephemeris);
        let (frame, terrain, tolerances, t, mass) = (
            sim.rocket.frame.clone(),
            sim.planet.terrain.clone(),
            sim.demo.options.tolerances,
            sim.rocket.time(),
            sim.rocket.mass_kg(),
        );
        sim.prediction = Some(predict_coast(
            &mut sim.ephemeris,
            &frame,
            &terrain,
            tolerances,
            t,
            state,
            mass,
            600.0,
        ));
    }
}

fn live_parts(sim: &Sim) -> Vec<RocketPart> {
    [RocketPart::Upper, RocketPart::Booster]
        .into_iter()
        .filter(|&w| sim.rocket.part_mode(w) != PhysicsMode::Destroyed)
        .collect()
}

fn terrain(sim: Res<Sim>, mut field: ResMut<Terrain>) {
    field.0.finish_builds();
    // As the landing lab's terrain: the rocket parts are the observers, no camera, horizon culling on.
    let observers = live_parts(&sim)
        .iter()
        .map(|&w| sim.rocket.part_state(&sim.ephemeris, w).position)
        .collect();
    field.0.select(&LodView {
        observer_positions: observers,
        camera: None,
        distance_scale: 1.0,
        horizon_culling: true,
    });
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    sim: Res<Sim>,
    orbit: Res<Orbit>,
    mut field: ResMut<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Transform, With<Tile>>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), NotTile>,
    mut flames: Query<(&Flame, &mut Transform, &mut Visibility), FlameOnly>,
    mut camera: Single<(&mut Transform, &mut Projection), CameraOnly>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut gizmos: Gizmos,
) {
    let eph = &sim.ephemeris;
    let rocket = &sim.rocket;
    let centre = rocket.part_state(eph, RocketPart::Upper).position;
    // The camera orbits the upper stage in its local east-north-up frame.
    let up = centre.normalize();
    let east = DVec3::Z.cross(up).try_normalize().unwrap_or(DVec3::X);
    let north = up.cross(east);
    let (d, az, el) = (orbit.distance, orbit.azimuth, orbit.elevation);
    let eye = centre
        + up * (el.sin() * d)
        + east * (el.cos() * az.cos() * d)
        + north * (el.cos() * az.sin() * d);
    let (camera_transform, projection) = &mut *camera;
    **camera_transform = Transform::default().looking_to((centre - eye).as_vec3(), up.as_vec3());
    if let Projection::Perspective(p) = &mut **projection {
        p.near = (d * 0.001).max(0.1) as f32;
    }

    field.0.draw(&mut commands, &mut meshes, &mut tiles, eye);

    for (part, mut transform, mut visibility) in &mut parts {
        let alive = rocket.part_mode(part.0) != PhysicsMode::Destroyed;
        *visibility = if alive {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if alive {
            *transform = anchor(rocket.part_state(eph, part.0).position, eye)
                .with_rotation(rocket.part_orientation(part.0).as_quat());
        }
    }
    let firing = if !sim.paused && sim.engine_armed && rocket.fuel_kg() > 0.0 {
        sim.control.throttle
    } else {
        0.0
    };
    let lit = if sim.stage == 2 {
        RocketPart::Upper
    } else {
        RocketPart::Booster
    };
    for (flame, mut transform, mut visibility) in &mut flames {
        let on = sim.stage > 0 && flame.0 == lit && firing > 0.0;
        *visibility = if on {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        let length = (0.5 + 3.0 * firing) as f32;
        let nozzle = if flame.0 == RocketPart::Upper {
            -1.05
        } else {
            -1.4
        };
        transform.translation.y = nozzle - length / 2.0;
        transform.scale = Vec3::new(1.0, length, 1.0);
    }

    if let Some(prediction) = &sim.prediction {
        gizmos.linestrip(
            prediction.points.iter().map(|(_, p)| (*p - eye).as_vec3()),
            Color::srgb(0.35, 0.91, 0.93),
        );
    }

    let state = rocket.body_fixed_state(eph);
    let r = state.position.length();
    let vertical = state.velocity.dot(state.position) / r;
    let mode = |m: PhysicsMode| match m {
        PhysicsMode::Flight => "orbit",
        PhysicsMode::Contact => "contact",
        PhysicsMode::Destroyed => "destroyed",
    };
    let worlds = rocket.contact_worlds();
    let tiles_loaded: usize = worlds.iter().map(|w| w.loaded_tile_count()).sum();
    hud.0 = format!(
        "{}\n\
         T+{:.1} s   rate {}x{}   stage {}   throttle {:.0}%{}\n\
         height AGL {}   coast impact {}   surface speed {:.2} m/s   vertical {:+.2} m/s\n\
         upper {} | {:.0} kg fuel | {:.0} m/s   booster {} | {:.0} kg fuel | {:.0} m/s\n\
         {} contact world(s), {} collision tiles   terrain: {} tiles drawn (L{}-L{}), {} building   crashes {}\n\
         Space stage | Shift/Ctrl throttle | W/S pitch | A/D yaw | Q/E roll | drag orbit | wheel zoom | 1/2/3 rate | P pause | R reset | B wireframe",
        sim.planet.label.replace('·', "|"),
        rocket.time(),
        sim.rate,
        if sim.paused { " (paused)" } else { "" },
        sim.stage,
        sim.throttle_percent,
        if sim.engine_armed {
            ""
        } else {
            " (Space to ignite)"
        },
        meters((rocket.clearance(eph) - rocket.spec().half_extents.y).max(0.0)),
        sim.prediction
            .as_ref()
            .and_then(|p| p.impact)
            .map_or("-".into(), |(t, _)| format!(
                "{:.0} s",
                (t - rocket.time()).max(0.0)
            )),
        state.velocity.length(),
        vertical,
        mode(rocket.part_mode(RocketPart::Upper)),
        rocket.part_fuel_kg(RocketPart::Upper),
        rocket.part_delta_v(RocketPart::Upper),
        mode(rocket.part_mode(RocketPart::Booster)),
        rocket.part_fuel_kg(RocketPart::Booster),
        rocket.part_delta_v(RocketPart::Booster),
        worlds.len(),
        tiles_loaded,
        field.0.drawn_count(),
        field.0.levels.0,
        field.0.levels.1,
        field.0.building_count(),
        rocket.crashes.len(),
    );
}

fn meters(m: f64) -> String {
    if m.abs() >= 1e4 {
        format!("{:.2} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}

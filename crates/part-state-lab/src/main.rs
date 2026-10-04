//! Independent acceptance lab; all actions go through FleetFlight's durable command journal.
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use void_assembly::{
    Craft, Module, ModuleState, ResourceId, add_part, definition, flight_rocket, fresh_craft,
    full_resources,
};
use void_fleet_flight::session::{
    Action, FlightSession, InitialWorld, Outcome, Playback, Recording,
};
use void_landing::{FrameState, PlanetFrame, earth_size};
use void_vessels::flat_site;

struct Lab {
    session: FlightSession,
    playback: Option<Playback>,
    paused: bool,
    rate: f64,
    message: String,
}
#[derive(Resource)]
struct Shapes {
    parts: std::collections::BTreeMap<String, Handle<Mesh>>,
    sphere: Handle<Mesh>,
    ground: Handle<Mesh>,
    part_material: Handle<StandardMaterial>,
    chute_material: Handle<StandardMaterial>,
    ground_material: Handle<StandardMaterial>,
}
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct ShipVisual;
fn root_craft(def: &str) -> Craft {
    let mut c = fresh_craft();
    c.parts[0].definition_id = def.into();
    c.parts[0].resources = full_resources(definition(def).unwrap());
    c.parts[0].stage = (def == "dual-resource-pod").then_some(0);
    c
}
fn action(lab: &mut Lab, action: Action) {
    lab.message = format!("{:?}", lab.session.execute(action));
}
fn preset(lab: &mut Lab, index: u8) {
    let p = earth_size();
    let site = flat_site(&p);
    let mut c = match index {
        1 => flight_rocket(),
        2 => root_craft("dual-resource-pod"),
        3 | 4 => root_craft("parachute-pod"),
        _ => panic!("unknown lab preset"),
    };
    if index == 4 {
        c = add_part(&c, "decoupler", "p1", "bottom", "top").unwrap();
        c = add_part(&c, "parachute-pod", "p2", "bottom", "top").unwrap();
        c.parts[1].stage = Some(0);
    }
    action(
        lab,
        Action::ResetWorld {
            initial: Box::new(InitialWorld::new(&p, &c, site, index != 2)),
        },
    );
    if index != 1 {
        let sim = lab.session.sim();
        let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
        let altitude = if index == 2 {
            500_000.0
        } else if index == 4 {
            4000.0
        } else {
            1000.0
        };
        let local = FrameState {
            position: site * (p.terrain.radius_meters + altitude),
            velocity: if index == 2 {
                DVec3::ZERO
            } else {
                -site * 80.0
            },
        };
        let state = frame.to_inertial(&sim.fleet.ephemeris, sim.fleet.time(), local);
        let rotation = sim
            .fleet
            .frames()
            .transform(sim.fleet.body_frames(sim.home).1, sim.fleet.origin_frame())
            .to_motion()
            .rotation
            * void_landing::upright_at(site);
        let id = match lab.session.execute(Action::LaunchState {
            craft: c,
            position: state.position,
            velocity: state.velocity,
            rotation,
            angular_velocity: DVec3::ZERO,
        }) {
            Outcome::Spawned(id) => id,
            _ => panic!("failed lab spawn"),
        };
        action(lab, Action::Select { vessel: id });
        if index == 2 {
            action(lab, Action::Stage);
            action(
                lab,
                Action::Control {
                    throttle: 1.0,
                    turn: DVec3::ZERO,
                },
            );
        }
    }
    lab.paused = true;
    lab.rate = 1.0;
    lab.session.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    lab.message = format!("Preset {index}: paused; press P to run");
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |key: &str| {
        args.iter()
            .position(|s| s == key)
            .map(|i| args.get(i + 1).expect("flag needs path").clone())
    };
    if let Some(path) = arg("--verify") {
        let recording = Recording::read(path);
        let sim = FlightSession::from_recording(recording);
        println!("verified, T+{:.3}s", sim.sim().fleet.time());
        return;
    }
    let p = earth_size();
    let initial = InitialWorld::new(&p, &flight_rocket(), flat_site(&p), true);
    let (playback, session) = if let Some(path) = arg("--replay") {
        let (p, s) = Playback::new(Recording::read(path));
        (Some(p), s)
    } else {
        (None, FlightSession::new(initial))
    };
    let mut lab = Lab {
        session,
        playback,
        paused: false,
        rate: 1.0,
        message: String::new(),
    };
    if let Some(path) = arg("--record") {
        assert!(lab.playback.is_none(), "record/replay conflict");
        lab.session.begin_stream(path);
    }
    if lab.playback.is_none() {
        preset(&mut lab, 3);
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID part state / resources lab".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_non_send(lab)
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, simulate, draw).chain())
        .run();
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Shapes {
        parts: Default::default(),
        sphere: meshes.add(Sphere::new(1.0)),
        ground: meshes.add(Cylinder::new(1000.0, 0.2)),
        part_material: materials.add(Color::srgb(0.7, 0.75, 0.8)),
        chute_material: materials.add(Color::srgb(1.0, 0.45, 0.12)),
        ground_material: materials.add(Color::srgb(0.25, 0.4, 0.25)),
    });
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(35.0, 25.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_xyz(10.0, 30.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
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
fn controls(keys: Res<ButtonInput<KeyCode>>, mut lab: NonSendMut<Lab>) {
    if lab.playback.is_some() {
        return;
    }
    for (key, index) in [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4),
    ] {
        if keys.just_pressed(key) {
            preset(&mut lab, index);
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::KeyW) {
        lab.rate = if lab.rate == 1.0 { 4.0 } else { 1.0 };
    }
    if keys.just_pressed(KeyCode::Space) {
        action(&mut lab, Action::Stage);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        action(&mut lab, Action::Sas { enabled: true });
        action(
            &mut lab,
            Action::Control {
                throttle: 1.0,
                turn: DVec3::ZERO,
            },
        );
    }
    if keys.just_pressed(KeyCode::Tab) {
        let sim = lab.session.sim();
        let ids = sim.fleet.vessel_ids();
        let index = ids.iter().position(|id| id == &sim.selected).unwrap();
        let next = ids[(index + 1) % ids.len()].clone();
        action(&mut lab, Action::Select { vessel: next });
    }
    if keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::KeyC) {
        let deploy = keys.just_pressed(KeyCode::KeyD);
        let sim = lab.session.sim();
        let modules: Vec<_> = sim
            .fleet
            .part_snapshots(&sim.selected)
            .into_iter()
            .flat_map(|p| {
                p.definition.modules.iter().filter_map(move |m| {
                    if let Module::Parachute { id, .. } = m {
                        Some((p.id.clone(), id.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect();
        let count = modules.len();
        for (part, module) in modules {
            action(
                &mut lab,
                Action::Parachute {
                    part,
                    module,
                    deploy,
                },
            );
        }
        lab.message = if count == 0 {
            "Selected vessel has no parachute; use preset 3 or 4".into()
        } else if deploy && lab.paused {
            "Parachutes armed; simulation paused — press P to begin deployment".into()
        } else if deploy {
            "Parachutes armed; deployment waits for safe pressure / dynamic pressure".into()
        } else {
            "Parachutes cut".into()
        };
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        let sim = lab.session.sim();
        let all = sim.fleet.vessel_ids();
        if all.len() == 3 && sim.fleet.parts().connection_at("v2/p1", "bottom").is_none() {
            action(
                &mut lab,
                Action::Join {
                    part_a: "v2/p1".into(),
                    node_a: "bottom".into(),
                    part_b: "v2/p2".into(),
                    node_b: "top".into(),
                },
            );
        } else {
            lab.message = "J requires split preset 4".into();
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        action(
            &mut lab,
            Action::Advance {
                seconds: 1.0,
                rails: true,
            },
        );
    }
    if keys.just_pressed(KeyCode::F6) {
        let paused = lab.paused;
        let rate = if lab.rate == 1.0 { 0 } else { 2 };
        lab.session.execute(Action::EndFrame { paused, rate });
        lab.session.save_checkpoint("lab-log/part-state-save.json");
        lab.message = "saved checkpoint".into();
    }
    if keys.just_pressed(KeyCode::F7) {
        action(
            &mut lab,
            Action::LoadWorld {
                checkpoint: Box::new(void_fleet_flight::checkpoint::FlightCheckpoint::read(
                    "lab-log/part-state-save.json",
                )),
            },
        );
        sync_presentation(&mut lab);
    }
    if keys.just_pressed(KeyCode::F8) {
        if lab.session.streaming() {
            lab.session.finish_stream();
            lab.message = "recording stopped".into();
        } else {
            lab.session.begin_stream("lab-log/part-state.jsonl");
            lab.message = "recording started".into();
        }
    }
}
fn sync_presentation(lab: &mut Lab) {
    let p = &lab.session.sim().presentation;
    lab.paused = p.paused;
    lab.rate = match p.rate {
        0 => 1.0,
        2 => 4.0,
        _ => panic!("recording uses a rate outside this lab's 1x/4x controls"),
    };
}
fn simulate(time: Res<Time>, mut lab: NonSendMut<Lab>) {
    if let Some(mut p) = lab.playback.take() {
        if p.next_frame(&mut lab.session) {
            sync_presentation(&mut lab);
            lab.playback = Some(p);
        } else {
            lab.paused = true;
            lab.message = "replay verified and finished".into();
        }
        return;
    }
    if !lab.paused {
        let seconds = time.delta_secs_f64().min(0.05) * lab.rate;
        action(
            &mut lab,
            Action::Advance {
                seconds,
                rails: false,
            },
        );
    }
    let paused = lab.paused;
    let rate = if lab.rate == 1.0 { 0 } else { 2 };
    lab.session.execute(Action::EndFrame { paused, rate });
    if lab.session.streaming() {
        lab.session.mark();
    }
}
#[allow(clippy::too_many_arguments)] // Bevy system parameters have independently checked access.
fn draw(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shapes: ResMut<Shapes>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
    visuals: Query<Entity, With<ShipVisual>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut gizmos: Gizmos,
) {
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let sim = lab.session.sim();
    let ship = sim.fleet.snapshot(&sim.selected);
    let origin = ship.position
        + ship.rotation
            * (sim.fleet.root_position_local(&sim.selected)
                - sim.fleet.centre_of_mass_local(&sim.selected));
    let fixed = sim.body_fixed(
        sim.home,
        FrameState {
            position: ship.position,
            velocity: ship.velocity,
        },
    );
    let up = (ship.position
        - sim
            .fleet
            .ephemeris
            .body_position(sim.home, sim.fleet.time()))
    .normalize();
    let reference = if up.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let side = up.cross(reference).normalize();
    let front = up.cross(side);
    **camera = Transform::from_translation((up * 20.0 + side * 30.0 + front * 35.0).as_vec3())
        .looking_at(Vec3::ZERO, up.as_vec3());
    let air = void_modules::Conditions::at(
        sim.fleet.environment(),
        &*sim.fleet.ephemeris,
        sim.fleet.time(),
        void_frames::State {
            position: ship.position,
            velocity: ship.velocity,
        },
    )
    .air;
    let q = air.map_or(0.0, |a| 0.5 * a.air.density * a.airspeed.length_squared());
    let ground_fixed = fixed.position.normalize()
        * (sim.planet.terrain.radius_meters
            + sim.planet.terrain.height(fixed.position.normalize()));
    let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let ground = frame
        .to_inertial(
            &sim.fleet.ephemeris,
            sim.fleet.time(),
            FrameState {
                position: ground_fixed,
                velocity: DVec3::ZERO,
            },
        )
        .position
        - origin;
    commands.spawn((
        ShipVisual,
        Mesh3d(shapes.ground.clone()),
        MeshMaterial3d(shapes.ground_material.clone()),
        Transform::from_translation(ground.as_vec3())
            .with_rotation(DQuat::from_rotation_arc(DVec3::Y, up).as_quat()),
    ));
    let mut lines = format!(
        "1 old rocket  2 dual resources (vacuum)  3 return pod  4 two chutes / decoupler\nP pause  W 1x/4x physics  D deploy  C cut  Space stage  Tab vessel  J debug join\nT SAS/full throttle  R request rails  F6 save  F7 load  F8 record/stop\nT+{:.3}s  {} {:?}  {:.1}kg  altitude {:.1}m  speed {:.1}m/s\nrails {:?}  paused {}  rate {}x  recording {}\n{}\nForce-only: no parachute attitude torque\n",
        sim.fleet.time(),
        sim.selected,
        ship.mode,
        ship.mass_kg,
        fixed.position.length() - sim.planet.terrain.radius_meters,
        fixed.velocity.length(),
        sim.fleet.rails_blocker(),
        lab.paused,
        lab.rate,
        lab.session.streaming(),
        lab.message
    );
    lines.push_str(&format!("dynamic pressure {q:.1}Pa\n"));
    // Small independent scene deliberately shows the module state, rather than maintaining another runtime.
    for p in sim.fleet.part_snapshots(&sim.selected) {
        let relative = p.position - origin;
        commands.spawn((
            ShipVisual,
            Mesh3d(
                shapes
                    .parts
                    .entry(p.definition.id.clone())
                    .or_insert_with(|| {
                        meshes.add(Cylinder::new(
                            p.definition.radius as f32,
                            p.definition.height as f32,
                        ))
                    })
                    .clone(),
            ),
            MeshMaterial3d(shapes.part_material.clone()),
            Transform::from_translation(relative.as_vec3()).with_rotation(p.rotation.as_quat()),
        ));
        lines.push_str(&format!(
            "{} liquid {:.3}kg mono {:.3}kg\n",
            p.id,
            p.resources
                .get(&ResourceId::LiquidPropellant)
                .copied()
                .unwrap_or(0.0),
            p.resources
                .get(&ResourceId::Monopropellant)
                .copied()
                .unwrap_or(0.0)
        ));
        for m in &p.definition.modules {
            if let Module::Parachute { id, parameters } = m {
                let ModuleState::Parachute { state } = p.modules[id] else {
                    panic!()
                };
                let area = void_modules::parachute::area(state, parameters, 0.0);
                lines.push_str(&format!(
                    "  {id} {:?} {:.3}s area {:.2}m2\n",
                    state.phase, state.elapsed_seconds, area
                ));
                if area > 0.0 {
                    let radius = (area / std::f64::consts::PI).sqrt();
                    let canopy = relative + up * 8.0;
                    commands.spawn((
                        ShipVisual,
                        Mesh3d(shapes.sphere.clone()),
                        MeshMaterial3d(shapes.chute_material.clone()),
                        Transform::from_translation(canopy.as_vec3())
                            .with_rotation(DQuat::from_rotation_arc(DVec3::Y, up).as_quat())
                            .with_scale(Vec3::new(
                                radius as f32,
                                (radius * 0.35) as f32,
                                radius as f32,
                            )),
                    ));
                    for i in 0..8 {
                        let angle = i as f64 * std::f64::consts::TAU / 8.0;
                        let rim = canopy + (side * angle.cos() + front * angle.sin()) * radius;
                        gizmos.line(relative.as_vec3(), rim.as_vec3(), Color::WHITE);
                    }
                }
            }
        }
    }
    hud.0 = lines;
}

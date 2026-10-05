//! Near-rendezvous acceptance lab. All changes use the common durable FlightSession journal.
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use void_assembly::{Module, ResourceId, rendezvous_pod};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Playback, Recording};
use void_landing::{FrameState, PlanetFrame, earth_size};
use void_vessels::{RcsControl, flat_site};
struct Lab {
    session: FlightSession,
    playback: Option<Playback>,
    paused: bool,
    enabled: bool,
    message: String,
}
#[derive(Resource)]
struct Shapes {
    pod: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Ship;
fn act(lab: &mut Lab, action: Action) {
    lab.message = format!("{:?}", lab.session.execute(action));
}
fn preset(lab: &mut Lab, near: bool) {
    let p = earth_size();
    let site = flat_site(&p);
    let craft = rendezvous_pod();
    act(
        lab,
        Action::ResetWorld {
            initial: Box::new(InitialWorld::new(&p, &craft, site, true)),
        },
    );
    let sim = lab.session.sim();
    let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let state = frame.to_inertial(
        &sim.fleet.ephemeris,
        0.,
        FrameState {
            position: site * (p.terrain.radius_meters + 500_000.),
            velocity: DVec3::ZERO,
        },
    );
    for i in 0..2 {
        act(
            lab,
            Action::LaunchState {
                craft: craft.clone(),
                position: state.position + DVec3::Y * (if near { 2.15 } else { 8. }) * i as f64,
                velocity: state.velocity,
                rotation: if i == 0 {
                    DQuat::IDENTITY
                } else {
                    DQuat::from_rotation_x(std::f64::consts::PI)
                },
                angular_velocity: DVec3::ZERO,
            },
        );
    }
    act(
        lab,
        Action::Select {
            vessel: "v2".into(),
        },
    );
    lab.paused = true;
    lab.enabled = true;
    act(
        lab,
        Action::Rcs {
            control: RcsControl {
                enabled: true,
                ..Default::default()
            },
        },
    );
    act(
        lab,
        Action::EndFrame {
            paused: true,
            rate: 0,
        },
    );
    lab.message = "Two pods ready. P run, Shift approach along +Y; J capture.".into();
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |key: &str| {
        args.iter()
            .position(|s| s == key)
            .map(|i| args.get(i + 1).expect("flag requires path").clone())
    };
    if let Some(path) = arg("--verify") {
        let s = FlightSession::from_recording(Recording::read(path));
        println!("Verified RCS/docking T+{:0.3}", s.sim().fleet.time());
        return;
    }
    let p = earth_size();
    let initial = InitialWorld::new(&p, &rendezvous_pod(), flat_site(&p), true);
    let (playback, session) = if let Some(path) = arg("--replay") {
        let (p, s) = Playback::new(Recording::read(path));
        (Some(p), s)
    } else {
        (None, FlightSession::new(initial))
    };
    let mut lab = Lab {
        session,
        playback,
        paused: true,
        enabled: true,
        message: String::new(),
    };
    if let Some(path) = arg("--record") {
        assert!(lab.playback.is_none(), "record/replay conflict");
        lab.session.begin_stream(path);
    }
    if lab.playback.is_none() {
        preset(&mut lab, true);
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID RCS / physical docking lab".into(),
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
        pod: meshes.add(Cylinder::new(0.45, 2.)),
        material: materials.add(Color::srgb(0.65, 0.8, 0.95)),
    });
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(7., 5., 8.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 7000.,
            ..default()
        },
        Transform::from_xyz(5., 8., 6.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.),
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
    if keys.just_pressed(KeyCode::Digit1) {
        preset(&mut lab, true);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        preset(&mut lab, false);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.enabled = !lab.enabled;
    }
    if keys.just_pressed(KeyCode::Tab) {
        let sim = lab.session.sim();
        let ids: Vec<_> = sim
            .fleet
            .vessel_ids()
            .into_iter()
            .filter(|id| id != "v1")
            .collect();
        let index = ids.iter().position(|id| id == &sim.selected).unwrap();
        let next = ids[(index + 1) % ids.len()].clone();
        let (outcome, enabled) = void_rcs_docking_lab::pilot_handoff(&mut lab.session, &next);
        lab.message = format!("{outcome:?}");
        lab.enabled = enabled;
    }
    let axis = |positive, negative| {
        f64::from(u8::from(keys.pressed(positive))) - f64::from(u8::from(keys.pressed(negative)))
    };
    let precision = if keys.pressed(KeyCode::AltLeft) {
        0.1
    } else {
        1.
    };
    let force = DVec3::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::ShiftLeft, KeyCode::ControlLeft),
        axis(KeyCode::KeyS, KeyCode::KeyW),
    ) * 40.
        * precision;
    let torque = DVec3::new(
        axis(KeyCode::ArrowUp, KeyCode::ArrowDown),
        axis(KeyCode::KeyE, KeyCode::KeyQ),
        axis(KeyCode::ArrowLeft, KeyCode::ArrowRight),
    ) * 15.
        * precision;
    let control = RcsControl {
        enabled: lab.enabled,
        force,
        torque,
    };
    if lab
        .session
        .sim()
        .fleet
        .rcs_control(&lab.session.sim().selected)
        != control
    {
        act(&mut lab, Action::Rcs { control });
    }
    if keys.just_pressed(KeyCode::KeyH) {
        let sim = lab.session.sim();
        let part = sim.fleet.snapshot(&sim.selected).part_ids[0].clone();
        let module = "rcs-0-0-1".to_string();
        let enabled = !matches!(
            sim.fleet.parts().part(&part).modules[&module],
            void_assembly::ModuleState::Rcs { enabled: true }
        );
        act(
            &mut lab,
            Action::RcsNozzle {
                part,
                module,
                enabled,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        act(
            &mut lab,
            Action::Dock {
                part_a: "v2/p1".into(),
                module_a: "dock".into(),
                part_b: "v3/p1".into(),
                module_b: "dock".into(),
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyU) {
        act(
            &mut lab,
            Action::Undock {
                part: "v2/p1".into(),
                module: "dock".into(),
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyC) {
        for part in ["v2/p1", "v3/p1"] {
            act(
                &mut lab,
                Action::ArmDock {
                    part: part.into(),
                    module: "dock".into(),
                    armed: true,
                },
            );
        }
    }
    if keys.just_pressed(KeyCode::F6) {
        let paused = lab.paused;
        act(&mut lab, Action::EndFrame { paused, rate: 0 });
        lab.session.save_checkpoint("lab-log/rcs-docking-save.json");
        lab.message = "Checkpoint saved".into();
    }
    if keys.just_pressed(KeyCode::F7) {
        act(
            &mut lab,
            Action::LoadWorld {
                checkpoint: Box::new(void_fleet_flight::checkpoint::FlightCheckpoint::read(
                    "lab-log/rcs-docking-save.json",
                )),
            },
        );
        sync(&mut lab);
    }
    if keys.just_pressed(KeyCode::F8) {
        if lab.session.streaming() {
            lab.session.finish_stream();
            lab.message = "Recording stopped".into();
        } else {
            lab.session.begin_stream("lab-log/rcs-docking.jsonl");
            lab.message = "Recording started".into();
        }
    }
}
fn sync(lab: &mut Lab) {
    lab.paused = lab.session.sim().presentation.paused;
    lab.enabled = lab
        .session
        .sim()
        .fleet
        .rcs_control(&lab.session.sim().selected)
        .enabled;
}
fn simulate(time: Res<Time>, mut lab: NonSendMut<Lab>) {
    if let Some(mut playback) = lab.playback.take() {
        if playback.next_frame(&mut lab.session) {
            sync(&mut lab);
            lab.playback = Some(playback);
        } else {
            lab.paused = true;
            lab.message = "Replay verified and finished".into();
        }
        return;
    }
    if !lab.paused {
        let seconds = time.delta_secs_f64().min(0.05);
        lab.session.execute(Action::Advance {
            seconds,
            rails: false,
        });
    }
    let paused = lab.paused;
    lab.session.execute(Action::EndFrame { paused, rate: 0 });
    if lab.session.streaming() {
        lab.session.mark();
    }
}
fn vector(v: DVec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", v.x, v.y, v.z)
}
fn draw(
    mut commands: Commands,
    lab: NonSend<Lab>,
    shapes: Res<Shapes>,
    visuals: Query<Entity, With<Ship>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    for e in &visuals {
        commands.entity(e).despawn();
    }
    let sim = lab.session.sim();
    let selected = sim.fleet.snapshot(&sim.selected);
    let origin = selected.position;
    let mut lines = format!(
        "1 capture-ready  2 rendezvous at 8m  P pause  Tab ship  R RCS toggle\nWASD X/Z  Shift/Ctrl Y  arrows X/Z rotation  Q/E Y rotation  Alt precision\nJ physical capture  U undock  C rearm both  H nozzle toggle  F6 save  F7 load  F8 record\nT+{:0.2}  {} {:?}  paused {}  RCS {}\n{}\n",
        sim.fleet.time(),
        sim.selected,
        selected.mode,
        lab.paused,
        lab.enabled,
        lab.message
    );
    let a = sim
        .fleet
        .part_snapshots(&sim.fleet.vessel_of_part("v2/p1"))
        .into_iter()
        .find(|p| p.id == "v2/p1")
        .unwrap();
    let b = sim
        .fleet
        .part_snapshots(&sim.fleet.vessel_of_part("v3/p1"))
        .into_iter()
        .find(|p| p.id == "v3/p1")
        .unwrap();
    let midpoint = ((a.position - origin) + (b.position - origin)) * 0.5;
    let zoom = (b.position - a.position).length().max(4.0) / 4.0;
    **camera = Transform::from_translation((midpoint + DVec3::new(7.0, 5.0, 8.0) * zoom).as_vec3())
        .looking_at(midpoint.as_vec3(), Vec3::Y);
    let delta = (b.position - a.position) + b.rotation * DVec3::Y - a.rotation * DVec3::Y;
    lines.push_str(&format!(
        "Port distance {:0.3}m; states {:?} / {:?}\n",
        delta.length(),
        a.modules["dock"],
        b.modules["dock"]
    ));
    for id in sim.fleet.vessel_ids().into_iter().filter(|id| id != "v1") {
        let ship = sim.fleet.snapshot(&id);
        let allocation = sim.fleet.rcs_allocation(&id);
        for part in sim.fleet.part_snapshots(&id) {
            commands.spawn((
                Ship,
                Mesh3d(shapes.pod.clone()),
                MeshMaterial3d(shapes.material.clone()),
                Transform::from_translation((part.position - origin).as_vec3())
                    .with_rotation(part.rotation.as_quat()),
            ));
            let centre = part.position - origin;
            for (axis, color) in [
                (DVec3::X, Color::srgb(1.0, 0.1, 0.1)),
                (DVec3::Z, Color::srgb(0.2, 0.5, 1.0)),
            ] {
                gizmos.line(
                    (centre + part.rotation * axis * 0.5).as_vec3(),
                    (centre + part.rotation * axis * 0.9).as_vec3(),
                    color,
                );
            }
            let port = centre + part.rotation * DVec3::Y;
            gizmos.line(
                port.as_vec3(),
                (port + part.rotation * DVec3::Y * 0.5).as_vec3(),
                Color::srgb(0.2, 1., 0.2),
            );
            lines.push_str(&format!(
                "{} mono {:0.3}kg\n",
                part.id,
                part.resources[&ResourceId::Monopropellant]
            ));
            if id == sim.selected {
                let mut count = 0;
                for module in &part.definition.modules {
                    if let Module::Rcs { id: module_id, .. } = module {
                        let throttle = allocation
                            .nozzles
                            .iter()
                            .find(|n| n.part == part.id && n.module == *module_id)
                            .map_or(0., |n| n.throttle);
                        let state = &part.modules[module_id];
                        let label = if matches!(
                            state,
                            void_assembly::ModuleState::Rcs { enabled: false }
                        ) {
                            " off".into()
                        } else {
                            format!("{:3.0}%", throttle * 100.0)
                        };
                        lines.push_str(&format!("{module_id}: {label}  "));
                        count += 1;
                        if count % 6 == 0 {
                            lines.push('\n');
                        }
                    }
                }
                lines.push('\n');
            }
        }
        for n in &allocation.nozzles {
            if n.throttle > 1e-6 {
                let point = void_rcs_docking_lab::nozzle_point_relative(
                    &ship,
                    sim.fleet.centre_of_mass_local(&id),
                    n.point,
                    origin,
                );
                let exhaust = point - ship.rotation * n.full_force.normalize() * n.throttle;
                gizmos.line(
                    point.as_vec3(),
                    exhaust.as_vec3(),
                    Color::srgb(1., 0.5, 0.1),
                );
            }
        }
        if id == sim.selected {
            lines.push_str(&format!(
                "Applied F {} N; torque {} Nm\nResidual F {}; torque {}\n",
                vector(allocation.force),
                vector(allocation.torque),
                vector(allocation.force_residual),
                vector(allocation.torque_residual)
            ));
        }
    }
    hud.0 = lines;
}

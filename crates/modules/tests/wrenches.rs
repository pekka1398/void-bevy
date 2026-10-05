use glam::{DQuat, DVec3};
use void_assembly::{
    ModuleState, ParachutePhase, ParachuteState, PartGraph, PartPose, compile, fresh_craft,
};
use void_frames::{FrameTree, Motion, State};
use void_modules::{Wrench, vessel_air};
fn graph(def: &str) -> (PartGraph, Vec<String>) {
    let mut c = fresh_craft();
    c.parts[0].definition_id = def.into();
    let mut g = PartGraph::new();
    let ids = g.add(&compile(&c).unwrap(), "v1");
    (g, ids)
}
#[test]
fn wrench_reference_shift_and_frame_rotation_preserve_one_moment_arm() {
    let mut tree = FrameTree::new();
    let root = tree.add_system(void_frames::SystemId(0));
    let frame = tree.add_fixed(
        root,
        Motion::fixed(DVec3::new(2.0, 3.0, 4.0), DQuat::from_rotation_z(0.7)),
    );
    let reference = DVec3::new(1.0, 2.0, 3.0);
    let point = reference + DVec3::X * 2.0;
    let force = DVec3::Y * 5.0;
    let w = Wrench::at_point(frame, reference, point, force, DVec3::X);
    assert_eq!(w.torque, DVec3::Z * 10.0 + DVec3::X);
    let moved = w.about(point);
    assert_eq!(moved.torque, DVec3::X);
    assert_eq!(moved.about(reference), w);
    // FrameTree root itself has no dynamic/external source requirement.
    let (e, _) = void_landing::planet_ephemeris(&void_landing::earth_size());
    let at = tree.at(0.0, &e);
    let roundtrip = w.in_frame(&at, root).in_frame(&at, frame);
    assert!((roundtrip.force - w.force).length() < 1e-12);
    assert!((roundtrip.torque - w.torque).length() < 1e-12);
    assert!((roundtrip.reference_point - w.reference_point).length() < 1e-12);
    let mut sum = Wrench::zero(frame, reference);
    sum.add(w);
    assert_eq!(sum, w);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sum.add(w.about(point)))).is_err()
    );
}
#[test]
fn point_wind_damps_rotation_and_drag_power_is_nonpositive() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, true);
    let (g, ids) = graph("aero-stabilizer-pod");
    let source = vessel_air(&env, &g, &ids, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    let frames = env.frames();
    let at = frames.tree.at(0.0, &e);
    let query = frames.surface[home];
    let state = State {
        position: DVec3::X * (p.terrain.radius_meters + 5000.0),
        velocity: DVec3::ZERO,
    };
    let still = source.wrench_in(&at, query, state, DQuat::IDENTITY, DVec3::ZERO);
    assert_eq!(still.force, DVec3::ZERO);
    assert_eq!(still.torque, DVec3::ZERO);
    for rate in [0.5, 5.0, 50.0] {
        let spin = DVec3::Z * rate;
        let spinning = source.wrench_in(&at, query, state, DQuat::IDENTITY, spin);
        assert!(
            spinning.torque.dot(spin) < 0.0 && spinning.force.is_finite(),
            "{spinning:?}"
        );
    }
    let spin = DVec3::Z * 0.5;
    let spinning = source.wrench_in(&at, query, state, DQuat::IDENTITY, spin);
    assert!(spinning.torque.dot(spin) < 0.0, "{spinning:?}");
    let moving = State {
        velocity: DVec3::new(8.0, 80.0, 3.0),
        ..state
    };
    let load = source.wrench_in(&at, query, moving, DQuat::IDENTITY, spin);
    assert!(
        load.force.dot(moving.velocity) + load.torque.dot(spin) < 0.0,
        "{load:?}"
    );
    let high = State {
        position: DVec3::X * (p.terrain.radius_meters + 200000.0),
        ..moving
    };
    let vacuum = source.wrench_in(&at, query, high, DQuat::IDENTITY, spin);
    assert_eq!(vacuum.force, DVec3::ZERO);
    assert_eq!(vacuum.torque, DVec3::ZERO);
}
#[test]
fn symmetric_body_offsets_cancel_and_eccentric_chute_has_exact_moment() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, true);
    let frames = env.frames();
    let at = frames.tree.at(0.0, &e);
    let query = frames.surface[home];
    let state = State {
        position: DVec3::X * (p.terrain.radius_meters + 1000.0),
        velocity: DVec3::Y * 80.0,
    };
    let (mut g, ids) = graph("pod");
    g.set_pose(
        &ids[0],
        PartPose {
            position: DVec3::Z,
            rotation: DQuat::IDENTITY,
        },
    );
    let mut other = g.part(&ids[0]).clone();
    other.id = "mirror".into();
    other.pose.position = -DVec3::Z;
    g.insert(other);
    let members = vec![ids[0].clone(), "mirror".into()];
    let air = vessel_air(&env, &g, &members, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    let load = air.wrench_in(&at, query, state, DQuat::IDENTITY, DVec3::ZERO);
    assert!(load.torque.length() < 1e-8, "{load:?}");
    let (mut g, ids) = graph("eccentric-chute-pod");
    let stowed = vessel_air(&env, &g, &ids, DVec3::ZERO, DQuat::IDENTITY)
        .unwrap()
        .wrench_in(&at, query, state, DQuat::IDENTITY, DVec3::ZERO);
    let mut part = g.part(&ids[0]).clone();
    part.id = "full".into();
    part.modules.insert(
        "parachute1".into(),
        ModuleState::Parachute {
            state: ParachuteState {
                phase: ParachutePhase::Full,
                elapsed_seconds: 0.0,
            },
        },
    );
    g.insert(part);
    let air = vessel_air(&env, &g, &["full".into()], DVec3::ZERO, DQuat::IDENTITY).unwrap();
    let before = g.part("full").modules.clone();
    let full = air.wrench_in(&at, query, state, DQuat::IDENTITY, DVec3::ZERO);
    let expected = DVec3::X.cross(full.force - stowed.force);
    assert!(
        (full.torque - stowed.torque - expected).length() < 1e-8,
        "{full:?}"
    );
    assert!(full.torque.length() > 100.0);
    assert_eq!(before, g.part("full").modules);
}

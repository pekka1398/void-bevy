mod common;
use glam::DVec3;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
use void_frames::{BodyId, BodyStates, SplitPosition};
use void_multiscale::*;
use void_orbit::{EphemerisSource, Tolerances, VesselPropagator};

#[test]
fn accelerating_origin_preserves_all_gravity_sources() {
    let world = Rc::new(RefCell::new(CoupledWorld::new(
        common::compact_seeds(SplitPosition::at(DVec3::ZERO)),
        10.0,
        8192,
    )));
    let mut e = FrameEphemeris::new(world.clone(), "A");
    e.extend_to(100.0);
    let mut prop = VesselPropagator::new(
        &e,
        Tolerances {
            position_meters: 1e-5,
            velocity_meters_per_second: 1e-8,
        },
    );
    let p = DVec3::new(8e8, 1e9, 2e8);
    let w = world.borrow();
    let origin = &w.at(50.0)[0];
    let expected = w.gravity_at(50.0, &origin.origin.translate(p)) - origin.acceleration;
    assert!(origin.acceleration.length() > 1e-6);
    assert!((expected - prop.gravity_at(&e, 50.0, p)).length() < 1e-12);
    let mut positions = vec![DVec3::ZERO; e.bodies().len()];
    let mut velocities = positions.clone();
    e.states_at(50.0, &mut positions, Some(&mut velocities));
    for i in 0..positions.len() {
        let (p, v) = e.body_state(BodyId(i), 50.0);
        assert_eq!(p, positions[i]);
        assert_eq!(v, velocities[i]);
    }
    drop(w);
    let mut other = FrameEphemeris::new(world.clone(), "B");
    other.extend_to(150.0);
    assert_eq!(e.end_time(), other.end_time());
    assert!(catch_unwind(AssertUnwindSafe(|| e.forget_before(50.0))).is_err());
}

#[test]
fn system_frames_agree_with_the_physics_view() {
    let world = Rc::new(RefCell::new(CoupledWorld::new(
        common::compact_seeds(SplitPosition::at(DVec3::new(3e17, -2e16, 5e15))),
        10.0,
        8192,
    )));
    let mut e = FrameEphemeris::new(world.clone(), "B");
    e.extend_to(100.0);
    let frames = void_orbit::SystemFrames::new(&e);
    assert_eq!(frames.systems.len(), 3);
    let t = 50.0;
    let snapshot = frames.tree.at(t, &e);
    let mut foreign = 0;
    for i in 0..e.bodies().len() {
        if e.system_of(i) != e.origin_system() {
            foreign += 1;
        }
        let (p, v) = e.body_state(BodyId(i), t);
        let s = snapshot
            .transform(frames.inertial[i], frames.origin)
            .apply_state(void_frames::State {
                position: DVec3::ZERO,
                velocity: DVec3::ZERO,
            });
        // Bodies of other systems are 4e9 m away; the bridge is exact, so only the last bits move.
        assert!(
            (s.position - p).length() <= 1e-15 * p.length().max(1.0),
            "{i}: {s:?} {p}"
        );
        assert!(
            (s.velocity - v).length() <= 1e-12 * v.length().max(1.0),
            "{i}: {s:?} {v}"
        );
        let axes = e.bodies()[i].rotation.body_axes(t);
        let turn = snapshot.transform(frames.surface[i], frames.origin);
        for (k, axis) in axes.iter().enumerate() {
            let mut unit = DVec3::ZERO;
            unit[k] = 1.0;
            assert!(
                (turn.apply_direction(unit) - *axis).length() < 1e-15,
                "{i} axis {k}"
            );
        }
    }
    assert!(
        foreign > 0,
        "the fixture should have bodies outside the origin system"
    );
}

#[test]
fn split_probe_focus_keeps_metre_offsets_between_systems() {
    let world = wide_world(default_galaxy());
    let frames = world.frames("Aster");
    let snapshot = frames.tree.at(0.0, &world);
    let system = frames.systems[0];
    let probe_local = SplitPosition::at(DVec3::X * LIGHT_YEAR).translate(DVec3::X * 0.125);
    let probe = world.at(0.0)[0].origin.compose(&probe_local);
    // The probe and its trail use split galaxy positions, even far from their system.
    for metres in [0.0, 0.125, 1.0, 18.0] {
        let next = probe.translate(DVec3::X * metres);
        assert!((next.relative(&probe) - DVec3::X * metres).length() < 1e-4);
    }
    // Bodies use tree-local points. Put the anchor one metre from a body in another system:
    // the old system-f64 path loses this gap before subtracting the camera position.
    let body = frames.inertial[2];
    let body_position = snapshot.to_galaxy(body, DVec3::ZERO);
    let anchor = body_position.translate(DVec3::new(1.0, 0.125, -0.25));
    let relative = snapshot.relative_to_galaxy_anchor(body, DVec3::ZERO, &anchor);
    assert!((relative + DVec3::new(1.0, 0.125, -0.25)).length() < 1e-4);
    let old = snapshot.from_galaxy(&anchor, system) - snapshot.from_galaxy(&body_position, system);
    assert!((old - DVec3::new(1.0, 0.125, -0.25)).length() > 0.1);
}

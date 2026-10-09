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

#[test]
fn translated_source_subtracts_split_anchor_exactly_once_in_every_query() {
    use void_frames::{BodyId, BodyStates, SplitPosition, SystemId};
    use void_orbit::EphemerisSource;
    let world = std::rc::Rc::new(std::cell::RefCell::new(void_multiscale::wide_world(
        SplitPosition::ORIGIN,
    )));
    let mut view = void_multiscale::FrameEphemeris::new(world.clone(), "Beryl");
    let p = view.body_state(BodyId(3), 0.0).0;
    let offset = SplitPosition::at(p).translate(glam::DVec3::new(0.01, 20.0, 30.0));
    view.set_physics_offset(offset);
    let expected = glam::DVec3::new(-0.01, -20.0, -30.0);
    let scalar = view.body_state(BodyId(3), 0.0).0;
    let mut positions = vec![glam::DVec3::ZERO; view.bodies().len()];
    view.positions_at(0.0, &mut positions);
    assert_eq!(scalar, positions[3]);
    assert!((scalar - expected).length() < 1e-6, "{scalar:?}");
    let clone = view.local_view(SystemId(1)).unwrap();
    assert_eq!(clone.physics_offset(), SplitPosition::ORIGIN);
    assert_eq!(clone.body_state(BodyId(3), 0.0).0, p);
}

#[test]
fn prediction_snapshot_preserves_coupled_frames_and_retains_reader_history() {
    use void_orbit::{CancellationToken, PredictionBudget};
    let world = Rc::new(RefCell::new(CoupledWorld::new(
        common::compact_seeds(SplitPosition::at(DVec3::new(3e17, -2e16, 5e15))),
        10.0,
        8192,
    )));
    let mut live = FrameEphemeris::new(world.clone(), "B");
    live.extend_to(20.0);
    live.set_physics_offset(SplitPosition::at(DVec3::new(1e8, 3e7, -2e6)));
    let saved = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap();
    let result = std::thread::spawn(move || {
        let mut copy = saved.into_source();
        copy.try_extend_to(200.0).unwrap();
        assert_eq!(copy.start_time(), 0.0);
        assert_eq!(copy.origin_system(), void_frames::SystemId(1));
        (0..copy.bodies().len())
            .map(|b| copy.body_state(BodyId(b), 195.0))
            .collect::<Vec<_>>()
    })
    .join()
    .unwrap();
    assert_eq!(live.end_time(), 20.0);
    live.extend_to(200.0);
    for (b, state) in result.into_iter().enumerate() {
        assert_eq!(live.body_state(BodyId(b), 195.0), state);
    }
}

#[test]
fn coupled_prediction_budget_and_adoption_preserve_checkpoint_policy() {
    use void_orbit::{CancellationToken, PredictionBudget, PredictionError};
    let seeds = common::compact_seeds(SplitPosition::at(DVec3::ZERO));
    let world = Rc::new(RefCell::new(CoupledWorld::new(seeds.clone(), 10.0, 8192)));
    let mut live = FrameEphemeris::new(world.clone(), "A");
    live.extend_to(20.0);
    let mut copy = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    assert!(matches!(
        copy.try_extend_to(100_000.0),
        Err(PredictionError::BudgetExceeded {
            resource: "retained samples",
            ..
        })
    ));
    assert_eq!(copy.end_time(), 20.0);
    let mut copy = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    copy.try_extend_to(100.0).unwrap();
    live.adopt_prediction(copy.export_prediction().unwrap())
        .unwrap();
    assert_eq!(world.borrow().sample_limit, 8192);
    assert_eq!(world.borrow().time(), 100.0);
    let restored = CoupledWorld::from_checkpoint(seeds, world.borrow().checkpoint());
    assert_eq!(restored.at(95.0), world.borrow().at(95.0));
    let copy = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    world.borrow_mut().extend_to(110.0, 1);
    assert_eq!(
        live.adopt_prediction(copy.export_prediction().unwrap()),
        Err(PredictionError::IncompatibleSnapshot)
    );
}

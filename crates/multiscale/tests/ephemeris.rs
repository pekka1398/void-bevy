mod common;
use glam::DVec3;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
use void_frames::{BodyId, BodyStates};
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

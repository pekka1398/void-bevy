use glam::DVec3;
use void_frames::SplitPosition;
use void_multiscale::{CoupledCheckpoint, CoupledWorld, wide_seeds};

#[test]
fn exact_continuation_keeps_compensation_and_interpolation() {
    let origin = SplitPosition::new(DVec3::new(0.125, -0.5, 0.0), [1_i128 << 90, 0, 0]);
    let seeds = wide_seeds(origin);
    let mut original = CoupledWorld::new(seeds.clone(), 86400.0, 8192);
    original.extend_to(86400.0 * 17.0, 20);
    original.forget_before(86400.0 * 10.5);
    let wire = serde_json::to_string(&original.checkpoint()).unwrap();
    let saved: CoupledCheckpoint = serde_json::from_str(&wire).unwrap();
    let mut restored = CoupledWorld::from_checkpoint(seeds, saved);
    assert_eq!(restored.start_time(), original.start_time());
    assert_eq!(restored.at(86400.0 * 12.25), original.at(86400.0 * 12.25));
    original.extend_to(86400.0 * 32.0, 20);
    restored.extend_to(86400.0 * 32.0, 20);
    assert_eq!(restored.at(restored.time()), original.at(original.time()));
    assert_eq!(restored.steps, original.steps);
}

#[test]
fn altered_definitions_are_rejected() {
    let seeds = wide_seeds(SplitPosition::ORIGIN);
    let world = CoupledWorld::new(seeds.clone(), 86400.0, 8192);
    let saved = world.checkpoint();
    let mut changed = seeds;
    changed[0].system.bodies[0].j2 = 0.01;
    assert!(std::panic::catch_unwind(|| CoupledWorld::from_checkpoint(changed, saved)).is_err());
}

#[test]
fn vessel_gravity_retains_oblateness_in_nonzero_system() {
    let mut seeds = wide_seeds(SplitPosition::ORIGIN);
    seeds[1].system.bodies[1].j2 = 0.003;
    seeds[1].system.bodies[1].j2_reference_radius_meters = 6_371_000.0;
    let world = CoupledWorld::new(seeds, 86400.0, 8192);
    let state = world.at(0.0);
    let body = &world.bodies[3];
    let radial = DVec3::new(7_000_000.0, 3_000_000.0, 2_000_000.0);
    let point = world.body_position(3, &state).translate(radial);
    let mut expected = DVec3::ZERO;
    for (i, source) in world.bodies.iter().enumerate() {
        // Body positions are always subtracted as split coordinates before local f64 forces.
        let r = point.relative(&world.body_position(i, &state));
        expected += void_orbit::gravity::body_pull(source, r);
    }
    let actual = world.gravity_at(0.0, &point);
    assert!(
        (actual - expected).length() < 1e-12,
        "{actual:?} vs {expected:?}"
    );
    let correction = void_orbit::gravity::pull(
        0.0,
        void_orbit::gravity::oblateness(body),
        body.rotation.axis(),
        radial,
    );
    assert!(correction.length() > 1e-5);
}

#[test]
fn altered_placement_and_initial_velocity_are_rejected() {
    let seeds = wide_seeds(SplitPosition::ORIGIN);
    let world = CoupledWorld::new(seeds.clone(), 86400.0, 8192);
    for change in 0..3 {
        let mut changed = seeds.clone();
        match change {
            0 => changed[0].origin = changed[0].origin.translate(DVec3::X),
            1 => changed[0].velocity += DVec3::Y,
            _ => changed[0].system.velocities[1] += DVec3::Z,
        }
        let saved = world.checkpoint();
        assert!(
            std::panic::catch_unwind(|| CoupledWorld::from_checkpoint(changed, saved)).is_err()
        );
    }
}

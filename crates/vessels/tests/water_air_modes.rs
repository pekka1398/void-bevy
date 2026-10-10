use glam::DVec3;
use void_frames::State;
use void_vessels::{AirDynamics, Fleet, FleetOptions};
fn make(mode: AirDynamics, sea: bool, air: bool, height: f64) -> (Fleet, String) {
    make_with_ground(mode, sea, air, height, false)
}
fn make_with_ground(
    mode: AirDynamics,
    sea: bool,
    air: bool,
    height: f64,
    ground: bool,
) -> (Fleet, String) {
    let mut planet = void_testkit::earth_size();
    planet.sea_level = sea.then_some(9000.);
    let (e, home) = void_testkit::planet_ephemeris(&planet);
    let env = void_testkit::planet_environment(&planet, &e, home, air);
    let frames = env.frames();
    let at = frames.tree.at(0., &e);
    let transform = at.transform(frames.surface[home], frames.origin);
    let state = transform.apply_state(void_frames::State {
        position: DVec3::X * (planet.terrain.radius_meters + 9000. + height),
        velocity: DVec3::Y * 20.,
    });
    let q = transform.rotation();
    let spin = q * DVec3::Z * 2.;
    let mut craft = void_assembly::fresh_craft();
    craft.parts[0].definition_id = "aero-stabilizer-pod".into();
    let mut fleet = Fleet::new(
        e,
        env,
        0.,
        if ground {
            vec![ground_spec(home)]
        } else {
            vec![]
        },
        FleetOptions {
            air_dynamics: mode,
            ..FleetOptions::default()
        },
    );
    let id = fleet.launch(
        &craft,
        State {
            position: state.position,
            velocity: state.velocity,
        },
        q,
        spin,
    );
    (fleet, id)
}
#[test]
fn nearby_dry_sea_does_not_turn_force_only_air_into_full_air() {
    let (mut plain, p) = make(AirDynamics::ForceOnly, false, true, 5.);
    let (mut sea, s) = make(AirDynamics::ForceOnly, true, true, 5.);
    let (mut full, f) = make(AirDynamics::ForceAndTorque, true, true, 5.);
    for fleet in [&mut plain, &mut sea, &mut full] {
        fleet.advance(0.2);
    }
    let a = plain.snapshot(&p);
    let b = sea.snapshot(&s);
    let c = full.snapshot(&f);
    assert_eq!(sea.water_wrench(&s).force, DVec3::ZERO);
    assert!(
        (a.angular_velocity - b.angular_velocity).length() < 1e-8,
        "force-only spin {:?} vs {:?}",
        a.angular_velocity,
        b.angular_velocity
    );
    assert!(
        (c.angular_velocity - b.angular_velocity).length() > 1e-3,
        "full spin {:?} vs {:?}",
        c.angular_velocity,
        b.angular_velocity
    );
}
#[test]
fn submerged_water_rotation_is_independent_of_air_mode() {
    let (mut only, a) = make(AirDynamics::ForceOnly, true, false, -2.);
    let (mut full, b) = make(AirDynamics::ForceAndTorque, true, false, -2.);
    for fleet in [&mut only, &mut full] {
        fleet.advance(0.1);
    }
    assert!(only.water_wrench(&a).torque.length() > 0.);
    let a = only.snapshot(&a);
    let b = full.snapshot(&b);
    assert!((a.angular_velocity - b.angular_velocity).length() < 1e-10);
    assert!((a.velocity - b.velocity).length() < 1e-10);
}

#[test]
fn wet_scene_does_not_change_remote_dry_scene_cadence_and_restores_at_boundary() {
    let (mut mixed, wet) = make_with_ground(AirDynamics::ForceAndTorque, true, false, -2., true);
    let (dry, dry_only) = make(AirDynamics::ForceAndTorque, true, false, 5.);
    // The antipodal craft requires another Ground scene, outside contact/encounter range.
    let source = dry.snapshot(&dry_only);
    let mut craft = void_assembly::fresh_craft();
    craft.parts[0].definition_id = "aero-stabilizer-pod".into();
    let remote = mixed.launch(
        &craft,
        State {
            position: -source.position,
            velocity: -source.velocity,
        },
        source.rotation,
        source.angular_velocity,
    );
    let (reference, _) = make_with_ground(AirDynamics::ForceAndTorque, true, false, 5., true);
    // Replace the reference launch with the exact antipodal initial conditions.
    let mut reference = Fleet::new(
        void_testkit::planet_ephemeris(&void_testkit::earth_size()).0,
        reference.environment().clone(),
        0.,
        vec![ground_spec(0)],
        reference.options,
    );
    let reference_id = reference.launch(
        &craft,
        State {
            position: -source.position,
            velocity: -source.velocity,
        },
        source.rotation,
        source.angular_velocity,
    );
    mixed.advance(0.1);
    reference.advance(0.1);
    assert!(mixed.water_wrench(&wet).force.length() > 0.);
    assert_eq!(mixed.scene_snapshots().len(), 2);
    let a = mixed.snapshot(&remote);
    let b = reference.snapshot(&reference_id);
    assert!((a.position - b.position).length() < 1e-7);
    assert!((a.velocity - b.velocity).length() < 1e-7);
    assert!((a.angular_velocity - b.angular_velocity).length() < 1e-10);
    let checkpoint = mixed.checkpoint();
    let mut restored = Fleet::from_checkpoint(
        void_testkit::planet_ephemeris(&void_testkit::earth_size()).0,
        mixed.environment().clone(),
        checkpoint,
    );
    assert_eq!(restored.time(), mixed.time());
    for id in mixed.vessel_ids() {
        assert_eq!(
            restored.snapshot(&id).position,
            mixed.snapshot(&id).position
        );
    }
    mixed.advance(0.05);
    restored.advance(0.05);
    for id in mixed.vessel_ids() {
        assert!((restored.snapshot(&id).position - mixed.snapshot(&id).position).length() < 1e-7);
        assert!((restored.snapshot(&id).velocity - mixed.snapshot(&id).velocity).length() < 1e-7);
    }
}

fn ground_spec(body_index: usize) -> void_vessels::GroundSpec {
    void_vessels::GroundSpec {
        body_index,
        band_enter_meters: 10000.,
        band_exit_meters: 11000.,
        tiles: void_landing::ContactWorldOptions {
            step_seconds: 1. / 60.,
            tile_level: void_landing::level_for_tile_size(6371000., 300.),
            tile_resolution: 9,
            tile_reach_meters: 300.,
            tile_keep_meters: 600.,
            recenter_meters: 5000.,
            sleeping: true,
        },
    }
}

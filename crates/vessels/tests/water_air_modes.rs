use glam::DVec3;
use void_landing::FrameState;
use void_vessels::{AirDynamics, Fleet, FleetOptions};
fn make(mode: AirDynamics, sea: bool, air: bool, height: f64) -> (Fleet, String) {
    let mut planet = void_landing::earth_size();
    planet.sea_level = sea.then_some(9000.);
    let (e, home) = void_landing::planet_ephemeris(&planet);
    let env = void_landing::planet_environment(&planet, &e, home, air);
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
        vec![],
        FleetOptions {
            air_dynamics: mode,
            ..FleetOptions::default()
        },
    );
    let id = fleet.launch(
        &craft,
        FrameState {
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

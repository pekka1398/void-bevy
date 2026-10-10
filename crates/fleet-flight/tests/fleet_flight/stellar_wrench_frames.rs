//! Trial convenience loads must name the common owner frame, not an ephemeral environment tree.
use crate::common;
use common::stellar_neighborhood;
use glam::DVec3;
use void_fleet_flight::session::InitialWorld;
use void_frames::State;
#[test]
fn translated_air_load_requires_and_preserves_a_registered_common_frame() {
    let planet = void_testkit::aurelia();
    let mut craft = void_assembly::fresh_craft();
    craft.parts[0].definition_id = "aero-stabilizer-pod".into();
    let mut initial = InitialWorld::new(&planet, &craft, void_testkit::flat_site(&planet), true);
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut sim = initial.build();
    let id = sim.launch_flight_at(
        "Beryl/aurelia",
        &craft,
        State {
            position: DVec3::X * (planet.terrain.radius_meters + 5000.0),
            velocity: DVec3::Y * 80.0,
        },
    );
    let precise = sim.fleet.precise_snapshot(&id);
    let members = sim
        .fleet
        .part_snapshots(&id)
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let air = void_modules::vessel_air_at(
        sim.fleet.environment(),
        sim.fleet.parts(),
        &members,
        DVec3::ZERO,
        precise.residual.rotation,
        sim.fleet.time(),
    )
    .unwrap();
    let mut view = sim.fleet.ephemeris.local_view(precise.system).unwrap();
    view.set_physics_offset(precise.anchor);
    let state = State {
        position: precise.residual.position,
        velocity: precise.residual.velocity,
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| air.wrench(
            view.as_ref(),
            sim.fleet.time(),
            state,
            precise.residual.rotation,
            precise.residual.angular_velocity
        )))
        .is_err()
    );
    let query = sim.fleet.vessel_anchor_frame(&id);
    view.set_physics_query_frame(Some(query));
    let actual = air.wrench(
        view.as_ref(),
        sim.fleet.time(),
        state,
        precise.residual.rotation,
        precise.residual.angular_velocity,
    );
    let expected = air.wrench_in(
        &sim.fleet.frames(),
        query,
        state,
        precise.residual.rotation,
        precise.residual.angular_velocity,
    );
    assert!(sim.fleet.frame_tree().contains(actual.frame));
    assert_eq!(actual, expected);
    view.set_physics_offset(precise.anchor.translate(DVec3::X));
    assert_eq!(
        view.physics_query_frame(),
        None,
        "a changed view must invalidate its frame annotation"
    );
}

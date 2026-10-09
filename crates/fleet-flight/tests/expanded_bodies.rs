use glam::DVec3;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    presentation::ViewCommand,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
    world::expanded_solar_scenery,
};
fn initial() -> InitialWorld {
    let planet = void_landing::aurelia();
    InitialWorld {
        world: expanded_solar_scenery(&planet),
        air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
        launch_body: "aurelia".into(),
        craft: void_vessels::pod_tank("Exploration witness"),
        launch_site: void_vessels::flat_site(&planet),
    }
}
#[test]
fn every_catalog_body_has_explicit_environment_and_focus() {
    let initial = initial();
    let mut session = FlightSession::new(initial.clone());
    assert_eq!(session.sim().fleet.ephemeris.bodies().len(), 58);
    assert_eq!(initial.world.bodies.len(), 58);
    for body in session.sim().fleet.ephemeris.bodies().to_vec() {
        let sim = session.sim();
        assert!(
            sim.fleet
                .ephemeris
                .body_position(body.index, 0.0)
                .is_finite()
        );
        assert!(sim.fleet.environment().body(body.index).is_some());
        assert!(initial.world.bodies.contains_key(&body.id));
        session.execute(Action::View {
            command: ViewCommand::BodyPreset {
                body: body.index,
                direction: DVec3::X,
                distance: body.radius_meters * 3.0,
            },
        });
        let sim = session.sim();
        let sample = sim.presentation.sample(sim);
        assert!(sample.eye.is_finite() && sample.offset.is_finite());
        if !["sol", "velvet", "halo", "azure", "abyss"].contains(&body.id.as_str()) {
            assert!(sim.terrains.contains_key(&body.index));
        }
    }
}
#[test]
fn small_body_orbital_fixture_records_and_restores() {
    let initial = initial();
    let mut session = FlightSession::new(initial.clone()).with_recording();
    for id in ["phobos", "enceladus", "triton", "charon", "bennu", "67p"] {
        let outcome = session.execute(Action::LaunchOrbitAt {
            body: id.into(),
            craft: initial.craft.clone(),
            offset: DVec3::ZERO,
        });
        let Outcome::Spawned(vessel) = outcome else {
            panic!("fixture did not spawn")
        };
        session.execute(Action::Select { vessel });
        let sim = session.sim();
        let body_index = sim.world.body_index(id);
        let body = &sim.fleet.ephemeris.bodies()[body_index];
        let origin = sim
            .fleet
            .frames()
            .transform(
                sim.fleet.body_frames(body_index).0,
                sim.fleet.vessel_anchor_frame(&sim.selected),
            )
            .apply_point(DVec3::ZERO);
        let radius =
            (sim.fleet.precise_snapshot(&sim.selected).residual.position - origin).length();
        let expected = body.radius_meters
            + if body.radius_meters < 1_000_000.0 {
                body.radius_meters * 0.25
            } else {
                400_000.0
            };
        assert!(
            (radius - expected).abs() < 0.01,
            "{id}: {radius} vs {expected}"
        );
        // Existing Laplace navigation SOI for Phobos lies below its physical radius.
        if id != "phobos" {
            assert_eq!(sim.navigation_body(&sim.selected), body_index, "{id}");
        }
        assert!(sim.fleet.snapshot(&sim.selected).position.is_finite());
    }
    session.execute(Action::Advance {
        seconds: 0.02,
        rails: false,
    });
    let replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
    let checkpoint = FlightCheckpoint::capture(session.sim(), initial);
    assert_eq!(world_mark(session.sim()), world_mark(&checkpoint.restore()));
}

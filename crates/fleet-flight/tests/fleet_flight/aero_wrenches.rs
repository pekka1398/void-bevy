//! Force/torque and live deployment state survive direct saves and the same durable action model.
use glam::{DQuat, DVec3};
use void_assembly::fresh_craft;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome, world_mark};
use void_frames::State;
use void_landing::PlanetFrame;
use void_testkit::earth_size;
use void_testkit::flat_site;
fn advance(s: &mut FlightSession, seconds: f64) {
    assert_eq!(
        s.execute(Action::Advance {
            seconds,
            rails: false
        }),
        Outcome::Advanced(true)
    );
}
#[test]
fn checkpoint_rejects_conflicting_air_modes_and_marks_distinguish_them() {
    use void_fleet_flight::checkpoint::FlightCheckpoint;
    use void_vessels::AirDynamics;
    let p = earth_size();
    let c = fresh_craft();
    let initial = InitialWorld::new(&p, &c, flat_site(&p), true)
        .with_air_dynamics(AirDynamics::ForceAndTorque);
    let sim = initial.build();
    let checkpoint = FlightCheckpoint::capture(&sim, initial.clone());
    let mut corrupted = serde_json::to_value(checkpoint).unwrap();
    corrupted["initial"]["air_dynamics"] = serde_json::json!("forceOnly");
    let checkpoint: FlightCheckpoint = serde_json::from_value(corrupted).unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| checkpoint.restore())).is_err()
    );
    let mut sim = sim;
    let full = world_mark(&sim);
    sim.fleet.options.air_dynamics = AirDynamics::ForceOnly;
    assert_ne!(full, world_mark(&sim));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| FlightCheckpoint::capture(
            &sim, initial
        )))
        .is_err()
    );
}
#[test]
fn aerodynamic_wrench_checkpoint_and_recording_continue_exactly() {
    for def in ["aero-stabilizer-pod", "eccentric-chute-pod"] {
        let p = earth_size();
        let mut c = fresh_craft();
        c.parts[0].definition_id = def.into();
        let mut s = FlightSession::new(
            InitialWorld::new(&p, &c, flat_site(&p), true)
                .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque),
        )
        .with_recording();
        let sim = s.sim();
        let state = PlanetFrame::new(&sim.fleet.ephemeris, sim.home).to_inertial(
            &sim.fleet.ephemeris,
            0.0,
            State {
                position: DVec3::X * (p.terrain.radius_meters + 5000.0),
                velocity: DVec3::Y * 80.0,
            },
        );
        assert!(matches!(
            s.execute(Action::LaunchState {
                craft: c,
                position: state.position,
                velocity: state.velocity,
                rotation: DQuat::from_rotation_z(0.2),
                angular_velocity: DVec3::Z * 0.3
            }),
            Outcome::Spawned(_)
        ));
        s.execute(Action::Select {
            vessel: "v2".into(),
        });
        if def == "eccentric-chute-pod" {
            s.execute(Action::Parachute {
                part: "v2/p1".into(),
                module: "parachute1".into(),
                deploy: true,
            });
        }
        advance(&mut s, 0.5);
        s.mark();
        let path = std::env::temp_dir().join(format!(
            "void-aero-wrench-{}-{def}.json",
            std::process::id()
        ));
        s.save_checkpoint(&path);
        let mut restored = FlightSession::load_checkpoint(&path);
        assert_eq!(
            s.sim().fleet.aerodynamic_wrench("v2"),
            restored.sim().fleet.aerodynamic_wrench("v2")
        );
        for session in [&mut s, &mut restored] {
            advance(session, 0.75);
        }
        assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
        s.mark();
        let recording = s.recording();
        let replay = FlightSession::from_recording(recording);
        assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
        std::fs::remove_file(path).unwrap();
    }
}

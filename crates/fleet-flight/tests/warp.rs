use glam::DVec3;
use void_assembly::demo_craft;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
    warp::ManeuverWarp,
};
use void_orbit::{ManeuverSpec, ReferenceMode};
use void_testkit::{flat_site, pod_tank};
fn fixture() -> FlightSession {
    let planet = void_testkit::earth_size();
    let mut s = FlightSession::new(InitialWorld::new(
        &planet,
        &pod_tank("Sleepable pod"),
        flat_site(&planet),
        false,
    ))
    .with_recording();
    s.execute(Action::Advance {
        seconds: 20.0,
        rails: false,
    });
    let Outcome::Spawned(id) = s.execute(Action::LaunchOrbit {
        craft: demo_craft(),
        offset: DVec3::ZERO,
    }) else {
        panic!("launch")
    };
    s.execute(Action::Select { vessel: id });
    s.execute(Action::Stage);
    assert!(
        s.sim().fleet.rails_blocker().is_none(),
        "{:?}",
        s.sim().fleet.rails_blocker()
    );
    let start = s.sim().fleet.time() + 200.037;
    assert_eq!(
        s.execute(Action::AddManeuver {
            spec: ManeuverSpec {
                start_time: start,
                reference_body: s.sim().home,
                reference_mode: ReferenceMode::Fixed,
                prograde: 10.0,
                normal: 0.0,
                radial: 0.0
            }
        }),
        Outcome::Applied
    );
    s
}
#[test]
fn maneuver_approach_clips_large_steps_restores_and_replays_exactly() {
    let mut original = fixture();
    let id = original.sim().selected.clone();
    let start = original.sim().plans[&id].plan.burns()[0].start_time;
    let mass = original.sim().fleet.snapshot(&id).mass_kg;
    assert_eq!(
        original.execute(Action::BeginManeuverWarp),
        Outcome::Applied
    );
    original.execute(Action::Advance {
        seconds: 11.113,
        rails: true,
    });
    assert!(original.sim().maneuver_warp.active());
    original.execute(Action::Select {
        vessel: "v1".into(),
    });
    let checkpoint =
        FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let decoded: FlightCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&checkpoint).unwrap()).unwrap();
    let mut loaded = FlightSession::from_checkpoint(decoded).with_recording();
    let step = Action::Advance {
        seconds: 10_000.0,
        rails: true,
    };
    assert_eq!(original.execute(step.clone()), Outcome::Advanced(true));
    assert_eq!(loaded.execute(step), Outcome::Advanced(true));
    assert_eq!(world_mark(original.sim()), world_mark(loaded.sim()));
    assert_eq!(original.sim().fleet.time(), start - 30.0);
    assert!(!original.sim().maneuver_warp.active());
    assert_eq!(original.sim().fleet.snapshot(&id).mass_kg, mass);
    let replay = FlightSession::from_recording(loaded.recording()).with_recording();
    assert_eq!(world_mark(original.sim()), world_mark(replay.sim()));
}
#[test]
fn manual_edits_and_controls_cancel_the_saved_intent() {
    for action in [
        Action::Control {
            throttle: 0.2,
            turn: DVec3::ZERO,
        },
        Action::RemoveManeuver { index: 0 },
        Action::AbortManeuver,
    ] {
        let mut s = fixture();
        assert_eq!(s.execute(Action::BeginManeuverWarp), Outcome::Applied);
        s.execute(action);
        assert!(matches!(
            s.sim().maneuver_warp,
            ManeuverWarp::Stopped { .. }
        ));
        let replay = FlightSession::from_recording(s.recording()).with_recording();
        assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    }
}
#[test]
fn encounter_stops_approach_without_passing_the_target() {
    let mut s = fixture();
    let target = s.sim().selected.clone();
    let start = s.sim().plans[&target].plan.burns()[0].start_time;
    s.execute(Action::LaunchOrbit {
        craft: pod_tank("Encounter neighbor"),
        offset: DVec3::Y * 2001.0,
    });
    assert_eq!(s.execute(Action::BeginManeuverWarp), Outcome::Applied);
    // The newly encountered physics pair must interrupt rails even when the requested time crosses the burn.
    assert_eq!(
        s.execute(Action::Advance {
            seconds: 1000.0,
            rails: true
        }),
        Outcome::Advanced(false)
    );
    assert!(s.sim().fleet.time() < start - 30.0);
    assert!(
        matches!(&s.sim().maneuver_warp,ManeuverWarp::Stopped {message} if message.contains("encounter"))
    );
}

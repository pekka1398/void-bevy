use crate::common;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, world_mark},
};

fn assert_same_mark(a: &serde_json::Value, b: &serde_json::Value, path: &str) {
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            assert!(a.keys().eq(b.keys()), "{path} object keys differ");
            for (key, value) in a {
                assert_same_mark(value, &b[key], &format!("{path}/{key}"));
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path} array length");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_same_mark(a, b, &format!("{path}/{index}"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}

#[test]
fn distant_coast_checkpoint_and_journal_continue_identically() {
    let (sim, initial) = common::distant_coast();
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1000.0);
    let mut original =
        FlightSession::from_checkpoint(FlightCheckpoint::capture(&sim, initial)).with_recording();
    let action = Action::Advance {
        seconds: 86_400.0,
        rails: true,
    };
    original.execute(action.clone());
    assert_eq!(original.sim().fleet.time(), 86_400.0);
    let checkpoint =
        FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let checkpoint: FlightCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&checkpoint).unwrap()).unwrap();
    let mut session = FlightSession::from_checkpoint(checkpoint).with_recording();
    assert_same_mark(
        &world_mark(original.sim()),
        &world_mark(session.sim()),
        "restore",
    );
    original.execute(action.clone());
    session.execute(action);
    assert_eq!(session.sim().fleet.time(), 172_800.0);
    assert_same_mark(
        &world_mark(original.sim()),
        &world_mark(session.sim()),
        "continue",
    );
    let replay = FlightSession::from_recording(session.recording());
    assert_same_mark(
        &world_mark(session.sim()),
        &world_mark(replay.sim()),
        "replay",
    );
}

#[test]
fn distant_coast_agrees_with_short_chunks_without_losing_local_position() {
    let (mut fast, _) = common::distant_coast();
    let (mut reference, _) = common::distant_coast();
    reference.fleet.options.rails_chunk_seconds = 1.0;
    reference.fleet.options.distant_coast_chunk_seconds = 1.0;
    assert!(fast.advance(3600.0, true).unwrap());
    assert!(reference.advance(3600.0, true).unwrap());
    let a = fast.fleet.precise_snapshot(&fast.selected);
    let b = reference.fleet.precise_snapshot(&reference.selected);
    // 3.6e9 m of travel at 1000 km/s; the 58-body Sol sums more forces per step, so rounding
    // alone measured 2.1e-5 m and 1.8e-8 m/s (about 6e-15 of the distance covered).
    let dp = a.position.relative(&b.position).length();
    let dv = (a.local.velocity - b.local.velocity).length();
    assert!(dp < 5e-5, "distant vs short chunks: {dp:e} m");
    assert!(dv < 5e-8, "distant vs short chunks: {dv:e} m/s");
    assert_eq!(a.local.mass_kg, b.local.mass_kg);
}

#[test]
fn sleeping_home_craft_stays_grounded_during_distant_coast_and_exact_replay() {
    let (sim, initial, ground) = common::distant_coast_with_ground_craft();
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1000.0);
    let target = sim.fleet.time() + sim.fleet.pending_seconds() + 86_400.0;
    let before = sim.fleet.body_fixed_state(&ground, sim.home);
    let resources: Vec<_> = sim
        .fleet
        .part_snapshots(&ground)
        .into_iter()
        .map(|p| (p.id, p.resources))
        .collect();
    let mut original = FlightSession::from_checkpoint(FlightCheckpoint::capture(&sim, initial));
    original.execute(Action::Advance {
        seconds: 86_400.0,
        rails: true,
    });
    assert_eq!(original.sim().fleet.time(), target);
    assert_eq!(
        original.sim().fleet.snapshot(&ground).mode,
        void_vessels::VesselMode::Ground
    );
    assert_eq!(
        original
            .sim()
            .fleet
            .body_fixed_state(&ground, original.sim().home),
        before
    );
    let after_resources: Vec<_> = original
        .sim()
        .fleet
        .part_snapshots(&ground)
        .into_iter()
        .map(|p| (p.id, p.resources))
        .collect();
    assert_eq!(after_resources, resources);
    assert_eq!(original.sim().fleet.rails_coast_chunk_seconds(), 1000.0);
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut restored = FlightSession::from_checkpoint(saved).with_recording();
    let action = Action::Advance {
        seconds: 3600.0,
        rails: true,
    };
    original.execute(action.clone());
    restored.execute(action);
    assert_same_mark(
        &world_mark(original.sim()),
        &world_mark(restored.sim()),
        "mixed continue",
    );
    let replay = FlightSession::from_recording(restored.recording());
    assert_same_mark(
        &world_mark(restored.sim()),
        &world_mark(replay.sim()),
        "mixed replay",
    );
}

#[test]
fn near_orbit_companion_still_keeps_short_chunks_with_a_sleeping_home_craft() {
    let (mut sim, _, _) = common::distant_coast_with_ground_craft();
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1000.0);
    sim.launch_orbital_at(
        sim.home,
        &void_testkit::pod_tank("Near orbit companion"),
        glam::DVec3::ZERO,
    );
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1.0);
}

#[test]
fn ground_control_and_wake_do_not_use_a_distant_chunk() {
    let (mut sim, _, ground) = common::distant_coast_with_ground_craft();
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1000.0);
    sim.fleet.set_control(
        &ground,
        void_vessels::VesselControl {
            throttle: 0.0,
            turn: glam::DVec3::X,
        },
    );
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1.0);
    sim.fleet.advance(1.0 / 60.0);
    assert!(sim.fleet.rails_blocker().is_some());
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1.0);
}

#[test]
fn sleeping_ground_craft_in_another_system_are_covered_by_their_own_body() {
    let (mut sim, _, _) = common::distant_coast_with_ground_craft();
    let body = sim.world.body_index("Beryl/aurelia");
    let site = common::daylight_terrain_site(&sim.world, "Beryl/aurelia").unwrap();
    let remote =
        sim.fleet
            .launch_landed(&void_testkit::pod_tank("Remote sleeping craft"), body, site);
    sim.fleet.advance(30.0);
    assert!(sim.fleet.rails_blocker().is_none());
    assert_eq!(sim.fleet.vessel_system(&remote), void_frames::SystemId(1));
    assert_eq!(sim.fleet.rails_coast_chunk_seconds(), 1000.0);
    let before = sim.fleet.body_fixed_state(&remote, body);
    let target = sim.fleet.time() + sim.fleet.pending_seconds() + 3600.0;
    assert!(sim.advance(3600.0, true).unwrap());
    assert_eq!(sim.fleet.time(), target);
    assert_eq!(sim.fleet.body_fixed_state(&remote, body), before);
    assert_eq!(
        sim.fleet.snapshot(&remote).mode,
        void_vessels::VesselMode::Ground
    );
}

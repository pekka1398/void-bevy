#[path = "../examples/support/interstellar_coast.rs"]
mod coast;
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
    let (sim, initial) = coast::fixture();
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
    let (mut fast, _) = coast::fixture();
    let (mut reference, _) = coast::fixture();
    reference.fleet.options.rails_chunk_seconds = 1.0;
    reference.fleet.options.distant_coast_chunk_seconds = 1.0;
    assert!(fast.advance(3600.0, true).unwrap());
    assert!(reference.advance(3600.0, true).unwrap());
    let a = fast.fleet.precise_snapshot(&fast.selected);
    let b = reference.fleet.precise_snapshot(&reference.selected);
    assert!(a.position.relative(&b.position).length() < 1e-5);
    assert!((a.local.velocity - b.local.velocity).length() < 1e-8);
    assert_eq!(a.local.mass_kg, b.local.mass_kg);
}

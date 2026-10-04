use glam::DVec3;
use std::{fs, process::Command};
use void_fleet_flight::session::{
    Action, FlightSession, InitialWorld, Recording, durable::Recovery, world_mark,
};
fn fixture() -> FlightSession {
    let planet = void_landing::pebble();
    FlightSession::new(InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        false,
    ))
    .with_recording()
}
fn path(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("void-durable-{}-{name}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    directory.join("session.jsonl")
}
#[test]
fn completed_stream_replays_the_same_world_and_does_not_overwrite_files() {
    let path = path("complete");
    let _ = fs::remove_file(&path);
    let mut s = fixture();
    s.execute(Action::Sas { enabled: true });
    s.begin_stream(&path);
    s.execute(Action::Stage);
    s.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    s.mark();
    s.execute(Action::Control {
        throttle: 0.2,
        turn: DVec3::ZERO,
    });
    s.finish_stream();
    let record = Recording::read(&path);
    let replay = FlightSession::from_recording(record).with_recording();
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    let bytes = fs::read(&path).unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.begin_stream(&path))).is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::remove_file(path).unwrap();
}
#[test]
fn crashing_command_survives_in_a_separate_process() {
    const ENV: &str = "VOID_DURABLE_CRASH_TEST_PATH";
    if let Some(path) = std::env::var_os(ENV) {
        let mut s = fixture();
        s.begin_stream(std::path::PathBuf::from(path));
        s.execute(Action::Stage);
        s.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        // A real simulation assertion after a durable intent, before a Commit or normal End.
        s.execute(Action::Control {
            throttle: 2.0,
            turn: DVec3::ZERO,
        });
        panic!("invalid control unexpectedly succeeded");
    }
    let path = path("crash");
    let _ = fs::remove_file(&path);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crashing_command_survives_in_a_separate_process",
            "--nocapture",
        ])
        .env(ENV, &path)
        .output()
        .unwrap();
    assert!(!child.status.success());
    assert!(String::from_utf8_lossy(&child.stderr).contains("invalid control"));
    assert!(std::panic::catch_unwind(|| Recording::read(&path)).is_err());
    let recovered = Recovery::read(&path);
    assert!(!recovered.ended_normally);
    assert_eq!(recovered.discarded_tail_bytes, 0);
    assert_eq!(recovered.recording.entries.len(), 2);
    let pending = recovered.pending.as_ref().unwrap();
    assert_eq!(pending.index, 2);
    assert!(matches!(
        pending.action,
        Action::Control { throttle: 2.0, .. }
    ));
    let mut expected = fixture();
    expected.execute(Action::Stage);
    expected.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    let replay = FlightSession::from_recording(recovered.recording).with_recording();
    assert_eq!(world_mark(expected.sim()), world_mark(replay.sim()));
    fs::remove_file(path).unwrap();
}
#[test]
fn torn_eof_requires_explicit_recovery_and_retains_the_pending_intent() {
    let path = path("tail");
    let _ = fs::remove_file(&path);
    let mut s = fixture();
    s.begin_stream(&path);
    s.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    s.finish_stream();
    let bytes = fs::read(&path).unwrap();
    let commit = bytes
        .windows(b"{\"kind\":\"Commit\"".len())
        .position(|w| w == b"{\"kind\":\"Commit\"")
        .unwrap();
    fs::write(&path, &bytes[..commit + 8]).unwrap();
    assert!(std::panic::catch_unwind(|| Recording::read(&path)).is_err());
    let recovered = Recovery::read(&path);
    assert_eq!(recovered.discarded_tail_bytes, 8);
    assert!(matches!(
        recovered.pending.unwrap().action,
        Action::Advance { seconds: 0.113, .. }
    ));
    assert!(recovered.recording.entries.is_empty());
    assert_eq!(
        world_mark(fixture().sim()),
        world_mark(
            FlightSession::from_recording(recovered.recording)
                .with_recording()
                .sim()
        )
    );
    fs::remove_file(path).unwrap();
}
#[test]
fn malformed_complete_lines_are_never_discarded_as_a_crash_tail() {
    let path = path("corrupt");
    let _ = fs::remove_file(&path);
    let mut s = fixture();
    s.begin_stream(&path);
    s.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    s.finish_stream();
    let text = fs::read_to_string(&path).unwrap().replace(
        "\"kind\":\"Commit\",\"index\":0",
        "\"kind\":\"Commit\",\"index\":9",
    );
    fs::write(&path, text).unwrap();
    assert!(std::panic::catch_unwind(|| Recovery::read(&path)).is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn reset_and_foreign_world_load_stay_in_the_same_stream_and_save_the_current_world() {
    let path = path("replacement");
    let _ = fs::remove_file(&path);
    let mut s = fixture();
    s.begin_stream(&path);
    s.execute(Action::Stage);
    let initial = s.recording_initial().clone();
    s.execute(Action::ResetWorld {
        initial: Box::new(initial),
    });
    s.mark();
    let planet = void_landing::moon_size();
    let foreign = FlightSession::new(InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        false,
    ))
    .with_recording();
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        foreign.sim(),
        foreign.recording_initial().clone(),
    );
    s.execute(Action::LoadWorld {
        checkpoint: Box::new(checkpoint),
    });
    s.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    s.finish_stream();
    let replay = FlightSession::from_recording(Recording::read(&path)).with_recording();
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    assert_eq!(replay.recording_initial().launch_body, planet.body_id);
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        replay.sim(),
        replay.recording_initial().clone(),
    );
    assert_eq!(world_mark(replay.sim()), world_mark(&saved.restore()));
    fs::remove_file(path).unwrap();
}
#[test]
fn pretty_printed_portable_recordings_remain_readable() {
    let path = path("pretty");
    let mut s = fixture();
    s.execute(Action::Stage);
    fs::write(&path, serde_json::to_string_pretty(&s.recording()).unwrap()).unwrap();
    let replay = FlightSession::from_recording(Recording::read(&path)).with_recording();
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    fs::remove_file(path).unwrap();
}

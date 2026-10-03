//! The shipped game binary must expose verification before any window or renderer is created.
use std::process::Command;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld};

#[test]
fn main_game_verifies_direct_world_saves_and_recordings_without_a_window() {
    let planet = void_landing::pebble();
    let craft = void_assembly::demo_craft();
    let mut session = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        void_vessels::flat_site(&planet),
        false,
    ))
    .with_recording();
    session.execute(Action::LaunchOrbit {
        craft,
        offset: glam::DVec3::ZERO,
    });
    session.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    let directory =
        std::env::temp_dir().join(format!("void-main-fleet-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let save = directory.join("world.json");
    let record = directory.join("recording.json");
    session.save_checkpoint(&save);
    session.save(&record);
    for (flag, path, message) in [
        ("--verify-save", &save, "Verified Fleet world save"),
        ("--verify", &record, "Verified Fleet session"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_void-app"))
            .arg(flag)
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(message));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovery_is_an_explicit_no_window_command_and_retains_uncommitted_input() {
    let planet = void_landing::pebble();
    let craft = void_assembly::demo_craft();
    let mut session = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        void_vessels::flat_site(&planet),
        false,
    ))
    .with_recording();
    let directory = std::env::temp_dir().join(format!("void-main-recovery-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("crash.jsonl");
    let output = directory.join("recovered.json");
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    let report = directory.join("recovered.json.recovery.json");
    let _ = std::fs::remove_file(&report);
    session.begin_stream(&input);
    session.execute(Action::Stage);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.execute(
            Action::Control {
                throttle: 2.0,
                turn: glam::DVec3::ZERO
            }
        )))
        .is_err()
    );
    drop(session);
    let before = std::fs::read(&input).unwrap();
    let recovered = Command::new(env!("CARGO_BIN_EXE_void-app"))
        .arg("--recover-recording")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert!(String::from_utf8_lossy(&recovered.stdout).contains("pending command true"));
    assert_eq!(before, std::fs::read(&input).unwrap());
    let verified = Command::new(env!("CARGO_BIN_EXE_void-app"))
        .arg("--verify")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let details: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    assert_eq!(details["pending"]["action"]["throttle"], 2.0);
    std::fs::remove_dir_all(directory).unwrap();
}

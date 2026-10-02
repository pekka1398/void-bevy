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
    ));
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

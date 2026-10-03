//! Native GPU validation, explicitly opt-in. These children disable Winit and render only to Image.
use std::process::Command;
#[test]
#[ignore = "requires a native rendering adapter; offscreen, no OS window"]
fn main_scene_renders_then_reuses_its_world_without_reapplying_the_preset() {
    let dir = std::env::temp_dir().join(format!("void-render-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.json");
    let output = Command::new(env!("CARGO_BIN_EXE_void-app"))
        .args(["--render-benchmark"])
        .arg(&first)
        .args([
            "--benchmark-scenario",
            "orbit",
            "--benchmark-frames",
            "4",
            "--benchmark-settle",
            "20",
            "--width",
            "320",
            "--height",
            "180",
        ])
        .output()
        .expect("start offscreen benchmark");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let a: serde_json::Value = serde_json::from_slice(&std::fs::read(&first).unwrap()).unwrap();
    assert_eq!(a["capture"]["delivered_frames"], 4);
    assert_eq!(a["benchmark"]["main_game_shaders"], true);
    assert_eq!(a["benchmark"]["pending_tiles_at_run"], 0);
    assert_eq!(a["render_errors"], serde_json::json!([]));
    assert!(
        a["capture"]["metrics"]
            .get("render/ui/elapsed_cpu")
            .is_some(),
        "the offscreen main camera must also render the HUD and navball"
    );
    assert!(
        a["capture"]["metrics"]
            .get("render/void_air/elapsed_cpu")
            .is_some()
    );
    if a["adapter"]["timestamp_queries"] == true {
        assert!(
            a["capture"]["metrics"]
                .get("render/void_air/elapsed_gpu")
                .is_some()
        );
    }
    if cfg!(feature = "render-metrics") {
        assert!(
            a["capture"]["metrics"]["render/submissions/covered_draw_records"]["min"]
                .as_f64()
                .unwrap()
                > 0.0
        );
    }
    let world = first.with_extension("world.json");
    let saved = void_fleet_flight::session::FlightSession::load_checkpoint(&world);
    assert_eq!(saved.sim().fleet.vessel_ids().len(), 2);
    let second = dir.join("second.json");
    let output = Command::new(env!("CARGO_BIN_EXE_void-app"))
        .arg("--render-benchmark")
        .arg(&second)
        .arg("--load")
        .arg(&world)
        .args([
            "--benchmark-frames",
            "4",
            "--benchmark-settle",
            "20",
            "--width",
            "320",
            "--height",
            "180",
        ])
        .output()
        .expect("start saved-scene benchmark");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let b: serde_json::Value = serde_json::from_slice(&std::fs::read(&second).unwrap()).unwrap();
    assert_eq!(b["benchmark"]["scene_source"], "saved checkpoint");
    let restored = void_fleet_flight::session::FlightSession::load_checkpoint(
        second.with_extension("world.json"),
    );
    assert_eq!(
        void_fleet_flight::session::world_mark(saved.sim()),
        void_fleet_flight::session::world_mark(restored.sim())
    );
    assert_eq!(b["capture"]["delivered_frames"], 4);
    std::fs::remove_dir_all(&dir).unwrap();
}

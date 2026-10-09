//! Build controlled camera-only checkpoints using the same Zoom/Exposure commands as the main GUI.
//! Arguments: BASE_CHECKPOINT OUTPUT_DIRECTORY. Physics and the fixed view direction are preserved.
use glam::{DQuat, DVec3};
use std::path::Path;
use void_fleet_flight::{
    presentation::ViewCommand,
    session::{Action, FlightSession, world_mark},
};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 2, "BASE_CHECKPOINT OUTPUT_DIRECTORY");
    std::fs::create_dir_all(&args[1]).unwrap();
    for exposure in [6.31, 20.0] {
        for km in (60..=72).step_by(2) {
            let mut session = FlightSession::load_checkpoint(&args[0]);
            let sim = session.sim();
            let mut before = world_mark(sim);
            before.as_object_mut().unwrap().remove("presentation");
            let body = sim.world.body_index("vesper");
            assert!(sim.presentation.main_camera && sim.presentation.focus_body.is_none());
            let sample = sim.presentation.sample(sim);
            let surface = sim.fleet.body_frames(body).1;
            let frames = sim.fleet.frames();
            let focus = frames
                .transform(sample.focus_frame, surface)
                .apply_point(sample.focus_local);
            let direction = frames
                .transform(sim.fleet.origin_frame(), surface)
                .apply_direction(sample.offset.normalize());
            let radius = sim.fleet.ephemeris.bodies()[body].radius_meters;
            let target = radius + f64::from(km) * 1000.0;
            let b = focus.dot(direction);
            let distance = -b + (b * b + target * target - focus.length_squared()).sqrt();
            let pixels = -(distance / sim.presentation.distance).ln() / 0.002;
            session.execute(Action::View {
                command: ViewCommand::Zoom { pixels },
            });
            session.execute(Action::View {
                command: ViewCommand::Exposure { value: exposure },
            });
            let sim = session.sim();
            // world_mark includes presentation, so compare the physical fleet witness separately.
            let mut after = world_mark(sim);
            after.as_object_mut().unwrap().remove("presentation");
            assert_eq!(before, after);
            let actual = sim
                .presentation
                .sample(sim)
                .to_camera(&sim.fleet, surface, DQuat::IDENTITY)
                .apply_point(DVec3::ZERO)
                .length()
                - radius;
            assert!(
                (actual - f64::from(km) * 1000.0).abs() < 1e-5,
                "camera altitude {actual}"
            );
            let name = format!("cloud-{exposure:.2}-{km}.json");
            session.save_checkpoint(Path::new(&args[1]).join(&name));
            println!(
                "{name} eye_m={actual:.6} distance_m={distance:.6} time={:.6}",
                sim.fleet.time()
            );
        }
    }
}

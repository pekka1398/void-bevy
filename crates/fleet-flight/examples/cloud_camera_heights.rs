//! Read true frame-tree camera altitude after each ordinary journalled Zoom command.
use glam::{DQuat, DVec3};
use void_fleet_flight::{
    presentation::ViewCommand,
    session::{Action, FlightSession, Recording},
};
fn main() {
    let path = std::env::args().nth(1).expect("journal path");
    let recording = Recording::read(path);
    let mut session = recording.base.map_or_else(
        || FlightSession::new(recording.initial),
        FlightSession::from_checkpoint,
    );
    let mut zoom = 0;
    for entry in recording.entries {
        let is_zoom = matches!(
            entry.action,
            Action::View {
                command: ViewCommand::Zoom { .. }
            }
        );
        session.execute(entry.action);
        if is_zoom {
            zoom += 1;
            let sim = session.sim();
            let body = sim.world.body_index("vesper");
            let sample = sim.presentation.sample(sim);
            let into = sample.to_camera(&sim.fleet, sim.fleet.body_frames(body).1, DQuat::IDENTITY);
            let altitude = into.apply_point(DVec3::ZERO).length()
                - sim.fleet.ephemeris.bodies()[body].radius_meters;
            println!("{zoom} {altitude:.6} {:.6}", sim.presentation.distance);
        }
    }
}

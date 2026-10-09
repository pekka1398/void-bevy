#[path = "support/interstellar_coast.rs"]
mod coast;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: interstellar_coast_fixture <new-checkpoint-path>");
    assert!(
        !std::path::Path::new(&path).exists(),
        "fixture path already exists"
    );
    let (sim, initial) = coast::fixture();
    void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial).write(&path);
    void_fleet_flight::checkpoint::FlightCheckpoint::read(&path).restore();
    println!(
        "Wrote paused, declared 2-light-year cruise starting state to {path}; not a completed interstellar trip."
    );
}

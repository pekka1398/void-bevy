#[path = "support/interstellar_coast.rs"]
mod coast;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: interstellar_coast_fixture <new-checkpoint-path> [--mixed-ground]");
    assert!(
        !std::path::Path::new(&path).exists(),
        "fixture path already exists"
    );
    assert!(
        std::env::args().nth(3).is_none(),
        "unexpected fixture argument"
    );
    let (sim, initial) = match std::env::args().nth(2).as_deref() {
        Some("--mixed-ground") => {
            let (sim, initial, _) = coast::mixed_fixture();
            (sim, initial)
        }
        None => coast::fixture(),
        Some(argument) => panic!("unsupported fixture option: {argument}"),
    };
    void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial).write(&path);
    void_fleet_flight::checkpoint::FlightCheckpoint::read(&path).restore();
    println!(
        "Wrote paused, declared 2-light-year cruise starting state to {path}; not a completed interstellar trip."
    );
}

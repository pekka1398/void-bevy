//! Times 100 days of ephemeris after 10 days of warm-up, as the TS comparison does.
//! cargo run --release -p void-orbit --example ephemeris_speed

use std::time::Instant;

use void_orbit::{Ephemeris, EphemerisOptions, SystemSpec, build_system, suggested_step_seconds};

fn main() {
    for id in ["binary", "sol"] {
        let path = format!("{}/systems/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let system = build_system(&SystemSpec::from_json(&std::fs::read_to_string(&path).expect(&path)));
        let step_seconds = suggested_step_seconds(&system.bodies, 256.0);
        let mut ephemeris = Ephemeris::new(&system, EphemerisOptions { step_seconds, chunk_steps: 2048 });
        ephemeris.extend_to(10.0 * 86_400.0);
        let started = Instant::now();
        ephemeris.extend_to(110.0 * 86_400.0);
        println!("{id}: 100 days in {:.0} ms", started.elapsed().as_secs_f64() * 1e3);
    }
}

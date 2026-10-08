//! Diagnose the exact main-game sea fixture without any renderer or recording overhead.
use std::time::Instant;
use void_fleet_flight::session::{Action, FlightSession, Recording};
fn main() {
    let path = std::env::args().nth(1).expect("recording path");
    let recording = Recording::read(path);
    let mut session = FlightSession::new(recording.initial);
    let id = void_fleet_flight::water::splashdown(&mut session);
    println!("time_s,advance_ms,sea_height_m,speed_m_s,water_force_n,stable_dt_ms,predict_ms");
    for i in 0..240 {
        let started = Instant::now();
        session.execute(Action::Advance {
            seconds: 0.05,
            rails: false,
        });
        let advance_ms = started.elapsed().as_secs_f64() * 1000.0;
        let sim = session.sim();
        let state = sim.fleet.body_fixed_state(&id, sim.home);
        let sea = sim
            .fleet
            .environment()
            .body(sim.home)
            .unwrap()
            .sea_level_meters
            .unwrap();
        let height = state.position.length()
            - sim.fleet.environment().bodies()[sim.home].radius_meters
            - sea;
        let force = sim.fleet.water_wrench(&id).force.length();
        let dt = sim.fleet.diagnostic_water_step_seconds();
        let time = sim.fleet.time();
        let speed = state.velocity.length();
        let predict_ms = if i % 40 == 0 {
            let start = Instant::now();
            let _ = session.predict(6000.0);
            start.elapsed().as_secs_f64() * 1000.0
        } else {
            0.0
        };
        println!(
            "{time:.6},{advance_ms:.6},{height:.6},{speed:.6},{force:.6},{:.6},{predict_ms:.6}",
            dt * 1000.0
        );
    }
}

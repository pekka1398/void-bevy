//! Diagnose the exact main-game sea fixture without any renderer or recording overhead.
use std::time::Instant;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Recording};
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("recording path or --initial-world");
    // An explicitly exported world recipe permits comparing simulation versions without
    // treating an old command journal as compatible with the new simulation rules.
    let initial = if path == "--initial-world" {
        let path = args.next().expect("initial world JSON path");
        serde_json::from_str::<InitialWorld>(&std::fs::read_to_string(path).unwrap()).unwrap()
    } else {
        Recording::read(path).initial
    };
    let mut session = FlightSession::new(initial);
    let speed = args.next().map_or(2., |s| s.parse::<f64>().unwrap());
    let tilt = args
        .next()
        .map_or(0., |s| s.parse::<f64>().unwrap())
        .to_radians();
    let entry = args
        .next()
        .map_or(0., |s| s.parse::<f64>().unwrap())
        .to_radians();
    let id = void_fleet_flight::water::splashdown_at(
        &mut session,
        &void_assembly::reentry_capsule(),
        speed,
        tilt,
        entry,
    );
    println!("time_s,advance_ms,sea_height_m,speed_m_s,water_force_n,scene_dt_ms,predict_ms");
    #[cfg(feature = "step-timing")]
    let mut previous_phases = [0.0; 6];
    for i in 0..240 {
        let started = Instant::now();
        session.execute(Action::Advance {
            seconds: 0.05,
            rails: false,
        });
        let advance_ms = started.elapsed().as_secs_f64() * 1000.0;
        #[cfg(feature = "step-timing")]
        {
            let phases = session.sim().fleet.step_timings();
            eprintln!(
                "phase {:?}",
                std::array::from_fn::<_, 6, _>(|i| phases[i] - previous_phases[i])
            );
            previous_phases = phases;
        }
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
        let dt = sim.fleet.options.step_seconds;
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

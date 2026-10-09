//! Real main-game 58-body planning lifecycle, without a renderer. Run in release mode.
use glam::DVec3;
use std::time::{Duration, Instant};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome};
use void_orbit::{NavigationOperation, NavigationRequest};
fn main() {
    let capture = std::env::args().any(|a| a == "capture");
    let cancel = std::env::args().any(|a| a == "cancel");
    let planet = void_landing::aurelia();
    let craft = void_assembly::flight_rocket();
    let mut initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), false);
    initial.world = void_fleet_flight::world::expanded_solar_scenery(&planet);
    let mut session = FlightSession::new(initial);
    if let Ok(path) = std::env::var("VOID_NAV_JOURNAL") {
        session.begin_stream(path);
    }
    let Outcome::Spawned(id) = session.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::ZERO,
    }) else {
        panic!("launch")
    };
    session.execute(Action::Select { vessel: id });
    session.execute(Action::Stage);
    if std::env::args().any(|a| a == "upper-stage") {
        session.execute(Action::Stage);
    }
    session.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    if let Ok(path) = std::env::var("VOID_NAV_ACCEPTANCE_SAVE") {
        session.save_checkpoint(path);
    }
    let bodies = session.sim().fleet.ephemeris.bodies().len();
    let target = if capture {
        session.sim().home
    } else {
        session.sim().world.body_index("selene")
    };
    let request = NavigationRequest {
        operation: if capture {
            NavigationOperation::Capture
        } else {
            NavigationOperation::Departure
        },
        target_body: target,
        reference_body: session.sim().home,
        earliest_departure: 30.,
        latest_departure: if capture { 30. } else { 30. + 30. * 86400. },
        min_flight_seconds: 60.,
        max_flight_seconds: if capture { 12000. } else { 7. * 86400. },
        periapsis_altitude_m: if capture { 400000. } else { 100000. },
    };
    let mass = session
        .sim()
        .fleet
        .snapshot(&session.sim().selected)
        .mass_kg;
    let start = Instant::now();
    session.request_navigation(request, true).unwrap();
    let snapshot_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut idle_poll_max_ms = 0f64;
    let mut completion_ms = 0f64;
    let mut polls = 0;
    let mut cancellation = None;
    let mut outcome = None;
    while session.navigation_running() {
        if cancel && cancellation.is_none() && start.elapsed() > Duration::from_millis(50) {
            cancellation = Some(Instant::now());
            session.cancel_navigation("profile cancellation");
        }
        let tick = Instant::now();
        let result = session.poll_navigation(true);
        let elapsed = tick.elapsed().as_secs_f64() * 1000.;
        if result.is_some() {
            completion_ms = elapsed;
            outcome = result;
        } else {
            idle_poll_max_ms = idle_poll_max_ms.max(elapsed);
        }
        session.execute(Action::EndFrame {
            paused: true,
            rate: 0,
        });
        polls += 1;
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "worker failed to finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Ok(path) = std::env::var("VOID_NAV_RESULT_SAVE") {
        session.save_checkpoint(path);
    }
    assert_eq!(
        mass,
        session
            .sim()
            .fleet
            .snapshot(&session.sim().selected)
            .mass_kg
    );
    println!(
        "{}",
        serde_json::json!({"bodies":bodies,"snapshot_ms":snapshot_ms,"elapsed_seconds":start.elapsed().as_secs_f64(),"idle_poll_max_ms":idle_poll_max_ms,"completion_ms":completion_ms,"polls":polls,"cancel_latency_ms":cancellation.map(|t| t.elapsed().as_secs_f64()*1000.),"outcome":outcome,"status":session.navigation_status(),"live_ephemeris_bytes":session.sim().fleet.ephemeris.retained_bytes()})
    );
    if session.streaming() {
        session.finish_stream();
    }
}

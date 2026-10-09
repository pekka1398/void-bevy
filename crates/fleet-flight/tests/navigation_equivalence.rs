//! Background delivery must preserve the synchronous physical plan, not just a success flag.
use glam::DVec3;
use std::time::{Duration, Instant};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome};
use void_orbit::{NavigationOperation, NavigationRequest};

#[test]
fn background_and_synchronous_capture_have_identical_accepted_trajectory() {
    let planet = void_landing::earth_size();
    let craft = void_assembly::demo_craft();
    let mut background = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        void_vessels::flat_site(&planet),
        false,
    ))
    .with_recording();
    let Outcome::Spawned(id) = background.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::ZERO,
    }) else {
        panic!("launch")
    };
    background.execute(Action::Select { vessel: id.clone() });
    background.execute(Action::Stage);
    background.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    let mut synchronous = FlightSession::from_recording(background.recording());
    let request = NavigationRequest {
        operation: NavigationOperation::Capture,
        target_body: background.sim().home,
        reference_body: background.sim().home,
        earliest_departure: 100.0,
        latest_departure: 100.0,
        min_flight_seconds: 100.0,
        max_flight_seconds: 12000.0,
        periapsis_altitude_m: 400000.0,
    };
    assert_eq!(
        synchronous.execute(Action::GenerateNavigation {
            request
        }),
        Outcome::Applied
    );
    background.request_navigation(request, true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut outcome = None;
    while background.navigation_running() {
        if let Some(result) = background.poll_navigation(true) {
            outcome = Some(result);
        }
        assert!(Instant::now() < deadline, "background completion timed out");
        std::thread::yield_now();
    }
    assert_eq!(outcome, Some(Outcome::Applied));
    let a = &synchronous.sim().plans[&id].plan;
    let b = &background.sim().plans[&id].plan;
    assert_eq!(a.burns(), b.burns());
    assert_eq!(a.count(), b.count());
    for i in 0..a.count() {
        assert_eq!(a.maneuver(i), b.maneuver(i));
    }
    assert_eq!(a.trajectory.count(), b.trajectory.count());
    for i in 0..a.trajectory.count() {
        assert_eq!(
            a.trajectory.time(i).to_bits(),
            b.trajectory.time(i).to_bits()
        );
        assert_eq!(
            a.trajectory.position(i).to_array().map(f64::to_bits),
            b.trajectory.position(i).to_array().map(f64::to_bits)
        );
        assert_eq!(
            a.trajectory.velocity(i).to_array().map(f64::to_bits),
            b.trajectory.velocity(i).to_array().map(f64::to_bits)
        );
    }
    assert_eq!(
        synchronous.sim().fleet.snapshot(&id).mass_kg.to_bits(),
        background.sim().fleet.snapshot(&id).mass_kg.to_bits()
    );
}

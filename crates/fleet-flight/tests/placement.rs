//! DEV place-ship: the selected vessel moves to a declared state on the normal main-game world,
//! through the journal, so replay and checkpoints reproduce it.
use glam::DVec3;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    placement::{Placement, PlacementAttitude, PlacementVelocity, SiteKind},
    session::{Action, FlightSession, Outcome, world_mark},
    world::main_game,
};
use void_vessels::VesselMode;

fn session(craft: &void_assembly::Craft) -> FlightSession {
    FlightSession::new(main_game(craft)).with_recording()
}
fn advance(session: &mut FlightSession, seconds: f64) {
    let steps = (seconds / 0.25).ceil() as usize;
    for _ in 0..steps {
        assert_eq!(
            session.execute(Action::Advance {
                seconds: 0.25,
                rails: false
            }),
            Outcome::Advanced(true)
        );
    }
}
fn altitude_and_speed(session: &FlightSession) -> (f64, f64) {
    let sim = session.sim();
    let body = sim.world.body_index("aurelia");
    let state = sim.fleet.body_fixed_state(&sim.selected, body);
    (
        state.position.length() - sim.fleet.ephemeris.bodies()[body].radius_meters,
        state.velocity.length(),
    )
}

#[test]
fn circular_orbit_placement_stays_in_orbit_and_replays() {
    let mut s = session(&void_assembly::rcs_flight_rocket());
    let mut placement = Placement {
        body: "aurelia".into(),
        latitude_degrees: 0.0,
        longitude_degrees: 30.0,
        altitude_meters: 400_000.0,
        velocity: PlacementVelocity::Orbital {
            speed: 0.0,
            heading_degrees: 90.0,
            flight_path_degrees: 0.0,
        },
        attitude: PlacementAttitude::Prograde,
    };
    let speed = s.sim().circular_speed(&placement).unwrap();
    placement.velocity = PlacementVelocity::Orbital {
        speed,
        heading_degrees: 90.0,
        flight_path_degrees: 0.0,
    };
    assert_eq!(s.execute(Action::Place { placement }), Outcome::Applied);
    assert_eq!(
        s.sim().fleet.snapshot(&s.sim().selected).mode,
        VesselMode::Orbit
    );
    // Altitude is measured above the surface below the site: here the sea.
    let (altitude, _) = altitude_and_speed(&s);
    let expected = 400_000.0 + void_terrain::SEA_LEVEL;
    assert!((altitude - expected).abs() < 1.0, "{altitude}");
    advance(&mut s, 20.0);
    let (later, _) = altitude_and_speed(&s);
    assert!((later - altitude).abs() < 2_000.0, "{altitude} -> {later}");
    let mark = world_mark(s.sim());
    assert_eq!(
        world_mark(FlightSession::from_recording(s.recording()).sim()),
        mark
    );
    let mut loaded = FlightSession::from_checkpoint(FlightCheckpoint::capture(
        s.sim(),
        s.recording_initial().clone(),
    ));
    for session in [&mut s, &mut loaded] {
        advance(session, 0.5);
    }
    assert_eq!(world_mark(loaded.sim()), world_mark(s.sim()));
}

#[test]
fn ocean_drop_floats_on_the_main_world_sea() {
    let mut s = session(&void_assembly::reentry_capsule());
    let (latitude, longitude) = s.sim().daylight_site("aurelia", SiteKind::Ocean).unwrap();
    let placement = Placement {
        body: "aurelia".into(),
        latitude_degrees: latitude,
        longitude_degrees: longitude,
        altitude_meters: 8.0,
        velocity: PlacementVelocity::Surface {
            speed: 2.0,
            heading_degrees: 0.0,
            flight_path_degrees: -90.0,
        },
        attitude: PlacementAttitude::Upright,
    };
    assert_eq!(s.execute(Action::Place { placement }), Outcome::Applied);
    advance(&mut s, 30.0);
    let (altitude, speed) = altitude_and_speed(&s);
    let sea = void_terrain::SEA_LEVEL;
    assert!((altitude - sea).abs() < 5.0, "altitude {altitude}");
    assert!(speed < 1.0, "speed {speed}");
    assert!(s.sim().fleet.water_wrench(&s.sim().selected).force.length() > 0.0);
}

#[test]
fn landed_placement_rests_on_daylight_land() {
    let mut s = session(&void_assembly::rcs_flight_rocket());
    let (latitude, longitude) = s.sim().daylight_site("aurelia", SiteKind::Land).unwrap();
    let placement = Placement {
        body: "aurelia".into(),
        latitude_degrees: latitude,
        longitude_degrees: longitude,
        altitude_meters: 0.0,
        velocity: PlacementVelocity::Landed,
        attitude: PlacementAttitude::Upright,
    };
    assert_eq!(s.execute(Action::Place { placement }), Outcome::Applied);
    advance(&mut s, 5.0);
    let (_, speed) = altitude_and_speed(&s);
    assert!(speed < 0.5, "speed {speed}");
    assert_eq!(
        s.sim().fleet.snapshot(&s.sim().selected).mode,
        VesselMode::Ground
    );
}

#[test]
fn place_near_target_matches_its_motion_and_refusals_leave_the_world() {
    let craft = void_assembly::rcs_flight_rocket();
    let mut s = session(&craft);
    let Outcome::Spawned(target) = s.execute(Action::LaunchOrbit {
        craft: craft.clone(),
        offset: DVec3::ZERO,
    }) else {
        panic!("orbit launch")
    };
    let before = world_mark(s.sim());
    let refused = s.execute(Action::Place {
        placement: Placement {
            body: "aurelia".into(),
            latitude_degrees: 0.0,
            longitude_degrees: 0.0,
            altitude_meters: 0.5,
            velocity: PlacementVelocity::Surface {
                speed: 0.0,
                heading_degrees: 0.0,
                flight_path_degrees: 0.0,
            },
            attitude: PlacementAttitude::Upright,
        },
    });
    assert!(matches!(refused, Outcome::Refused(_)), "{refused:?}");
    assert_eq!(world_mark(s.sim()), before);
    assert!(matches!(
        s.execute(Action::PlaceNear {
            target: s.sim().selected.clone(),
            gap_meters: 5.0
        }),
        Outcome::Refused(_)
    ));
    assert_eq!(
        s.execute(Action::PlaceNear {
            target: target.clone(),
            gap_meters: 5.0
        }),
        Outcome::Applied
    );
    let sim = s.sim();
    let relative = sim.fleet.relative(&target, &sim.selected);
    let distance = relative.position.length();
    assert!(distance > 5.0 && distance < 60.0, "{distance}");
    assert!(relative.velocity.length() < 1e-3, "{:?}", relative.velocity);
    let a = sim.fleet.snapshot(&sim.selected).rotation * DVec3::Y;
    let b = sim.fleet.snapshot(&target).rotation * DVec3::Y;
    assert!(a.dot(b) < -0.999, "noses face each other");
    advance(&mut s, 2.0);
    let sim = s.sim();
    let later = sim.fleet.relative(&target, &sim.selected).position.length();
    assert!((later - distance).abs() < 0.05, "{distance} -> {later}");
    let mark = world_mark(s.sim());
    assert_eq!(
        world_mark(FlightSession::from_recording(s.recording()).sim()),
        mark
    );
}

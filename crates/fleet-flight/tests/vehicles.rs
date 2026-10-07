use glam::DVec3;
use void_assembly::{ModuleState, VehicleControl, rover};
use void_fleet_flight::{
    FleetFlight,
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, world_mark},
};
use void_vessels::flat_site;
fn make() -> FlightSession {
    let planet = void_landing::earth_size();
    FlightSession::new(InitialWorld::new(
        &planet,
        &rover(),
        flat_site(&planet),
        false,
    ))
    .with_recording()
}
fn advance(s: &mut FlightSession, seconds: f64) {
    s.execute(Action::Advance {
        seconds,
        rails: false,
    });
}
#[test]
fn grounded_rover_settles_drives_brakes_and_persists_real_contact_state() {
    let mut s = make();
    advance(&mut s, 5.0);
    let sim = s.sim();
    let id = sim.selected.clone();
    let p = sim.fleet.body_fixed_state(&id, sim.home);
    let grounded = sim
        .fleet
        .parts()
        .parts()
        .filter_map(|p| {
            p.modules.values().find_map(|m| match m {
                ModuleState::Wheel { state, .. } => Some(state.grounded),
                _ => None,
            })
        })
        .filter(|x| *x)
        .count();
    println!("settle velocity {:?}, grounded {grounded}", p.velocity);
    assert!(grounded >= 3);
    assert!(p.velocity.length() < 0.5);
    s.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 1.0,
            steer: 0.0,
            brake: 0.0,
        },
    });
    advance(&mut s, 3.0);
    let sim = s.sim();
    let moving = sim.fleet.body_fixed_state(&id, sim.home);
    println!(
        "drive velocity {:?}, movement {}, snapshot {:?}",
        moving.velocity,
        (moving.position - p.position).length(),
        sim.fleet.snapshot(&id)
    );
    assert!((moving.position - p.position).length() > 2.0);
    assert!(moving.velocity.length() > 1.0);
    let saved = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let saved: FlightCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut restored = FlightSession::from_checkpoint(saved).with_recording();
    assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    for action in [
        Action::Vehicle {
            control: VehicleControl::default(),
        },
        Action::Advance {
            seconds: 3.0,
            rails: false,
        },
    ] {
        assert_eq!(s.execute(action.clone()), restored.execute(action));
        assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    }
    let stopped = s.sim().fleet.body_fixed_state(&id, s.sim().home);
    println!("brake velocity {:?}", stopped.velocity);
    assert!(stopped.velocity.length() < 0.5);
    let replay = FlightSession::from_recording(s.recording());
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
}
#[test]
fn fixture_uses_same_assembly_graph_mass_and_control_rejection() {
    let craft = rover();
    let compiled = void_assembly::compile(&craft).unwrap();
    assert_eq!(compiled.parts.len(), 5);
    assert!((compiled.summary(None).mass_kg - 360.0).abs() < 1e-12);
    let p = void_landing::pebble();
    let mut sim = FleetFlight::new(
        p.clone(),
        &void_assembly::demo_craft(),
        flat_site(&p),
        false,
    );
    assert!(
        sim.fleet
            .set_vehicle_control(&sim.selected, VehicleControl::default())
            .is_err()
    );
    assert!(sim.fleet.snapshot(&sim.selected).velocity.is_finite());
    let _ = DVec3::ZERO;
}

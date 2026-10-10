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

fn body_heading(sim: &FleetFlight, id: &str) -> f64 {
    let state = sim.fleet.body_fixed_state(id, sim.home);
    let up = state.position.normalize();
    let north = (DVec3::Z - up * up.z).normalize();
    let east = DVec3::Z.cross(up).normalize();
    let forward = sim
        .fleet
        .frames()
        .transform(
            sim.fleet.vessel_frame(id),
            sim.fleet.body_frames(sim.home).1,
        )
        .apply_direction(DVec3::Z);
    forward.dot(east).atan2(forward.dot(north))
}
#[test]
fn positive_steering_turns_right_in_body_fixed_north_east_coordinates() {
    let mut s = make();
    advance(&mut s, 5.0);
    let id = s.sim().selected.clone();
    s.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.6,
            steer: 0.0,
            brake: 0.0,
        },
    });
    advance(&mut s, 3.0);
    let before = body_heading(s.sim(), &id);
    s.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.5,
            steer: 0.5,
            brake: 0.0,
        },
    });
    advance(&mut s, 0.6);
    let after = body_heading(s.sim(), &id);
    let delta = (after - before + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    assert!(
        delta > 0.02,
        "right steer body-fixed heading change {}°",
        delta.to_degrees()
    );
}

#[test]
fn passive_suspension_can_sleep_and_driver_input_wakes_without_sinking() {
    let mut s = make();
    advance(&mut s, 20.0);
    let sim = s.sim();
    let id = sim.selected.clone();
    let scene = sim.fleet.snapshot(&id).scene.unwrap();
    let state = sim
        .fleet
        .scene_snapshots()
        .into_iter()
        .find(|x| x.id == scene)
        .unwrap();
    println!("parked sleep {}", state.asleep);
    assert!(
        state.asleep,
        "passive support repeatedly reset native sleep activation"
    );
    let before = sim.fleet.body_fixed_state(&id, sim.home);
    advance(&mut s, 2.0);
    let after = s.sim().fleet.body_fixed_state(&id, s.sim().home);
    assert!((after.position - before.position).length() < 1e-6);
    s.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.5,
            steer: 0.0,
            brake: 0.0,
        },
    });
    advance(&mut s, 1.0);
    let moved = s.sim().fleet.body_fixed_state(&id, s.sim().home);
    assert!((moved.position - after.position).length() > 0.2);
}

#[test]
fn four_tires_park_on_real_inclined_terrain_and_roll_when_brake_released() {
    let planet = void_landing::earth_size();
    let terrain = &planet.terrain;
    let site = (0..160)
        .find_map(|i| {
            let u = i as f64 * 2.399963229728653;
            let z = -0.8 + 1.6 * i as f64 / 159.0;
            let r = (1.0 - z * z).sqrt();
            let site = DVec3::new(r * u.cos(), r * u.sin(), z);
            let east = DVec3::Z.cross(site).normalize();
            let north = site.cross(east);
            let height =
                |axis: DVec3| terrain.height((site + axis / terrain.radius_meters).normalize());
            let grade = ((height(east) - height(-east)) / 2.0)
                .hypot((height(north) - height(-north)) / 2.0);
            (grade > 0.12 && grade < 0.22).then_some(site)
        })
        .expect("authored Terra fixture must contain a moderate incline");
    let mut session = FlightSession::new(InitialWorld::new(&planet, &rover(), site, false));
    advance(&mut session, 20.0);
    let id = session.sim().selected.clone();
    let before = session
        .sim()
        .fleet
        .body_fixed_state(&id, session.sim().home);
    advance(&mut session, 3.0);
    let parked = session
        .sim()
        .fleet
        .body_fixed_state(&id, session.sim().home);
    assert!(
        (parked.position - before.position).length() < 0.02,
        "four-tire park brake creeped: {:?}",
        parked.position - before.position
    );
    session.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.0,
            steer: 0.0,
            brake: 0.0,
        },
    });
    advance(&mut session, 3.0);
    let rolling = session
        .sim()
        .fleet
        .body_fixed_state(&id, session.sim().home);
    assert!(
        (rolling.position - parked.position).length() > 0.2,
        "unbraked finite-inertia tires artificially held slope"
    );
}

fn carrier_and_rotor_momentum(sim: &FleetFlight, id: &str) -> DVec3 {
    let snapshot = sim.fleet.snapshot(id);
    let inertia = sim.fleet.inertia(id);
    let mut rotor = DVec3::ZERO;
    for pid in snapshot.part_ids {
        let part = sim.fleet.parts().part(&pid);
        for module in &part.definition.modules {
            let void_assembly::Module::Wheel { id, parameters } = module else {
                continue;
            };
            let ModuleState::Wheel { state, .. } = part.modules[id] else {
                panic!()
            };
            rotor += part.pose.rotation
                * void_assembly::wheel_axle(parameters, state.steer_radians)
                * (parameters.wheel_inertia_kg_m2 * state.spin_radians_per_second);
        }
    }
    snapshot.rotation
        * (inertia * (snapshot.rotation.conjugate() * snapshot.angular_velocity) + rotor)
}

#[test]
fn airborne_motor_steering_braking_and_gyro_conserve_total_momentum_and_continue_saved_journal() {
    use void_fleet_flight::session::Outcome;
    let planet = void_landing::pebble();
    let mut session = FlightSession::new(InitialWorld::new(
        &planet,
        &rover(),
        flat_site(&planet),
        false,
    ))
    .with_recording();
    advance(&mut session, 5.0);
    let Outcome::Spawned(id) = session.execute(Action::LaunchOrbit {
        craft: rover(),
        offset: DVec3::ZERO,
    }) else {
        panic!()
    };
    session.execute(Action::Select { vessel: id.clone() });
    let before = carrier_and_rotor_momentum(session.sim(), &id);
    session.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.4,
            steer: 0.0,
            brake: 0.0,
        },
    });
    assert!(
        session
            .sim()
            .fleet
            .rails_blocker()
            .unwrap()
            .contains("wheel")
    );
    advance(&mut session, 0.2);
    assert!(
        session.sim().fleet.snapshot(&id).angular_velocity.length() > 0.01,
        "motor reaction missing from Orbit owner"
    );
    assert!((carrier_and_rotor_momentum(session.sim(), &id) - before).length() < 1e-7);
    session.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.0,
            steer: 1.0,
            brake: 0.0,
        },
    });
    advance(&mut session, 0.1);
    let states: Vec<_> = session
        .sim()
        .fleet
        .part_snapshots(&id)
        .iter()
        .flat_map(|p| p.modules.values())
        .filter_map(|m| match m {
            ModuleState::Wheel { state, .. } => Some(*state),
            _ => None,
        })
        .collect();
    assert!(
        states
            .iter()
            .any(|s| s.steer_radians > 0.0 && s.steer_radians < 0.45)
    );
    assert!(states.iter().all(|s| !s.grounded));
    assert!(
        (carrier_and_rotor_momentum(session.sim(), &id) - before).length() < 1e-7,
        "steering axis-change reaction missing"
    );
    let saved = FlightCheckpoint::capture(session.sim(), session.recording_initial().clone());
    let saved: FlightCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut restored = FlightSession::from_checkpoint(saved).with_recording();
    for action in [
        Action::Advance {
            seconds: 0.15,
            rails: false,
        },
        Action::Vehicle {
            control: VehicleControl::default(),
        },
        Action::Advance {
            seconds: 0.5,
            rails: false,
        },
    ] {
        assert_eq!(session.execute(action.clone()), restored.execute(action));
        assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
        assert!((carrier_and_rotor_momentum(session.sim(), &id) - before).length() < 1e-7);
    }
    assert!(
        session.sim().fleet.snapshot(&id).angular_velocity.length() < 1e-7,
        "brake failed to return rotor momentum to carrier"
    );
    let replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
}

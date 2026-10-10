use glam::DVec3;
use void_assembly::{ModuleState, VehicleControl, aircraft};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome};
use void_vessels::AirDynamics;

/// A test-only spherical runway world with <1cm hills, so gear and takeoff checks do not
/// depend on the shape of normal terrain.
fn flat_runway_planet(mut planet: void_landing::LandingPlanet) -> void_landing::LandingPlanet {
    assert!(
        planet.air_density_scale.is_some(),
        "aircraft acceptance requires an atmospheric planet"
    );
    planet.terrain_config = void_terrain::TerrainConfig::Hills(void_terrain::HillsOptions {
        name: "Aircraft acceptance runway terrain".into(),
        radius_meters: planet.terrain.radius_meters,
        max_height_meters: 0.01,
        wavelength_meters: 10_000.0,
        octaves: 1,
    });
    planet.terrain =
        std::sync::Arc::new(void_terrain::Terrain::from_config(&planet.terrain_config));
    planet.air_datum = 0.0;
    planet.sea_level = None;
    planet.label.push_str(" | AIRCRAFT RUNWAY ACCEPTANCE");
    planet
}

fn make() -> FlightSession {
    let planet = flat_runway_planet(void_landing::earth_size());
    let site = DVec3::new(0.8, 0.55, 0.25).normalize();
    FlightSession::new(
        InitialWorld::new(&planet, &aircraft(), site, true)
            .with_air_dynamics(AirDynamics::ForceAndTorque),
    )
}
fn advance(session: &mut FlightSession, seconds: f64) {
    assert_eq!(
        session.execute(Action::Advance {
            seconds,
            rails: false
        }),
        Outcome::Advanced(true)
    );
}
#[test]
fn runway_aircraft_settles_on_shared_passive_gear_and_taxis_by_jet() {
    let compiled = void_assembly::compile(&aircraft()).unwrap();
    println!("mass center {:?}", compiled.summary(None));
    let mut session = make();
    advance(&mut session, 10.0);
    let sim = session.sim();
    let id = sim.selected.clone();
    let start = sim.fleet.body_fixed_state(&id, sim.home);
    let grounded = sim
        .fleet
        .parts()
        .parts()
        .flat_map(|p| p.modules.values())
        .filter(|m| matches!(m,ModuleState::Wheel{state,..} if state.grounded))
        .count();
    println!(
        "grounded {grounded}, speed {}, {:?}",
        start.velocity.length(),
        sim.fleet.snapshot(&id)
    );
    assert_eq!(grounded, 3);
    assert!(start.velocity.length() < 0.5);
    assert!(matches!(
        session.execute(Action::Sas { enabled: true }),
        Outcome::Refused(_)
    ));
    session.execute(Action::Stage);
    session.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.0,
            steer: 0.0,
            brake: 0.0,
        },
    });
    session.execute(Action::Control {
        throttle: 0.4,
        turn: DVec3::ZERO,
    });
    advance(&mut session, 10.0);
    let sim = session.sim();
    let end = sim.fleet.body_fixed_state(&id, sim.home);
    println!(
        "taxi speed {}, movement {}, {:?}",
        end.velocity.length(),
        (end.position - start.position).length(),
        sim.fleet.snapshot(&id)
    );
    assert!(end.velocity.length() > 3.0);
    assert!((end.position - start.position).length() > 3.0);
}

#[test]
fn runway_takeoff_uses_wings_and_accepted_fuel_without_reaction_torque() {
    let mut session = make();
    advance(&mut session, 10.0);
    let id = session.sim().selected.clone();
    let home = session.sim().home;
    let initial = session.sim().fleet.body_fixed_state(&id, home);
    session.execute(Action::Stage);
    session.execute(Action::Vehicle {
        control: VehicleControl {
            drive: 0.0,
            steer: 0.0,
            brake: 0.0,
        },
    });
    session.execute(Action::Control {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    let mut max_height = 0.0_f64;
    for step in 0..600 {
        let sim = session.sim();
        let local = sim.fleet.body_fixed_state(&id, home);
        let speed = local.velocity.length();
        let height = local.position.length() - initial.position.length();
        max_height = max_height.max(height);
        let snap = sim.fleet.snapshot(&id);
        let up = sim
            .fleet
            .frames()
            .transform(sim.fleet.body_frames(home).1, sim.fleet.origin_frame())
            .apply_direction(local.position.normalize());
        let nose = (snap.rotation * DVec3::Z).dot(up).asin().to_degrees();
        let body_rate = snap.rotation.conjugate() * snap.angular_velocity;
        let target = if height > 20.0 { 8.0 } else { 12.0 };
        let pitch = if speed > 35.0 {
            ((nose - target) * 0.04 - body_rate.x * 0.8).clamp(-0.8, 0.8)
        } else {
            0.0
        };
        session.execute(Action::Control {
            throttle: 1.0,
            turn: DVec3::X * pitch,
        });
        advance(&mut session, 0.05);
        let f = &session.sim().fleet;
        let snap = f.snapshot(&id);
        let load = f.aerodynamic_wrench(&id);
        if step % 20 == 0 {
            println!(
                "{step}: v {speed:.2} height {height:.2} pitch {pitch:.2} nose {nose:.2} lift {:.1} torque {:?}",
                load.force.dot(up),
                snap.rotation.conjugate() * load.torque
            );
        }
    }
    let mut touched_down = false;
    for step in 0..2400 {
        let sim = session.sim();
        let local = sim.fleet.body_fixed_state(&id, home);
        let height = local.position.length() - initial.position.length();
        let snap = sim.fleet.snapshot(&id);
        let up = sim
            .fleet
            .frames()
            .transform(sim.fleet.body_frames(home).1, sim.fleet.origin_frame())
            .apply_direction(local.position.normalize());
        let nose = (snap.rotation * DVec3::Z).dot(up).asin().to_degrees();
        let rate = (snap.rotation.conjugate() * snap.angular_velocity).x;
        let target = if height < 8.0 { 10.0 } else { 8.0 };
        let pitch = ((nose - target) * 0.04 - rate * 0.8).clamp(-0.8, 0.8);
        let grounded = sim
            .fleet
            .parts()
            .parts()
            .flat_map(|p| p.modules.values())
            .filter(|m| matches!(m,ModuleState::Wheel{state,..} if state.grounded))
            .count();
        if height < 2.0 && grounded >= 2 {
            println!(
                "first touchdown sink {:.2} speed {:.2} nose {nose:.2}",
                local.velocity.dot(local.position.normalize()),
                local.velocity.length()
            );
            touched_down = true;
            session.execute(Action::Control {
                throttle: 0.0,
                turn: DVec3::ZERO,
            });
            session.execute(Action::Vehicle {
                control: VehicleControl::default(),
            });
            advance(&mut session, 10.0);
            break;
        }
        session.execute(Action::Control {
            throttle: if height < 8.0 { 0.45 } else { 0.4 },
            turn: DVec3::X * pitch,
        });
        advance(&mut session, 0.05);
        if step % 200 == 0 {
            println!(
                "landing {step}: height {height:.2} speed {:.2} nose {nose:.2}",
                local.velocity.length()
            );
        }
    }
    assert!(touched_down, "aircraft never returned to wheel contact");
    let speed = session
        .sim()
        .fleet
        .body_fixed_state(&id, home)
        .velocity
        .length();
    println!("touchdown stopped speed {speed}");
    assert!(speed < 1.0, "landing brakes did not stop aircraft");
    let sim = session.sim();
    println!(
        "takeoff maxheight {max_height}, final {:?}",
        sim.fleet.snapshot(&id)
    );
    assert!(max_height > 15.0, "aircraft did not physically take off");
    assert!(!sim.fleet.has_reaction_wheel(&id));
    assert!(sim.fleet.snapshot(&id).mass_kg < 915.0);
}

#[test]
fn aircraft_controls_gear_thermal_and_fuel_checkpoint_and_replay_exactly() {
    use void_fleet_flight::{checkpoint::FlightCheckpoint, session::world_mark};
    let mut session = make().with_recording();
    advance(&mut session, 3.0);
    assert!(matches!(
        session.execute(Action::Sas { enabled: true }),
        Outcome::Refused(_)
    ));
    for action in [
        Action::Stage,
        Action::Vehicle {
            control: VehicleControl {
                drive: 0.0,
                steer: 0.25,
                brake: 0.0,
            },
        },
        Action::Control {
            throttle: 0.5,
            turn: DVec3::new(-0.1, 0.05, 0.03),
        },
        Action::Advance {
            seconds: 3.0,
            rails: false,
        },
    ] {
        session.execute(action);
    }
    let saved = FlightCheckpoint::capture(session.sim(), session.recording_initial().clone());
    let saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut restored = FlightSession::from_checkpoint(saved).with_recording();
    assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
    for action in [
        Action::Control {
            throttle: 0.2,
            turn: DVec3::ZERO,
        },
        Action::Vehicle {
            control: VehicleControl::default(),
        },
        Action::Advance {
            seconds: 2.0,
            rails: false,
        },
    ] {
        assert_eq!(session.execute(action.clone()), restored.execute(action));
        assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
    }
    let replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
    let f = &session.sim().fleet;
    for part in f.part_snapshots(&session.sim().selected) {
        assert!(f.parts().part(&part.id).modules.values().any(
            |m| matches!(m,ModuleState::Thermal{state} if state.skin_k.is_finite()&&!state.failed)
        ));
    }
}

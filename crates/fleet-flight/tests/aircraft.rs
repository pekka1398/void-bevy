use glam::DVec3;
use void_assembly::{ModuleState, VehicleControl, aircraft};
use void_fleet_flight::{
    aircraft_acceptance_planet,
    session::{Action, FlightSession, InitialWorld, Outcome},
};
use void_vessels::AirDynamics;

fn make() -> FlightSession {
    let planet = aircraft_acceptance_planet(void_landing::earth_size());
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
    let mut session = make();
    advance(&mut session, 3.0);
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
    advance(&mut session, 3.0);
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
    advance(&mut session, 3.0);
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
    for second in 0..30 {
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
            ((nose - target) * 0.07 + body_rate.x * 0.8).clamp(-0.8, 0.8)
        } else {
            0.0
        };
        session.execute(Action::Control {
            throttle: 1.0,
            turn: DVec3::X * pitch,
        });
        advance(&mut session, 1.0);
        let f = &session.sim().fleet;
        let snap = f.snapshot(&id);
        let load = f.aerodynamic_wrench(&id);
        println!(
            "{second}: v {speed:.2} height {height:.2} pitch {pitch:.2} nose {nose:.2} lift {:.1} torque {:?}",
            load.force.dot(up),
            snap.rotation.conjugate() * load.torque
        );
    }
    let sim = session.sim();
    println!(
        "takeoff maxheight {max_height}, final {:?}",
        sim.fleet.snapshot(&id)
    );
    assert!(max_height > 15.0, "aircraft did not physically take off");
    assert!(!sim.fleet.has_reaction_wheel(&id));
    assert!(sim.fleet.snapshot(&id).mass_kg < 915.0);
}

use glam::DVec3;
use void_assembly::demo_craft;
use void_fleet_flight::{
    FleetFlight,
    checkpoint::FlightCheckpoint,
    session::{InitialWorld, world_mark},
};
use void_orbit::{
    AttitudeLaw, Control, PropagationRun, ThrustControl, VesselPropagator, VesselState,
};
use void_testkit::earth_size;
use void_testkit::flat_site;
use void_vessels::{GuidanceStatus, VesselControl, VesselMode};

fn fixture() -> (FleetFlight, InitialWorld, String) {
    let planet = earth_size();
    let craft = demo_craft();
    let site = flat_site(&planet);
    let initial = InitialWorld::new(&planet, &craft, site, false);
    let mut sim = FleetFlight::new(planet, &craft, site, false);
    // Keep the ground craft alive: its fixed-step scene must not round the orbital burn times.
    let id = sim.launch_orbital_at(sim.home, &craft, DVec3::ZERO);
    sim.select(&id);
    sim.stage();
    assert_eq!(sim.mode(), VesselMode::Orbit);
    (sim, initial, id)
}
#[test]
fn guided_burn_splits_substep_boundaries_and_preserves_fuel_and_direction() {
    let (mut sim, _, id) = fixture();
    let rating = sim.fleet.full_throttle_vacuum_thrust(&id);
    let before = sim.fleet.snapshot(&id);
    let t = sim.fleet.time();
    let start = t + 0.007;
    let end = start + 0.119;
    let law = AttitudeLaw::Frenet {
        reference_body: sim.home,
        tangent: 1.0,
        normal: 0.0,
        radial: 0.0,
    };
    sim.fleet.arm_guided_burn(&id, start, end, law).unwrap();
    assert!(sim.fleet.rails_blocker().unwrap().contains("maneuver"));
    assert_eq!(sim.fleet.thrust(&id).flow_kg_per_second, 0.0);
    sim.advance(0.2, false).unwrap();
    let after = sim.fleet.snapshot(&id);
    let spent = rating.flow_kg_per_second * (end - start);
    assert!(
        (before.mass_kg - after.mass_kg - spent).abs() < 1e-9,
        "burn boundary lost fuel: {spent}"
    );
    assert_eq!(
        sim.fleet.guidance(&id).unwrap().status,
        GuidanceStatus::Completed
    );
    assert_eq!(sim.fleet.control(&id).throttle, 0.0);
    let mut reference = PropagationRun::new(VesselState {
        time: t,
        position: before.position,
        velocity: before.velocity,
        mass_kg: before.mass_kg,
    });
    let mut prop = VesselPropagator::new(&sim.fleet.ephemeris, sim.fleet.options.tolerances);
    let mut at_cutoff = void_frames::State {
        position: before.position,
        velocity: before.velocity,
    };
    for (until, control) in [
        (start, None),
        (
            end,
            Some(Control::Thrust(ThrustControl {
                thrust_newtons: rating.force.length(),
                exhaust_velocity: rating.force.length() / rating.flow_kg_per_second,
                minimum_mass_kg: before.mass_kg - spent,
                attitude: law,
            })),
        ),
        (sim.fleet.time(), None),
    ] {
        prop.advance(
            &mut sim.fleet.ephemeris,
            &mut reference,
            until,
            100_000,
            None,
            control,
        );
        if until == end {
            at_cutoff = void_frames::State {
                position: reference.state().position,
                velocity: reference.state().velocity,
            };
        }
    }
    let dv = (reference.state().velocity - after.velocity).length();
    assert!(
        dv < 1e-6,
        "direction law disagrees with independent orbital propagation: {dv}"
    );
    // Fleet moves the centre of mass as fuel is spent; the reference propagates a fixed point.
    let dp = (reference.state().position - after.position).length();
    assert!(dp < 0.01, "unexpected centre-of-mass displacement {dp}");
    let direction = prop.thrust_direction(
        &sim.fleet.ephemeris,
        &law,
        end,
        at_cutoff.position,
        at_cutoff.velocity,
    );
    let angle_error = (after.rotation * rating.force.normalize() - direction).length();
    assert!(
        angle_error < 1e-7,
        "orientation error {angle_error}: {:?} vs {direction:?}",
        after.rotation * rating.force.normalize()
    );
}
#[test]
fn armed_and_running_burns_restore_and_continue_without_rearming() {
    let (mut sim, initial, id) = fixture();
    let start = sim.fleet.time() + 0.047;
    sim.fleet
        .arm_guided_burn(
            &id,
            start,
            start + 0.713,
            AttitudeLaw::Inertial {
                direction: DVec3::Z,
            },
        )
        .unwrap();
    for elapsed in [0.023, 0.1] {
        sim.advance(elapsed, false).unwrap();
        let checkpoint = FlightCheckpoint::capture(&sim, initial.clone());
        let bytes = serde_json::to_vec(&checkpoint).unwrap();
        let mut restored = serde_json::from_slice::<FlightCheckpoint>(&bytes)
            .unwrap()
            .restore();
        for seconds in [0.037, 0.151, 0.023] {
            sim.advance(seconds, false).unwrap();
            restored.advance(seconds, false).unwrap();
            assert_eq!(world_mark(&sim), world_mark(&restored));
            assert_eq!(
                serde_json::to_value(sim.fleet.checkpoint()).unwrap(),
                serde_json::to_value(restored.fleet.checkpoint()).unwrap()
            );
        }
    }
}
#[test]
fn manual_rcs_and_ideal_guidance_cannot_own_attitude_together() {
    use void_vessels::RcsControl;
    let law = AttitudeLaw::Inertial {
        direction: DVec3::Z,
    };
    for elapsed in [0.0, 0.1] {
        let (mut sim, _, id) = fixture();
        let t = sim.fleet.time();
        sim.fleet
            .arm_guided_burn(&id, t + 0.05, t + 0.5, law)
            .unwrap();
        sim.fleet.set_rcs_control(
            &id,
            RcsControl {
                enabled: true,
                ..Default::default()
            },
        );
        assert_eq!(
            sim.fleet.guidance(&id).unwrap().status,
            GuidanceStatus::Armed
        );
        if elapsed > 0.0 {
            sim.advance(elapsed, false).unwrap();
        }
        sim.fleet.set_rcs_control(
            &id,
            RcsControl {
                enabled: true,
                force: DVec3::ZERO,
                torque: DVec3::X,
            },
        );
        assert_eq!(
            sim.fleet.guidance(&id).unwrap().status,
            GuidanceStatus::Aborted("manual RCS control".into())
        );
        assert_eq!(sim.fleet.control(&id).throttle, 0.0);
        let t = sim.fleet.time();
        assert_eq!(
            sim.fleet.arm_guided_burn(&id, t + 0.1, t + 0.3, law),
            Err("manual RCS control is active".into())
        );
        sim.fleet.set_rcs_control(
            &id,
            RcsControl {
                enabled: false,
                force: DVec3::ZERO,
                torque: DVec3::X,
            },
        );
        sim.fleet
            .arm_guided_burn(&id, t + 0.1, t + 0.3, law)
            .unwrap();
    }
}
#[test]
fn manual_control_and_staging_explicitly_abort_guidance() {
    for staging in [false, true] {
        let (mut sim, _, id) = fixture();
        sim.fleet
            .arm_guided_burn(
                &id,
                sim.fleet.time() + 1.0,
                sim.fleet.time() + 2.0,
                AttitudeLaw::Inertial {
                    direction: DVec3::Z,
                },
            )
            .unwrap();
        if staging {
            sim.stage();
        } else {
            sim.control(VesselControl {
                throttle: 0.2,
                turn: DVec3::ZERO,
            });
        }
        assert!(matches!(
            sim.fleet.guidance(&id).unwrap().status,
            GuidanceStatus::Aborted(_)
        ));
        if !staging {
            assert_eq!(sim.fleet.control(&id).throttle, 0.2);
        }
    }
}

#[test]
fn contact_handoff_aborts_an_armed_burn_instead_of_steering_a_contact_body() {
    let (mut sim, _, _) = fixture();
    let frame = void_landing::PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let position =
        DVec3::X * (frame.body.radius_meters + sim.planet.terrain.height(DVec3::X) + 450.0);
    let local = void_frames::State {
        position,
        velocity: -DVec3::X * 50.0,
    };
    let state = frame.to_inertial(&sim.fleet.ephemeris, sim.fleet.time(), local);
    let id = sim
        .fleet
        .launch(&demo_craft(), state, glam::DQuat::IDENTITY, DVec3::ZERO);
    sim.select(&id);
    sim.stage();
    assert_eq!(sim.mode(), VesselMode::Orbit);
    let t = sim.fleet.time();
    sim.fleet
        .arm_guided_burn(
            &id,
            t + 10.0,
            t + 11.0,
            AttitudeLaw::Inertial {
                direction: DVec3::Z,
            },
        )
        .unwrap();
    sim.advance(5.0, false).unwrap();
    assert_eq!(sim.mode(), VesselMode::Ground);
    assert_eq!(sim.fleet.control(&id).throttle, 0.0);
    assert_eq!(
        sim.fleet.guidance(&id).unwrap().status,
        GuidanceStatus::Aborted("entered contact physics".into())
    );
}

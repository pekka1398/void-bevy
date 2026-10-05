//! Engine-free helpers used by the keyboard lab; the generic session Select action is unchanged.
use glam::DVec3;
use void_fleet_flight::session::{Action, FlightSession, Outcome};
use void_vessels::{RcsControl, VesselSnapshot};

/// Transfer this lab's transient keyboard input, preserving each vessel's own RCS enable state.
/// Both actions pass through the durable journal; validate the destination before any mutation.
pub fn pilot_handoff(session: &mut FlightSession, target: &str) -> (Outcome, bool) {
    session.sim().fleet.snapshot(target);
    let selected = &session.sim().selected;
    let enabled = session.sim().fleet.rcs_control(selected).enabled;
    session.execute(Action::Rcs {
        control: RcsControl {
            enabled,
            ..Default::default()
        },
    });
    let outcome = session.execute(Action::Select {
        vessel: target.into(),
    });
    (outcome, session.sim().fleet.rcs_control(target).enabled)
}

/// Allocation points belong to the vessel's parts frame, while snapshots report the live COM.
pub fn nozzle_point_relative(
    ship: &VesselSnapshot,
    centre: DVec3,
    point: DVec3,
    origin: DVec3,
) -> DVec3 {
    (ship.position - origin) + ship.rotation * (point - centre)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DQuat;
    use void_assembly::{ResourceId, add_part, rendezvous_pod};
    use void_fleet_flight::session::{InitialWorld, world_mark};
    use void_landing::{FrameState, PlanetFrame, earth_size};
    use void_vessels::{Fleet, Scenario, VesselMode, create_lab_scene, flat_site};

    #[test]
    fn keyboard_handoff_neutralizes_old_ship_preserves_enables_and_replays() {
        let planet = earth_size();
        let site = flat_site(&planet);
        let craft = rendezvous_pod();
        let mut session =
            FlightSession::new(InitialWorld::new(&planet, &craft, site, false)).with_recording();
        let sim = session.sim();
        let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
        let state = frame.to_inertial(
            &sim.fleet.ephemeris,
            0.0,
            FrameState {
                position: site * (planet.terrain.radius_meters + 500_000.0),
                velocity: DVec3::ZERO,
            },
        );
        for i in 0..2 {
            session.execute(Action::LaunchState {
                craft: craft.clone(),
                position: state.position + DVec3::X * 100.0 * i as f64,
                velocity: state.velocity,
                rotation: DQuat::IDENTITY,
                angular_velocity: DVec3::ZERO,
            });
        }
        session.execute(Action::Select {
            vessel: "v2".into(),
        });
        session.execute(Action::Rcs {
            control: RcsControl {
                enabled: true,
                force: DVec3::X * 40.0,
                torque: DVec3::Y * 15.0,
            },
        });
        assert!(session.sim().fleet.thrust("v2").flow_kg_per_second > 0.0);
        let old_fuel = session
            .sim()
            .fleet
            .parts()
            .part("v2/p1")
            .resource(ResourceId::Monopropellant);
        let (outcome, enabled) = pilot_handoff(&mut session, "v3");
        assert_eq!(outcome, Outcome::Applied);
        assert!(!enabled);
        assert_eq!(session.sim().selected, "v3");
        assert_eq!(
            session.sim().fleet.rcs_control("v2"),
            RcsControl {
                enabled: true,
                ..Default::default()
            }
        );
        assert_eq!(session.sim().fleet.thrust("v2").flow_kg_per_second, 0.0);
        session.execute(Action::Advance {
            seconds: 0.1,
            rails: false,
        });
        assert_eq!(
            session
                .sim()
                .fleet
                .parts()
                .part("v2/p1")
                .resource(ResourceId::Monopropellant),
            old_fuel
        );
        let (_, enabled) = pilot_handoff(&mut session, "v2");
        assert!(enabled);
        assert!(!session.sim().fleet.rcs_control("v3").enabled);
        session.mark();
        let replay = FlightSession::from_recording(session.recording());
        assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
        session.execute(Action::Rcs {
            control: RcsControl {
                enabled: true,
                force: DVec3::X * 40.0,
                torque: DVec3::Y * 15.0,
            },
        });
        let before = world_mark(session.sim());
        let count = session.recording().entries.len();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pilot_handoff(&mut session, "unknown-vessel")
        }));
        assert!(failed.is_err());
        assert_eq!(before, world_mark(session.sim()));
        assert_eq!(session.recording().entries.len(), count);
    }

    fn depleted_scene() -> (Fleet, String) {
        let mut fleet = create_lab_scene(Scenario::Coast).fleet;
        let start = fleet.snapshot("v1");
        let mut craft = add_part(&rendezvous_pod(), "rcs-pod", "p1", "bottom", "top").unwrap();
        craft.parts[1]
            .resources
            .insert(ResourceId::Monopropellant, 0.0);
        let mut ids = vec![];
        for i in 0..2 {
            let id = fleet.launch(
                &craft,
                FrameState {
                    position: start.position + DVec3::X * (1000.0 + 100.0 * i as f64),
                    velocity: start.velocity,
                },
                DQuat::IDENTITY,
                DVec3::Z * 0.08,
            );
            fleet.set_rcs_control(
                &id,
                RcsControl {
                    enabled: true,
                    force: DVec3::Y * 320.0,
                    torque: DVec3::ZERO,
                },
            );
            ids.push(id);
        }
        fleet.advance(30.0);
        (fleet, ids.remove(0))
    }

    #[test]
    fn plume_points_match_part_frames_after_scene_mass_depletion() {
        let (fleet, id) = depleted_scene();
        let ship = fleet.snapshot(&id);
        assert_eq!(ship.mode, VesselMode::Bubble);
        let centre = fleet.centre_of_mass_local(&id);
        assert!(centre.length() > 0.01);
        // Render relative to the parts origin, independently obtained from the frame tree.
        let origin = fleet
            .frames()
            .transform(fleet.vessel_frame(&id), fleet.origin_frame())
            .apply_point(DVec3::ZERO);
        let allocation = fleet.rcs_allocation(&id);
        assert!(!allocation.nozzles.is_empty());
        for nozzle in allocation.nozzles {
            let part = fleet.parts().part(&nozzle.part);
            let module = part
                .definition
                .modules
                .iter()
                .find(|m| m.id() == nozzle.module)
                .unwrap();
            let void_assembly::Module::Rcs { point, .. } = module else {
                panic!("not RCS")
            };
            let expected = fleet
                .frames()
                .transform(fleet.part_frame(&nozzle.part), fleet.vessel_frame(&id))
                .apply_point(*point);
            let actual = nozzle_point_relative(&ship, centre, nozzle.point, origin);
            assert!((actual - ship.rotation * expected).length() < 5e-5);
            let old = (ship.position - origin) + ship.rotation * nozzle.point;
            assert!(
                (old - ship.rotation * expected).length() > 0.01,
                "uncorrected plume must visibly miss its mount"
            );
        }
    }
}

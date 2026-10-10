use glam::{DQuat, DVec3};
use void_assembly::rendezvous_pod;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome, world_mark};
use void_frames::State;
use void_landing::{PlanetFrame, earth_size};
use void_vessels::{RcsControl, flat_site};
#[test]
fn durable_rcs_dock_undock_save_replay_continues_identically() {
    let planet = earth_size();
    let site = flat_site(&planet);
    let craft = rendezvous_pod();
    let mut session =
        FlightSession::new(InitialWorld::new(&planet, &craft, site, false)).with_recording();
    let sim = session.sim();
    let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let state = frame.to_inertial(
        &sim.fleet.ephemeris,
        0.,
        State {
            position: site * (planet.terrain.radius_meters + 500_000.),
            velocity: DVec3::ZERO,
        },
    );
    for i in 0..2 {
        session.execute(Action::LaunchState {
            craft: craft.clone(),
            position: state.position + DVec3::Y * 2.15 * i as f64,
            velocity: state.velocity,
            rotation: if i == 0 {
                DQuat::IDENTITY
            } else {
                DQuat::from_rotation_x(std::f64::consts::PI)
            },
            angular_velocity: DVec3::ZERO,
        });
    }
    assert!(matches!(
        session.execute(Action::Dock {
            part_a: "v2/p1".into(),
            module_a: "dock".into(),
            part_b: "v3/p1".into(),
            module_b: "dock".into()
        }),
        Outcome::Spawned(_)
    ));
    session.execute(Action::Rcs {
        control: RcsControl {
            enabled: true,
            force: DVec3::X * 40.,
            torque: DVec3::Y * 15.,
        },
    });
    session.execute(Action::Advance {
        seconds: 0.1,
        rails: false,
    });
    session.execute(Action::RcsNozzle {
        part: "v2/p1".into(),
        module: "rcs-0-0-1".into(),
        enabled: false,
    });
    session.mark();
    let path = std::env::temp_dir().join(format!("void-rcs-save-{}.json", std::process::id()));
    session.save_checkpoint(&path);
    let mut restored = FlightSession::load_checkpoint(&path);
    assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
    let mut replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
    for s in [&mut session, &mut restored, &mut replay] {
        assert!(matches!(
            s.execute(Action::Undock {
                part: "v2/p1".into(),
                module: "dock".into()
            }),
            Outcome::Spawned(_)
        ));
        s.execute(Action::Advance {
            seconds: 0.2,
            rails: false,
        });
        for part in ["v2/p1", "v3/p1"] {
            s.execute(Action::ArmDock {
                part: part.into(),
                module: "dock".into(),
                armed: true,
            });
        }
    }
    assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
    std::fs::remove_file(path).unwrap();
}

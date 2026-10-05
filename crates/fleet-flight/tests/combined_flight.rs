//! A single durable world exercises Full air, RCS, docking, save and split continuation.
use glam::{DQuat, DVec3};
use void_assembly::{ResourceId, rendezvous_pod};
use void_fleet_flight::session::{
    Action, FlightSession, InitialWorld, Outcome, Recording, world_mark,
};
use void_landing::{FrameState, PlanetFrame, earth_size};
use void_vessels::{AirDynamics, RcsControl, VesselMode, flat_site};

fn advance(s: &mut FlightSession, seconds: f64) {
    assert_eq!(
        s.execute(Action::Advance {
            seconds,
            rails: false
        }),
        Outcome::Advanced(true)
    );
}

#[test]
fn full_air_rcs_docking_checkpoint_and_durable_journal_preserve_live_state() {
    let planet = earth_size();
    let craft = rendezvous_pod();
    let mut s = FlightSession::new(
        InitialWorld::new(&planet, &craft, flat_site(&planet), true)
            .with_air_dynamics(AirDynamics::ForceAndTorque),
    )
    .with_recording();
    let directory =
        std::env::temp_dir().join(format!("void-combined-flight-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let journal = directory.join("session.jsonl");
    let save = directory.join("checkpoint.json");
    s.begin_stream(&journal);
    let sim = s.sim();
    let state = PlanetFrame::new(&sim.fleet.ephemeris, sim.home).to_inertial(
        &sim.fleet.ephemeris,
        0.,
        FrameState {
            position: DVec3::X * (planet.terrain.radius_meters + 5000.),
            velocity: DVec3::Y * 80.,
        },
    );
    for i in 0..2 {
        assert!(matches!(
            s.execute(Action::LaunchState {
                craft: craft.clone(),
                position: state.position + DVec3::Y * (2.15 * i as f64),
                velocity: state.velocity,
                rotation: if i == 0 {
                    DQuat::IDENTITY
                } else {
                    DQuat::from_rotation_x(std::f64::consts::PI)
                },
                angular_velocity: DVec3::ZERO,
            }),
            Outcome::Spawned(_)
        ));
    }
    assert_eq!(
        s.execute(Action::RcsNozzle {
            part: "v2/p1".into(),
            module: "rcs-0-0-1".into(),
            enabled: false,
        }),
        Outcome::Applied
    );
    let before: Vec<_> = ["v2/p1", "v3/p1"]
        .map(|id| {
            let p = s.sim().fleet.parts().part(id);
            (p.resources.clone(), p.modules.clone())
        })
        .into();
    let joined = match s.execute(Action::Dock {
        part_a: "v2/p1".into(),
        module_a: "dock".into(),
        part_b: "v3/p1".into(),
        module_b: "dock".into(),
    }) {
        Outcome::Spawned(id) => id,
        result => panic!("capture: {result:?}"),
    };
    assert_eq!(s.sim().fleet.snapshot(&joined).mode, VesselMode::Bubble);
    for (i, id) in ["v2/p1", "v3/p1"].iter().enumerate() {
        let p = s.sim().fleet.parts().part(id);
        assert_eq!((&p.resources, &p.modules), (&before[i].0, &before[i].1));
    }
    s.execute(Action::Select {
        vessel: joined.clone(),
    });
    s.execute(Action::Rcs {
        control: RcsControl {
            enabled: true,
            force: DVec3::X * 40.,
            torque: DVec3::Z * 15.,
        },
    });
    // Non-grid duration retains a pending owner timestep in the direct checkpoint.
    advance(&mut s, 0.113);
    let live = serde_json::to_value(s.sim().fleet.checkpoint()).unwrap();
    assert!(live["pending"].as_f64().unwrap() > 0.);
    let fuel: f64 = ["v2/p1", "v3/p1"]
        .iter()
        .map(|id| {
            s.sim()
                .fleet
                .parts()
                .part(id)
                .resource(ResourceId::Monopropellant)
        })
        .sum();
    assert!(fuel < 40. && fuel > 39.);
    let load = s.sim().fleet.aerodynamic_wrench(&joined);
    assert!(load.force.length() > 1. && load.torque.length() > 1e-6);
    assert!(s.sim().fleet.rails_blocker().is_some());
    s.mark();
    s.save_checkpoint(&save);
    s.finish_stream();
    let mut restored = FlightSession::load_checkpoint(&save);
    let mut replay = FlightSession::from_recording(Recording::read(&journal));
    assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    let resources_before_split: Vec<_> = ["v2/p1", "v3/p1"]
        .map(|id| s.sim().fleet.parts().part(id).resources.clone())
        .into();
    for session in [&mut s, &mut restored, &mut replay] {
        assert!(matches!(
            session.execute(Action::Undock {
                part: "v2/p1".into(),
                module: "dock".into(),
            }),
            Outcome::Spawned(_)
        ));
        for (i, id) in ["v2/p1", "v3/p1"].iter().enumerate() {
            let p = session.sim().fleet.parts().part(id);
            assert_eq!(p.resources, resources_before_split[i]);
            assert_eq!(
                p.modules["dock"],
                void_assembly::ModuleState::DockingPort { armed: false }
            );
        }
        assert_eq!(
            session.sim().fleet.parts().part("v2/p1").modules["rcs-0-0-1"],
            void_assembly::ModuleState::Rcs { enabled: false }
        );
        advance(session, 0.217);
    }
    assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    std::fs::remove_file(journal).unwrap();
    std::fs::remove_file(save).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

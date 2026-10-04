//! Accepted simulation time, checkpoint and durable journal all observe the same module state.
use glam::{DQuat, DVec3};
use void_assembly::{
    Craft, ModuleState, ParachutePhase, add_part, definition, fresh_craft, full_resources,
};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome, world_mark};
use void_landing::{FrameState, PlanetFrame, earth_size};
use void_vessels::flat_site;
fn craft() -> Craft {
    let mut c = fresh_craft();
    c.parts[0].definition_id = "parachute-pod".into();
    c.parts[0].resources = full_resources(definition("parachute-pod").unwrap());
    c
}
fn drop() -> FlightSession {
    let p = earth_size();
    let c = craft();
    let mut s = FlightSession::new(InitialWorld::new(&p, &c, flat_site(&p), true)).with_recording();
    let sim = s.sim();
    let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let state = frame.to_inertial(
        &sim.fleet.ephemeris,
        0.0,
        FrameState {
            position: DVec3::X * (p.terrain.radius_meters + 1000.0),
            velocity: -DVec3::X * 80.0,
        },
    );
    assert!(matches!(
        s.execute(Action::LaunchState {
            craft: c,
            position: state.position,
            velocity: state.velocity,
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO
        }),
        Outcome::Spawned(_)
    ));
    s.execute(Action::Select {
        vessel: "v2".into(),
    });
    s.execute(Action::Parachute {
        part: "v2/p1".into(),
        module: "parachute1".into(),
        deploy: true,
    });
    s
}
fn phase(s: &FlightSession) -> ParachutePhase {
    let ModuleState::Parachute { state } =
        s.sim().fleet.parts().part("v2/p1").modules["parachute1"]
    else {
        panic!()
    };
    state.phase
}
fn advance(s: &mut FlightSession, seconds: f64) {
    assert!(matches!(
        s.execute(Action::Advance {
            seconds,
            rails: false
        }),
        Outcome::Advanced(true)
    ));
}
#[test]
fn deployment_uses_simulation_ticks_and_is_independent_of_render_delta() {
    let mut one = drop();
    let mut split = drop();
    advance(&mut one, 0.0);
    assert_eq!(phase(&one), ParachutePhase::Armed);
    assert!(
        one.sim()
            .fleet
            .rails_blocker()
            .unwrap()
            .contains("parachute")
    );
    advance(&mut one, 0.5);
    for _ in 0..50 {
        advance(&mut split, 0.01);
    }
    assert_eq!(phase(&one), ParachutePhase::SemiDeploying);
    // Different decimal frame sums differ in their pending clock by < 1 ulp;
    // accepted ticks, all owner states and module progress are identical.
    let a = world_mark(one.sim());
    let b = world_mark(split.sim());
    assert_eq!(a["ships"], b["ships"]);
    assert_eq!(a["scenes"], b["scenes"]);
    assert!(
        (one.sim().fleet.pending_seconds() - split.sim().fleet.pending_seconds()).abs() < 1e-12
    );
    for seconds in [0.5, 0.5, 0.5, 0.5, 0.5, 0.5] {
        advance(&mut one, seconds);
    }
    assert_eq!(phase(&one), ParachutePhase::Full);
}
#[test]
fn half_open_and_full_open_saves_and_journals_resume_exactly() {
    for duration in [0.5, 3.5] {
        let mut s = drop();
        advance(&mut s, duration);
        s.mark();
        let path = std::env::temp_dir().join(format!(
            "void-part-state-save-{}-{duration}.json",
            std::process::id()
        ));
        s.save_checkpoint(&path);
        let mut restored = FlightSession::load_checkpoint(&path);
        assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
        for session in [&mut s, &mut restored] {
            advance(session, 0.75);
            session.execute(Action::Parachute {
                part: "v2/p1".into(),
                module: "parachute1".into(),
                deploy: false,
            });
            advance(session, 0.25);
        }
        assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
        s.mark();
        let recording = s.recording();
        let replay = FlightSession::from_recording(recording);
        assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn two_chutes_keep_identity_when_the_stack_splits_and_joins() {
    let mut c = craft();
    c = add_part(&c, "decoupler", "p1", "bottom", "top").unwrap();
    c = add_part(&c, "parachute-pod", "p2", "bottom", "top").unwrap();
    c.parts[1].stage = Some(0);
    let p = earth_size();
    let mut s = FlightSession::new(InitialWorld::new(&p, &c, flat_site(&p), true));
    for part in ["v1/p1", "v1/p3"] {
        s.execute(Action::Parachute {
            part: part.into(),
            module: "parachute1".into(),
            deploy: true,
        });
    }
    advance(&mut s, 0.5);
    let before = s.sim().fleet.parts().part("v1/p1").modules.clone();
    s.execute(Action::Stage);
    assert_eq!(s.sim().fleet.parts().part("v1/p1").modules, before);
    assert_eq!(s.sim().fleet.parts().part("v1/p3").modules, before);
    s.execute(Action::Join {
        part_a: "v1/p1".into(),
        node_a: "bottom".into(),
        part_b: "v1/p2".into(),
        node_b: "top".into(),
    });
    assert_eq!(s.sim().fleet.parts().part("v1/p1").modules, before);
    assert_eq!(s.sim().fleet.parts().part("v1/p3").modules, before);
}

#[test]
#[should_panic(expected = "incompatible simulation model")]
fn model_nine_recording_is_rejected_before_state_restore() {
    let mut s = drop();
    s.mark();
    let mut recording = s.recording();
    recording.model_version = 9;
    let _ = FlightSession::from_recording(recording);
}

#[test]
fn an_open_parachute_slows_the_same_return_without_changing_mass() {
    let mut open = drop();
    let mut cut = drop();
    cut.execute(Action::Parachute {
        part: "v2/p1".into(),
        module: "parachute1".into(),
        deploy: false,
    });
    advance(&mut open, 5.0);
    advance(&mut cut, 5.0);
    assert_eq!(phase(&open), ParachutePhase::Full);
    let sample = |s: &FlightSession| {
        let sim = s.sim();
        let state = sim.fleet.snapshot("v2");
        let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
        let local = frame.to_body_fixed(
            &sim.fleet.ephemeris,
            sim.fleet.time(),
            FrameState {
                position: state.position,
                velocity: state.velocity,
            },
        );
        (local.velocity.length(), state.mass_kg)
    };
    let (slower, mass) = sample(&open);
    let (faster, other_mass) = sample(&cut);
    assert!(slower < 0.75 * faster, "open {slower}m/s, cut {faster}m/s");
    assert_eq!(mass, other_mass, "deploying adds no propellant or mass");
}

use void_fleet_flight::{
    session::{FlightSession, InitialWorld},
    world::stellar_neighborhood,
};
use void_frames::{FrameSource, SystemId};

#[test]
fn authored_neighborhood_launches_normal_craft_and_directly_restores_celestial_state() {
    let planet = void_landing::aurelia();
    let craft = void_assembly::demo_craft();
    let mut initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true);
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut session = FlightSession::new(initial);
    assert_eq!(session.sim().fleet.ephemeris.system_count(), 3);
    let a = session
        .sim()
        .fleet
        .ephemeris
        .system_state(SystemId(0), 0.0)
        .0;
    let b = session
        .sim()
        .fleet
        .ephemeris
        .system_state(SystemId(1), 0.0)
        .0;
    assert!((b.relative(&a).length() / void_multiscale::LIGHT_YEAR - 4.24).abs() < 1e-10);
    for _ in 0..30 {
        session.execute(void_fleet_flight::session::Action::Advance {
            seconds: 1.0 / 60.0,
            rails: false,
        });
    }
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    let wire = serde_json::to_string(&saved).unwrap();
    let decoded: void_fleet_flight::checkpoint::FlightCheckpoint =
        serde_json::from_str(&wire).unwrap();
    let restored = decoded.restore();
    assert_eq!(restored.fleet.ephemeris.system_count(), 3);
    assert_eq!(
        restored.coupled_world.as_ref().unwrap().borrow().steps,
        session.sim().coupled_world.as_ref().unwrap().borrow().steps
    );
    assert_eq!(
        restored.fleet.snapshot(&restored.selected).position,
        session
            .sim()
            .fleet
            .snapshot(&session.sim().selected)
            .position
    );
}

#[test]
fn stellar_checkpoint_refuses_missing_state_or_conflicting_world_bound() {
    let planet = void_landing::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        true,
    );
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let session = FlightSession::new(initial);
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    for (key, replacement) in [
        ("coupled_world", serde_json::Value::Null),
        ("ephemeris_end", serde_json::json!(9e8)),
    ] {
        let mut altered = serde_json::to_value(&saved).unwrap();
        altered[key] = replacement;
        let altered: void_fleet_flight::checkpoint::FlightCheckpoint =
            serde_json::from_value(altered).unwrap();
        assert!(
            std::panic::catch_unwind(|| altered.restore()).is_err(),
            "{key}"
        );
    }
}

#[test]
fn fleet_keeps_remote_orbit_ships_in_their_own_system_without_flattening() {
    use glam::{DQuat, DVec3};
    use void_landing::FrameState;
    let planet = void_landing::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        true,
    );
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut sim = initial.build();
    let body = sim.world.body_index("Beryl/aurelia");
    let (p, v) = sim
        .fleet
        .ephemeris
        .body_in_system(void_frames::BodyId(body), 0.0);
    let radius = sim.fleet.ephemeris.bodies()[body].radius_meters + 400_000.0;
    let state = FrameState {
        position: p + DVec3::X * radius,
        velocity: v + DVec3::Y * (sim.fleet.ephemeris.bodies()[body].gm / radius).sqrt(),
    };
    let a = sim.fleet.launch_in_system(
        &void_vessels::pod_tank("Beryl A · acceptance fixture"),
        SystemId(1),
        state,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let b = sim.fleet.launch_in_system(
        &void_vessels::pod_tank("Beryl B · acceptance fixture"),
        SystemId(1),
        FrameState {
            position: state.position + DVec3::new(0.03, 100.0, 0.0),
            velocity: state.velocity,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    assert!((sim.fleet.relative(&b, &a).position - DVec3::new(0.03, 100.0, 0.0)).length() < 1e-4);
    sim.fleet.advance(0.5);
    assert_eq!(sim.fleet.vessel_system(&a), SystemId(1));
    assert_eq!(sim.fleet.ephemeris.origin_system(), SystemId(0));
    assert!((sim.fleet.relative(&b, &a).position.length() - 100.0).abs() < 0.01);
    let local = sim.fleet.precise_snapshot(&a);
    let body_local = sim
        .fleet
        .ephemeris
        .body_in_system(void_frames::BodyId(body), sim.fleet.time())
        .0;
    assert!((local.local.position - body_local).length() > radius - 100.0);
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial);
    let restored = checkpoint.restore();
    assert_eq!(restored.fleet.precise_snapshot(&a).position, local.position);
    assert_eq!(restored.fleet.vessel_system(&a), SystemId(1));
}

fn neighborhood_initial() -> InitialWorld {
    let planet = void_landing::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        true,
    );
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    initial
}

#[test]
fn interstellar_bubble_preserves_centimeter_separation_and_dock_undock() {
    use glam::{DQuat, DVec3};
    let initial = neighborhood_initial();
    let mut sim = initial.build();
    let position =
        void_frames::SplitPosition::at(DVec3::new(2.0 * void_multiscale::LIGHT_YEAR, 0.0, 0.0));
    let velocity = DVec3::new(2.0, 0.0, 0.0);
    let craft = void_assembly::rendezvous_pod();
    let a = sim.fleet.launch_at_split(
        &craft,
        SystemId(0),
        position,
        velocity,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let b = sim.fleet.launch_at_split(
        &craft,
        SystemId(0),
        position.translate(DVec3::Y * 2.15),
        velocity,
        DQuat::from_rotation_x(std::f64::consts::PI),
        DVec3::ZERO,
    );
    let before = sim.fleet.relative(&b, &a).position;
    assert!((before - DVec3::Y * 2.15).length() < 1e-6);
    sim.fleet.advance(0.0);
    assert_eq!(
        sim.fleet.snapshot(&a).mode,
        void_vessels::VesselMode::Bubble
    );
    let joined = sim
        .fleet
        .dock(&format!("{a}/p1"), "dock", &format!("{b}/p1"), "dock")
        .unwrap();
    sim.fleet.advance(0.25);
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial);
    let mut restored = saved.restore();
    let part = format!("{joined}/p1");
    let separated = restored.fleet.undock(&part, "dock").unwrap();
    restored.fleet.advance(0.0);
    let separation = restored
        .fleet
        .relative(&separated, &joined)
        .position
        .length();
    assert!((separation - 2.15).abs() < 1e-5, "separation {separation}");
    assert!(restored.fleet.precise_snapshot(&joined).position.cell[0].abs() > 1_000_000);
    assert_eq!(
        restored.fleet.ephemeris.physics_offset(),
        void_frames::SplitPosition::ORIGIN
    );
}

#[test]
fn accepted_cruise_handoff_preserves_split_position_and_velocity() {
    use glam::{DQuat, DVec3};
    let mut sim = neighborhood_initial().build();
    // Declared fixture near the 5% hysteresis boundary, not normal rocket propulsion.
    let boundary = 4.24 * 1.05 / 2.05 * void_multiscale::LIGHT_YEAR;
    let position = void_frames::SplitPosition::at(DVec3::X * boundary).translate(DVec3::X * -40.0);
    let id = sim.fleet.launch_at_split(
        &void_vessels::pod_tank("Boundary coast · acceptance fixture"),
        SystemId(0),
        position,
        DVec3::X * 1000.0,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let before = sim.fleet.precise_snapshot(&id);
    sim.fleet.advance(0.25);
    let after = sim.fleet.precise_snapshot(&id);
    assert_eq!(after.system, SystemId(1));
    // Stellar barycentre drift is part of galaxy displacement; relative system coast remains 250 m.
    let root_drift = sim
        .fleet
        .ephemeris
        .system_state(SystemId(0), sim.fleet.time())
        .0
        .relative(&sim.fleet.ephemeris.system_state(SystemId(0), 0.0).0);
    assert!(
        (after.position.relative(&before.position) - root_drift - DVec3::X * 250.0).length() < 1e-4
    );
    assert!((after.velocity - before.velocity).length() < 1e-8);
    assert_eq!(after.residual.position, DVec3::ZERO);
}

#[test]
fn remote_ground_uses_its_own_terrain_air_and_preserves_owner_on_save() {
    use glam::DVec3;
    let initial = neighborhood_initial();
    let mut sim = initial.build();
    let body = sim.world.body_index("Cygnus/aurelia");
    let site = sim.launch_site;
    let remote = sim
        .fleet
        .launch_landed(&void_assembly::demo_craft(), body, site);
    sim.fleet.advance(0.25);
    assert_eq!(
        sim.fleet.snapshot(&remote).mode,
        void_vessels::VesselMode::Ground
    );
    assert_eq!(sim.fleet.vessel_system(&remote), SystemId(2));
    let local = sim.fleet.body_fixed_state(&remote, body);
    let height = sim
        .fleet
        .environment()
        .body(body)
        .unwrap()
        .terrain
        .as_ref()
        .unwrap()
        .height(local.position.normalize());
    let radius = sim.fleet.ephemeris.bodies()[body].radius_meters;
    assert!(local.position.length() - radius - height > 0.0);
    assert!(sim.fleet.aerodynamic_wrench(&remote).force.is_finite());
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial);
    let mut restored = saved.restore();
    assert!(
        (restored.fleet.body_fixed_state(&remote, body).position - local.position).length() < 1e-8
    );
    restored.fleet.set_control(
        &remote,
        void_vessels::VesselControl {
            throttle: 0.0,
            turn: DVec3::ZERO,
        },
    );
    restored.fleet.advance(0.25);
    assert_eq!(
        restored.fleet.snapshot(&remote).mode,
        void_vessels::VesselMode::Ground
    );
}

#[test]
fn journal_replays_remote_ground_orbit_and_split_cruise_with_precise_marks() {
    use glam::{DQuat, DVec3};
    use void_fleet_flight::session::{Action, Outcome, world_mark};
    let mut session = FlightSession::new(neighborhood_initial()).with_recording();
    for action in [
        Action::LaunchGroundAt {
            body: "Beryl/aurelia".into(),
            craft: void_assembly::demo_craft(),
            site: session.sim().launch_site,
        },
        Action::LaunchOrbitAt {
            body: "Cygnus/aurelia".into(),
            craft: void_assembly::demo_craft(),
            offset: DVec3::ZERO,
        },
        Action::LaunchSplitState {
            craft: void_vessels::pod_tank("Cruise · acceptance fixture"),
            system: SystemId(0),
            position: void_frames::SplitPosition::at(
                DVec3::X * (2.0 * void_multiscale::LIGHT_YEAR),
            ),
            velocity: DVec3::X * 3.0,
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        },
    ] {
        assert!(matches!(session.execute(action), Outcome::Spawned(_)));
    }
    session.execute(Action::Advance {
        seconds: 0.25,
        rails: false,
    });
    session.mark();
    let replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(replay.sim()), world_mark(session.sim()));
}

#[test]
fn remote_staging_burns_actual_fuel_and_continues_after_direct_restore() {
    use glam::DVec3;
    use void_fleet_flight::session::{Action, Outcome, world_mark};
    let mut session = FlightSession::new(neighborhood_initial()).with_recording();
    let Outcome::Spawned(id) = session.execute(Action::LaunchOrbitAt {
        body: "Beryl/aurelia".into(),
        craft: void_assembly::demo_craft(),
        offset: DVec3::ZERO,
    }) else {
        panic!("orbital fixture rejected");
    };
    session.execute(Action::Select { vessel: id.clone() });
    session.execute(Action::Stage);
    let fuel = |s: &FlightSession| {
        s.sim()
            .fleet
            .part_snapshots(&id)
            .iter()
            .map(|p| p.fuel_kg)
            .sum::<f64>()
    };
    let before = fuel(&session);
    session.execute(Action::Control {
        throttle: 0.5,
        turn: DVec3::X * 0.1,
    });
    session.execute(Action::Advance {
        seconds: 0.25,
        rails: false,
    });
    assert!(fuel(&session) < before);
    assert_eq!(session.sim().fleet.vessel_system(&id), SystemId(1));
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    let mut restored = FlightSession::from_checkpoint(saved);
    for s in [&mut session, &mut restored] {
        s.execute(Action::Advance {
            seconds: 0.25,
            rails: false,
        });
    }
    assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
}

#[test]
fn aircraft_air_data_is_local_in_remote_system_and_journal_keeps_exact_part_poses() {
    use glam::{DQuat, DVec3};
    let planet = void_landing::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        true,
    );
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut sim = initial.build();
    let mut observations = vec![];
    for (system, name) in [(SystemId(0), "Sol/aurelia"), (SystemId(1), "Beryl/aurelia")] {
        let body = sim.world.body_index(name);
        let def = &sim.fleet.ephemeris.bodies()[body];
        let radial = DVec3::X * (def.radius_meters + 20_000.0);
        let spin = def.rotation.axis() * def.rotation.rate();
        let (p, v) = sim
            .fleet
            .ephemeris
            .body_in_system(void_frames::BodyId(body), 0.0);
        let id = sim.fleet.launch_in_system(
            &void_assembly::aircraft(),
            system,
            void_landing::FrameState {
                position: p + radial,
                velocity: v + spin.cross(radial) + DVec3::Z * 100.0,
            },
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        observations.push(sim.fleet.air_data(&id));
        let marks = void_fleet_flight::session::world_mark(&sim);
        let ship = marks["ships"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        for part in ship["parts"].as_array().unwrap() {
            let pid = part["id"].as_str().unwrap();
            assert_eq!(
                part["localPose"],
                serde_json::to_value(sim.fleet.parts().part(pid).pose).unwrap()
            );
        }
    }
    assert!((observations[0].airspeed_mps - 100.0).abs() < 1e-6);
    assert!((observations[1].airspeed_mps - 100.0).abs() < 1e-6);
    assert!(
        (observations[0].dynamic_pressure_pa / observations[1].dynamic_pressure_pa - 1.0).abs()
            < 1e-8
    );
}

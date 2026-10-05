//! Combined model: RCS spending and aerodynamic torque share the existing owners.
use glam::{DQuat, DVec3};
use void_assembly::{ResourceId, add_part, rendezvous_pod};
use void_landing::{FrameState, PlanetFrame, earth_size, planet_environment, planet_ephemeris};
use void_vessels::{AirDynamics, Fleet, FleetOptions, RcsControl, VesselMode};

fn fixture(air: bool) -> Fleet {
    let p = earth_size();
    let (e, home) = planet_ephemeris(&p);
    let env = planet_environment(&p, &e, home, air);
    let state = PlanetFrame::new(&e, home).to_inertial(
        &e,
        0.,
        FrameState {
            position: DVec3::X * (p.terrain.radius_meters + 5000.),
            velocity: DVec3::Y * 80.,
        },
    );
    let mut options = FleetOptions {
        air_dynamics: AirDynamics::ForceAndTorque,
        step_seconds: 1. / 120.,
        ..Default::default()
    };
    // A bounded departure even with atmospheric drag; these ranges are captured
    // by the encounter gate at construction, as in actual world configuration.
    options.encounter.unpack_meters = 200.;
    options.encounter.pack_meters = 250.;
    let mut f = Fleet::new(e, env, 0., vec![], options);
    // Symmetric passive aero parts keep the monopropellant at COM while providing
    // real off-centre air sampling; spending can be checked against constant flow.
    let craft = add_part(
        &rendezvous_pod(),
        "aero-stabilizer-pod",
        "p1",
        "top",
        "bottom",
    )
    .unwrap();
    let craft = add_part(&craft, "aero-stabilizer-pod", "p1", "bottom", "bottom").unwrap();
    f.launch(&craft, state, DQuat::IDENTITY, DVec3::Z * 0.3);
    f.advance(0.);
    f
}

fn control() -> RcsControl {
    RcsControl {
        enabled: true,
        force: DVec3::X * 40.,
        torque: DVec3::Y * 15.,
    }
}

#[test]
fn full_air_rcs_trials_are_pure_and_each_owner_spends_only_accepted_time() {
    for bubble in [false, true] {
        let mut f = fixture(true);
        if bubble {
            let s = f.snapshot("v1");
            f.launch(
                &rendezvous_pod(),
                FrameState {
                    position: s.position + DVec3::Z * 100.,
                    velocity: s.velocity,
                },
                s.rotation,
                DVec3::ZERO,
            );
            f.advance(0.);
        }
        assert_eq!(
            f.snapshot("v1").mode,
            if bubble {
                VesselMode::Bubble
            } else {
                VesselMode::Orbit
            }
        );
        f.set_rcs_control("v1", control());
        let fuel = f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
        let flow = f.thrust("v1").flow_kg_per_second;
        assert!(flow > 0.);
        assert!(f.aerodynamic_wrench("v1").force.length() > 1.);
        assert!(f.aerodynamic_wrench("v1").torque.length() > 0.);
        let before = serde_json::to_value(f.checkpoint()).unwrap();
        for _ in 0..40 {
            let _ = f.aerodynamic_wrench("v1");
            let _ = f.rcs_allocation("v1");
            let _ = f.thrust("v1");
            assert!(f.rails_blocker().unwrap().contains("aerodynamic"));
        }
        f.advance(0.);
        assert_eq!(before, serde_json::to_value(f.checkpoint()).unwrap());
        let dt = 0.125;
        f.advance(dt);
        let remaining = f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
        assert!(
            (fuel - remaining - flow * dt).abs() < 1e-11,
            "owner={bubble}: spent {}, expected {}",
            fuel - remaining,
            flow * dt
        );
        let s = f.snapshot("v1");
        assert!(
            s.angular_velocity.y.abs() > 1e-4,
            "RCS torque must enter full-air rotation"
        );
        assert!(s.rotation.is_finite() && s.velocity.is_finite());
        if bubble {
            assert_eq!(
                f.parts().part("v2/p1").resource(ResourceId::Monopropellant),
                20.
            );
        }
    }
}

#[test]
fn combined_mode_rcs_and_air_have_independent_rails_gates_across_owner_handoffs() {
    for air in [false, true] {
        let mut f = fixture(air);
        let gate = if air { "aerodynamic" } else { "engine firing" };
        if !air {
            assert!(f.rails_blocker().is_none());
        }
        f.set_rcs_control("v1", control());
        assert!(f.rails_blocker().unwrap().contains(gate));
        let s = f.snapshot("v1");
        // A departing neighbor creates and then leaves the same rendezvous bubble.
        f.launch(
            &rendezvous_pod(),
            FrameState {
                position: s.position + DVec3::Z * 100.,
                velocity: s.velocity + DVec3::Z * 3000.,
            },
            s.rotation,
            DVec3::ZERO,
        );
        f.advance(0.);
        assert_eq!(f.snapshot("v1").mode, VesselMode::Bubble);
        let fuel = f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
        let flow = f.thrust("v1").flow_kg_per_second;
        let dt = 3.;
        f.advance(dt);
        assert_eq!(f.snapshot("v1").mode, VesselMode::Orbit);
        assert_eq!(f.rcs_control("v1"), control());
        let spent = fuel - f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
        assert!(
            (spent - flow * dt).abs() < 1e-10,
            "handoff must neither double-spend nor skip RCS"
        );
        assert!(f.rails_blocker().unwrap().contains(gate));
        f.set_rcs_control("v1", RcsControl::default());
        if air {
            assert!(
                f.rails_blocker().unwrap().contains("aerodynamic"),
                "neutral RCS cannot bypass the air gate after handoff"
            );
        } else {
            assert!(f.rails_blocker().is_none());
            let fuel = f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
            assert!(f.advance_on_rails(0.25));
            assert_eq!(
                f.parts().part("v1/p1").resource(ResourceId::Monopropellant),
                fuel
            );
        }
    }
}

#[test]
fn full_air_ground_rcs_wakes_sleep_and_updates_live_mass_only_after_acceptance() {
    use void_vessels::{GroundSpec, Scenario, create_lab_scene, flat_site};
    let scene = create_lab_scene(Scenario::Launch);
    let mut place = scene
        .fleet
        .environment()
        .body(scene.body_index)
        .unwrap()
        .clone();
    place.atmosphere = Some(void_environment::Atmosphere::earth());
    let env = std::sync::Arc::new(
        void_environment::Environment::new(&scene.fleet.ephemeris).with(scene.body_index, place),
    );
    let ground = GroundSpec {
        body_index: scene.body_index,
        band_enter_meters: 200.,
        band_exit_meters: 400.,
        tiles: void_landing::ContactWorldOptions {
            step_seconds: 1. / 60.,
            tile_level: void_landing::level_for_tile_size(scene.planet.terrain.radius_meters, 300.),
            tile_resolution: 33,
            tile_reach_meters: 300.,
            tile_keep_meters: 600.,
            recenter_meters: 5000.,
            sleeping: true,
        },
    };
    let mut f = Fleet::new(
        scene.fleet.ephemeris,
        env,
        0.,
        vec![ground],
        FleetOptions {
            air_dynamics: AirDynamics::ForceAndTorque,
            ..Default::default()
        },
    );
    f.launch_landed(
        &rendezvous_pod(),
        scene.body_index,
        flat_site(&scene.planet),
    );
    f.set_sas("v1", true);
    f.advance(30.);
    f.set_sas("v1", false);
    assert_eq!(f.snapshot("v1").mode, VesselMode::Ground);
    assert!(f.scene_snapshots().iter().all(|s| s.asleep));
    let fuel = f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
    let mass = f.snapshot("v1").mass_kg;
    f.set_rcs_control("v1", control());
    let before = serde_json::to_value(f.checkpoint()).unwrap();
    let flow = f.thrust("v1").flow_kg_per_second;
    for _ in 0..20 {
        let _ = f.aerodynamic_wrench("v1");
        let _ = f.rcs_allocation("v1");
        let _ = f.thrust("v1");
    }
    assert_eq!(before, serde_json::to_value(f.checkpoint()).unwrap());
    assert!(f.scene_snapshots().iter().all(|s| s.asleep));
    f.advance(0.2);
    let spent = fuel - f.parts().part("v1/p1").resource(ResourceId::Monopropellant);
    assert!((spent - flow * 0.2).abs() < 1e-11);
    assert!((mass - f.snapshot("v1").mass_kg - spent).abs() < 1e-11);
    assert!(f.scene_snapshots().iter().any(|s| !s.asleep));
    assert!(f.rails_blocker().is_some());
    assert_eq!(f.snapshot("v1").mode, VesselMode::Ground);
    assert!(f.snapshot("v1").rotation.is_finite());
}

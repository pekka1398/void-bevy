use glam::{DQuat, DVec3};
use void_assembly::{ModuleState, ResourceId, reentry_capsule};
use void_landing::{FrameState, PlanetFrame, earth_size, planet_environment, planet_ephemeris};
use void_vessels::{Fleet, FleetOptions, VesselMode};
fn scene(bubble: bool) -> Fleet {
    scene_at(bubble, 55000.0)
}
fn scene_at(bubble: bool, altitude: f64) -> Fleet {
    let p = earth_size();
    let (e, home) = planet_ephemeris(&p);
    let env = planet_environment(&p, &e, home, true);
    let local = FrameState {
        position: DVec3::X * (p.terrain.radius_meters + altitude),
        velocity: DVec3::new(-500., 6000., 0.),
    };
    let state = PlanetFrame::new(&e, home).to_inertial(&e, 0., local);
    let rotation = env
        .frames()
        .tree
        .at(0., &e)
        .transform(env.frames().surface[home], env.frames().origin)
        .rotation()
        * DQuat::from_rotation_arc(-DVec3::Y, local.velocity.normalize());
    let mut f = Fleet::new(
        e,
        env,
        0.,
        vec![],
        FleetOptions {
            air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
            ..Default::default()
        },
    );
    f.launch(&reentry_capsule(), state, rotation, DVec3::ZERO);
    if bubble {
        f.launch(
            &reentry_capsule(),
            FrameState {
                position: state.position + DVec3::Z * 100.,
                ..state
            },
            rotation,
            DVec3::ZERO,
        );
    }
    f.advance(0.);
    f
}
#[test]
fn accepted_ablation_updates_both_owners_and_observation_is_pure() {
    for bubble in [false, true] {
        let mut f = scene(bubble);
        assert_eq!(
            f.snapshot("v1").mode,
            if bubble {
                VesselMode::Bubble
            } else {
                VesselMode::Orbit
            }
        );
        assert!(f.rails_blocker().is_some());
        let before = f.checkpoint();
        let before = serde_json::to_value(before).unwrap();
        for _ in 0..10 {
            f.part_snapshots("v1");
            f.snapshot("v1");
            f.rails_blocker();
        }
        assert_eq!(serde_json::to_value(f.checkpoint()).unwrap(), before);
        let mass = f.snapshot("v1").mass_kg;
        f.advance(5.);
        let shield = f.parts().part("v1/shield");
        let remaining = shield.resource(ResourceId::Ablator);
        let ModuleState::Thermal { state: shield_heat } = shield.modules["thermal"] else {
            unreachable!()
        };
        let ModuleState::Thermal { state: pod_heat } = f.parts().part("v1/pod").modules["thermal"]
        else {
            unreachable!()
        };
        println!("bubble={bubble} shield={shield_heat:?} pod={pod_heat:?} ablator={remaining}");
        assert!((0.0..30.0).contains(&remaining));
        assert!((f.snapshot("v1").mass_kg - (mass - 30. + remaining)).abs() < 1e-8);
        assert!(
            shield_heat.skin_k > pod_heat.skin_k + 100.,
            "shield must protect downstream pod"
        );
        // A second accepted advance checks owner cache mass agrees with the new graph mass.
        f.advance(0.25);
        assert!(f.inertia("v1").iter().all(|v| v.is_finite()));
    }
}

#[test]
fn rails_stop_before_air_entry_without_spending_material_in_a_quiet_chunk() {
    let mut f = scene_at(false, 400000.0);
    assert!(f.rails_blocker().is_none());
    assert!(!f.advance_on_rails(1000.0));
    let height =
        f.snapshot("v1").position.length() - void_landing::earth_size().terrain.radius_meters;
    assert!(
        height > 120000.0,
        "thermal rails gate must stop before physical air, got {height}"
    );
    assert_eq!(
        f.parts().part("v1/shield").resource(ResourceId::Ablator),
        30.0
    );
    let ModuleState::Thermal { state } = f.parts().part("v1/shield").modules["thermal"] else {
        unreachable!()
    };
    assert_eq!(state.skin_k, 270.0);
    assert!(
        f.rails_blocker()
            .unwrap()
            .contains("approaching atmosphere")
    );
}

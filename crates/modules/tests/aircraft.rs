use glam::{DQuat, DVec3};
use void_assembly::*;
use void_frames::State;
use void_modules::vessel_air;

fn wings() -> (PartGraph, Vec<String>) {
    let mut craft = fresh_craft();
    craft.version = 3;
    craft.parts[0].definition_id = "aircraft-cockpit".into();
    craft.parts[0].resources.clear();
    let mount = SurfaceMount {
        definition_id: "aircraft-wing".into(),
        parent_socket_id: "surface-0".into(),
        node_id: "root".into(),
        pose: PartPose {
            position: DVec3::new(2.6, 0.0, 0.0),
            rotation: DQuat::IDENTITY,
        },
    };
    let craft = mount_mirrored_pair(
        &craft,
        "p1",
        &mount,
        "aircraft-wing-left",
        "surface-1",
        "root",
    )
    .unwrap();
    let mut graph = PartGraph::new();
    let ids = graph.add(&compile(&craft).unwrap(), "plane");
    (graph, ids)
}
#[test]
fn mirror_pair_adds_lift_cancels_roll_and_deflection_produces_opposite_roll() {
    let planet = void_testkit::earth_size();
    let (ephemeris, home) = void_testkit::planet_ephemeris(&planet);
    let environment = void_testkit::planet_environment(&planet, &ephemeris, home, true);
    let (graph, ids) = wings();
    let at = environment.frames().tree.at(0.0, &ephemeris);
    let query = environment.frames().surface[home];
    // Locate at the pole so equal-x wing points have equal sampled atmosphere.
    let state = State {
        position: DVec3::Y * (planet.terrain.radius_meters + 1000.0),
        velocity: DVec3::Z * 50.0,
    };
    let source = || vessel_air(&environment, &graph, &ids, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    let neutral = source().wrench_in(&at, query, state, DQuat::IDENTITY, DVec3::ZERO);
    assert!(neutral.force.y > 1000.0, "{neutral:?}");
    assert!(neutral.torque.z.abs() < 1e-8, "{neutral:?}");
    let right = source().with_controls(DVec3::Z * 0.5).wrench_in(
        &at,
        query,
        state,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let left = source().with_controls(-DVec3::Z * 0.5).wrench_in(
        &at,
        query,
        state,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    assert!(
        right.torque.z > 1000.0 && left.torque.z < -1000.0,
        "{right:?} {left:?}"
    );
    assert!((right.torque.z + left.torque.z).abs() < 1e-8);
    assert_eq!(graph.part(&ids[1]).modules["lift"], ModuleState::Passive);
}
#[test]
fn cuboid_face_pressure_uses_true_projected_area_and_dissipates_energy() {
    let (graph, ids) = wings();
    let elements = void_modules::body::elements(&graph, &ids, &ids[1], DVec3::ZERO);
    assert_eq!(elements.len(), 3);
    let areas: Vec<f64> = elements
        .iter()
        .map(|e| match &e.shape {
            void_aero::AeroShape::Body(b) => b.front_area,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(areas, vec![0.12 * 1.5, 4.0 * 1.5, 4.0 * 0.12]);
    let velocity = DVec3::new(10., 20., 30.);
    let loads = void_aero::aerodynamic_forces(
        &elements,
        &void_aero::AeroState {
            center: DVec3::ZERO,
            velocity,
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        },
        &void_environment::Atmosphere::earth().sample(0.0),
        DVec3::ZERO,
        &void_aero::NEUTRAL,
    );
    assert!(loads.force.dot(velocity) < 0.0);
}

#[test]
fn jet_requires_air_and_scales_force_and_accepted_supply_rate() {
    let mut craft = fresh_craft();
    craft.version = 3;
    craft.parts[0].definition_id = "aircraft-cockpit".into();
    craft.parts[0].resources.clear();
    let craft = add_part(&craft, "aircraft-fuselage", "p1", "tail", "front").unwrap();
    let craft = add_part(&craft, "aircraft-jet", "p2", "rear", "front").unwrap();
    let mut graph = PartGraph::new();
    let ids = graph.add(&compile(&craft).unwrap(), "plane");
    let jet = graph.part(&ids[2]);
    let vacuum = void_modules::engine::thrust(jet, 1.0, &void_modules::Conditions::VACUUM);
    assert_eq!(vacuum.force, DVec3::ZERO);
    assert_eq!(vacuum.flow_kg_per_second, 0.0);
    let air = void_environment::Air {
        density: 1.225,
        ..void_environment::Atmosphere::earth().sample(0.0)
    };
    let conditions = void_modules::Conditions {
        air: Some(void_environment::AirSample {
            altitude: 0.0,
            air,
            airspeed: DVec3::ZERO,
        }),
    };
    let low = void_modules::Conditions {
        air: Some(void_environment::AirSample {
            air: void_environment::Air {
                density: air.density / 2.0,
                ..air
            },
            ..conditions.air.unwrap()
        }),
    };
    let full = void_modules::engine::thrust(jet, 1.0, &conditions);
    let half = void_modules::engine::thrust(jet, 1.0, &low);
    assert!((half.force.length() * 2.0 - full.force.length()).abs() < 1e-8);
    assert!((half.flow_kg_per_second * 2.0 - full.flow_kg_per_second).abs() < 1e-12);
    assert_eq!(jet.resources.len(), 0);
    assert_eq!(
        graph.part(&ids[1]).resource(ResourceId::LiquidPropellant),
        200.0
    );
}

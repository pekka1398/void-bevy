use glam::{DQuat, DVec3};
use void_assembly::*;

#[test]
fn rectangular_geometry_has_matching_principal_inertia_and_bounds() {
    let wing = definition("aircraft-wing").unwrap();
    assert_eq!(part_box_size(wing), DVec3::new(4.0, 0.12, 1.5));
    let expected = DVec3::new(
        0.12_f64.powi(2) + 1.5_f64.powi(2),
        4.0_f64.powi(2) + 1.5_f64.powi(2),
        4.0_f64.powi(2) + 0.12_f64.powi(2),
    ) / 12.0;
    assert_eq!(part_inertia_per_kg(wing), expected);
    assert_eq!(part_bound_radius(wing), part_box_size(wing).length() / 2.0);
    let mut bad = wing.clone();
    bad.box_size_meters = Some(DVec3::new(0.0, 1.0, 1.0));
    assert!(validate_definition(&bad).is_err());
}

fn aircraft() -> Craft {
    serde_json::from_value(serde_json::json!({"version":3,"name":"geometry test","parts":[
        {"id":"cockpit","definitionId":"aircraft-cockpit","resources":{},"stage":null,"attachment":null},
        {"id":"wing","definitionId":"aircraft-wing","resources":{},"stage":null,
            "attachment":{"parentId":"cockpit","parentNodeId":"surface-0","nodeId":"root"}}
    ]})).unwrap()
}

#[test]
fn surface_weld_preserves_graph_and_rejects_detached_or_interior_mounts() {
    let craft = aircraft();
    let pose = PartPose {
        position: DVec3::new(2.6, 0.0, 0.0),
        rotation: DQuat::IDENTITY,
    };
    let placed = set_attachment_pose(&craft, "wing", pose).unwrap();
    let compiled = compile(&placed).unwrap();
    assert_eq!(compiled.part("wing").pose.position, pose.position);
    assert_eq!(compiled.connections.len(), 1);
    let mut graph = PartGraph::new();
    let ids = graph.add(&compiled, "aircraft");
    assert_eq!(graph.components(&ids).len(), 1);
    assert!(
        set_attachment_pose(
            &craft,
            "wing",
            PartPose {
                position: DVec3::ZERO,
                ..pose
            }
        )
        .is_err()
    );
    assert!(
        set_attachment_pose(
            &craft,
            "wing",
            PartPose {
                position: DVec3::new(20.0, 0.0, 0.0),
                ..pose
            }
        )
        .is_err()
    );
    assert!(
        set_attachment_pose(
            &craft,
            "wing",
            PartPose {
                rotation: DQuat::from_rotation_y(1.0),
                ..pose
            }
        )
        .is_err()
    );
    let mut legacy = placed.clone();
    legacy.version = 2;
    assert!(compile(&legacy).is_err());
    let restored: Craft = serde_json::from_str(&serde_json::to_string(&placed).unwrap()).unwrap();
    assert_eq!(
        compile(&restored).unwrap().part("wing").pose.position,
        pose.position
    );
}

#[test]
fn mirrored_pose_is_involution_and_node_twist_retains_connection_point() {
    let pose = PartPose {
        position: DVec3::new(3.0, 1.0, -2.0),
        rotation: DQuat::from_rotation_y(0.3),
    };
    let twice = mirror_pose_x(mirror_pose_x(pose).unwrap()).unwrap();
    assert!((twice.rotation.dot(pose.rotation).abs() - 1.0).abs() < 1e-12);
    assert_eq!(twice.position, pose.position);
    let craft = aircraft();
    let twisted = set_attachment_twist(&craft, "wing", 0.4).unwrap();
    let c = compile(&twisted).unwrap();
    let p = c.part("wing");
    let socket = node(p.definition, "root").unwrap();
    assert!(
        (p.pose.position + p.pose.rotation * socket.position - DVec3::new(0.6, 0.0, 0.0)).length()
            < 1e-12
    );
}

#[test]
fn public_mirror_pair_builder_has_distinct_identity_and_atomic_rejection() {
    let mut craft = aircraft();
    craft.parts.pop();
    let mount = SurfaceMount {
        definition_id: "aircraft-wing".into(),
        parent_socket_id: "surface-0".into(),
        node_id: "root".into(),
        pose: PartPose {
            position: DVec3::new(2.6, 0.0, 0.0),
            rotation: DQuat::IDENTITY,
        },
    };
    let pair = mount_mirrored_pair(
        &craft,
        "cockpit",
        &mount,
        "aircraft-wing-left",
        "surface-1",
        "root",
    )
    .unwrap();
    let compiled = compile(&pair).unwrap();
    assert_ne!(pair.parts[1].id, pair.parts[2].id);
    assert_eq!(
        compiled.parts[1].pose.position.x,
        -compiled.parts[2].pose.position.x
    );
    assert_eq!(compiled.connections.len(), 2);
    assert!(
        mount_mirrored_pair(
            &craft,
            "cockpit",
            &mount,
            "aircraft-wing-left",
            "surface-0",
            "root"
        )
        .is_err()
    );
    assert_eq!(craft.parts.len(), 1);
}

#[test]
fn authored_airframe_uses_the_same_public_connections_and_supply() {
    let craft = aircraft_airframe();
    let compiled = compile(&craft).unwrap();
    assert_eq!(compiled.parts.len(), 8);
    let mut graph = PartGraph::new();
    let ids = graph.add(&compiled, "aircraft");
    assert_eq!(graph.components(&ids).len(), 1);
    assert_eq!(
        graph.resource_tanks(&ids, &ids[7], ResourceId::LiquidPropellant),
        vec![ids[1].clone()]
    );
    assert!(
        !graph
            .part(&ids[0])
            .definition
            .modules
            .iter()
            .any(|m| matches!(
                m,
                Module::Command {
                    reaction_wheel: true,
                    ..
                }
            ))
    );
}

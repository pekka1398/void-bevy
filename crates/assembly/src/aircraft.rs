//! A reusable authored aircraft assembled entirely through the public geometry API.
use crate::*;
use glam::{DQuat, DVec3};

/// Main aircraft structure. Landing gear is added by `aircraft()` using the shared Wheel module.
/// This airframe helper is also useful for aerodynamic diagnostics without a second runtime.
pub fn aircraft_airframe() -> Craft {
    let mut craft = fresh_craft();
    craft.version = 3;
    craft.name = "A-02 modular jet aircraft".into();
    craft.parts[0].definition_id = "aircraft-cockpit".into();
    craft.parts[0].resources.clear();
    craft =
        add_part(&craft, "aircraft-fuselage", "p1", "tail", "front").expect("aircraft fuselage");
    craft = mount_mirrored_pair(
        &craft,
        "p2",
        &SurfaceMount {
            definition_id: "aircraft-wing".into(),
            parent_socket_id: "surface-3".into(),
            node_id: "root".into(),
            pose: PartPose {
                position: DVec3::new(2.5, 0.0, 1.0),
                rotation: DQuat::IDENTITY,
            },
        },
        "aircraft-wing-left",
        "surface-4",
        "root",
    )
    .expect("aircraft wings");
    craft = mount_mirrored_pair(
        &craft,
        "p2",
        &SurfaceMount {
            definition_id: "aircraft-elevator-right".into(),
            parent_socket_id: "surface-0".into(),
            node_id: "root".into(),
            pose: PartPose {
                position: DVec3::new(1.25, 0.0, -0.8),
                rotation: DQuat::IDENTITY,
            },
        },
        "aircraft-elevator-left",
        "surface-1",
        "root",
    )
    .expect("aircraft horizontal tail");
    craft = mount_surface(
        &craft,
        "p2",
        &SurfaceMount {
            definition_id: "aircraft-rudder".into(),
            parent_socket_id: "surface-2".into(),
            node_id: "root".into(),
            pose: PartPose {
                position: DVec3::new(0.0, 1.25, -0.8),
                rotation: DQuat::IDENTITY,
            },
        },
    )
    .expect("aircraft rudder");
    craft = add_part(&craft, "aircraft-jet", "p2", "rear", "front").expect("aircraft jet");
    craft
}

/// Complete airplane: the same PartGraph carries passive shared Wheel landing gear.
pub fn aircraft() -> Craft {
    let craft = aircraft_airframe();
    let craft = mount_mirrored_pair(
        &craft,
        "p2",
        &SurfaceMount {
            definition_id: "aircraft-gear-right".into(),
            parent_socket_id: "surface-5".into(),
            node_id: "root".into(),
            pose: PartPose {
                position: DVec3::new(0.5, -0.4, 0.0),
                rotation: DQuat::IDENTITY,
            },
        },
        "aircraft-gear-left",
        "surface-6",
        "root",
    )
    .expect("aircraft main gear");
    mount_surface(
        &craft,
        "p1",
        &SurfaceMount {
            definition_id: "aircraft-gear-nose".into(),
            parent_socket_id: "surface-2".into(),
            node_id: "root".into(),
            pose: PartPose {
                position: DVec3::new(0.0, -0.4, 1.5),
                rotation: DQuat::IDENTITY,
            },
        },
    )
    .expect("aircraft nose gear")
}

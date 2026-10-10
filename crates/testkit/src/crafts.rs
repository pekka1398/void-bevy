//! Craft only the tests fly. The game's own craft are `void_assembly::rcs_flight_rocket`, the
//! editor's `demo_craft` and the files in `crafts/`.
use glam::{DQuat, DVec3};
use std::collections::BTreeMap;
use void_assembly::*;

/// A self-contained near-rendezvous vehicle, using the same authored catalog as other craft.
pub fn rendezvous_pod() -> Craft {
    let mut craft = fresh_craft();
    craft.name = "Rendezvous pod".into();
    craft.parts[0].definition_id = "rcs-pod".into();
    craft.parts[0].resources = full_resources(definition("rcs-pod").expect("authored pod"));
    craft
}

/// Configurable four-wheel main-game acceptance craft, using ordinary surface assembly APIs.
pub fn rover() -> Craft {
    import_craft(include_str!("../data/rover.json")).expect("invalid rover fixture")
}

/// Rover with a dedicated crew seat and isolated backpack tank, ordinary surface assembly.
pub fn crew_rover() -> Craft {
    let mut base = rover();
    base.parts[0].definition_id = "rover-crewed-chassis".into();
    mount_surface(
        &base,
        "chassis",
        &SurfaceMount {
            definition_id: "crew-seat".into(),
            parent_socket_id: "surface-4".into(),
            node_id: "mount".into(),
            pose: PartPose {
                position: DVec3::new(0.0, 0.675, 0.0),
                rotation: DQuat::IDENTITY,
            },
        },
    )
    .expect("invalid crew rover fixture")
}

/// Existing two-stage rocket carrying the same reusable seat/hatch and finite backpack tank.
/// The external seat is an explicit first-round fixture; there is no cabin traversal model.
pub fn crewed_flight_rocket() -> Craft {
    let mut base = rcs_flight_rocket();
    base.version = 3;
    base.name = "VOID crewed two-stage rocket".into();
    base.parts
        .iter_mut()
        .find(|p| p.id == "p1")
        .unwrap()
        .definition_id = "crewed-flight-rcs-pod".into();
    add_part(&base, "crew-seat", "p1", "crew-seat", "side").expect("invalid crewed rocket fixture")
}

/// A command pod with a finite ablative disk on its bottom stack node.
pub fn reentry_capsule() -> Craft {
    let craft = Craft {
        version: 2,
        name: "VOID shielded reentry capsule".into(),
        parts: vec![
            PartInstance {
                id: "pod".into(),
                definition_id: "flight-rcs-pod".into(),
                resources: full_resources(definition("flight-rcs-pod").unwrap()),
                module_stages: BTreeMap::new(),
                stage: None,
                attachment: None,
            },
            PartInstance {
                id: "shield".into(),
                definition_id: "heat-shield".into(),
                resources: full_resources(definition("heat-shield").unwrap()),
                module_stages: BTreeMap::new(),
                stage: None,
                attachment: Some(Attachment {
                    parent_id: "pod".into(),
                    parent_node_id: "bottom".into(),
                    node_id: "top".into(),
                    twist_radians: 0.0,
                    pose: None,
                }),
            },
        ],
    };
    compile(&craft).expect("authored reentry capsule");
    craft
}

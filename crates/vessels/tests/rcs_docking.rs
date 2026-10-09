mod common;
use common::{Setup, scene};
use glam::{DMat3, DQuat, DVec3};
use void_assembly::*;
use void_modules::{
    Conditions,
    rcs::{RcsControl, allocate},
};
use void_vessels::*;
fn graph() -> (PartGraph, Vec<String>) {
    let mut g = PartGraph::new();
    let ids = g.add(&compile(&rendezvous_pod()).unwrap(), "test");
    (g, ids)
}
#[test]
fn allocator_translation_rotation_saturation_and_fuel() {
    let (mut g, ids) = graph();
    for (force, torque) in [
        (DVec3::X * 80.0, DVec3::ZERO),
        (DVec3::ZERO, DVec3::Y * 30.0),
        (DVec3::new(30., -40., 50.), DVec3::new(10., 20., -10.)),
    ] {
        let a = allocate(
            &g,
            &ids,
            DVec3::ZERO,
            RcsControl {
                enabled: true,
                force,
                torque,
            },
        );
        assert!(a.force_residual.length() < 1e-5, "{:?}", a.force_residual);
        assert!(a.torque_residual.length() < 1e-5, "{:?}", a.torque_residual);
        assert!(a.nozzles.iter().all(|n| (0.0..=1.0).contains(&n.throttle)));
    }
    let request = RcsControl {
        enabled: true,
        force: DVec3::X * 10000.,
        torque: DVec3::ZERO,
    };
    let a = allocate(&g, &ids, DVec3::ZERO, request);
    assert!(a.force.x <= 320. + 1e-8);
    assert!(a.force_residual.x > 9600.);
    let p = rcs_propulsion(&g, &ids, DVec3::ZERO, request);
    assert_eq!(p.groups.len(), 1);
    let mono = g.part(&ids[0]).resource(ResourceId::Monopropellant);
    burn(&mut g, &p.groups, 1.0);
    assert!(
        (g.part(&ids[0]).resource(ResourceId::Monopropellant) - (mono - p.flow_kg_per_second))
            .abs()
            < 1e-12
    );
    g.set_resource(&ids[0], ResourceId::Monopropellant, 0.001);
    let p = rcs_propulsion(&g, &ids, DVec3::ZERO, request);
    let (force, _, used) = step_thrust(&p, 1.0, DVec3::ZERO);
    assert!(force.length() < p.force.length());
    assert_eq!(used, 0.001);
    burn(&mut g, &p.groups, 1.0);
    assert!(allocate(&g, &ids, DVec3::ZERO, request).nozzles.is_empty());
    assert_eq!(
        propulsion(&g, &ids, 1., DVec3::ZERO, &Conditions::VACUUM).force,
        DVec3::ZERO
    );
}
fn pair(offset: DVec3, velocity: DVec3, rotation: DQuat, spin: DVec3) -> Fleet {
    let mut s = scene(Setup::Coast);
    // Keep existing ships distant; tests exercise true docking modules on newly launched craft.
    let snap = s.fleet.snapshot("v1");
    let craft = rendezvous_pod();
    let p = snap.position + DVec3::X * 1000.;
    s.fleet.launch(
        &craft,
        void_frames::State {
            position: p,
            velocity: snap.velocity,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    s.fleet.launch(
        &craft,
        void_frames::State {
            position: p + offset,
            velocity: snap.velocity + velocity,
        },
        rotation,
        spin,
    );
    s.fleet
}
#[test]
fn capture_boundaries_are_explicit_and_do_not_mutate_graph() {
    for (offset, velocity, rotation, spin, reason) in [
        (
            DVec3::Y * 2.201,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::ZERO,
            "distance",
        ),
        (
            DVec3::Y * 2.1,
            DVec3::X * 0.401,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::ZERO,
            "speed",
        ),
        (
            DVec3::Y * 0.1,
            DVec3::ZERO,
            DQuat::IDENTITY,
            DVec3::ZERO,
            "directions",
        ),
        (
            DVec3::Y * 2.1,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::Y * 0.101,
            "spin",
        ),
    ] {
        let mut f = pair(offset, velocity, rotation, spin);
        let count = f.parts().connections().len();
        let e = f.dock("v3/p1", "dock", "v4/p1", "dock").unwrap_err();
        assert!(e.contains(reason), "{e}");
        assert_eq!(count, f.parts().connections().len());
    }
}
#[test]
fn capture_split_preserves_ids_pose_momentum_and_checkpoint_continuation() {
    let mut f = pair(
        DVec3::Y * 2.1,
        DVec3::X * 0.1,
        DQuat::from_rotation_x(std::f64::consts::PI),
        DVec3::ZERO,
    );
    let ss = [f.snapshot("v3"), f.snapshot("v4")];
    let before: Vec<_> = ["v3", "v4"]
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect();
    let momentum = ss[0].velocity * ss[0].mass_kg + ss[1].velocity * ss[1].mass_kg;
    let relative = f.relative("v4", "v3");
    let mass = ss[0].mass_kg + ss[1].mass_kg;
    let com = relative.position * ss[1].mass_kg / mass;
    let vcom = relative.velocity * ss[1].mass_kg / mass;
    let angular_before = (-com).cross(-vcom) * ss[0].mass_kg
        + (relative.position - com).cross(relative.velocity - vcom) * ss[1].mass_kg;
    let joined = f.dock("v3/p1", "dock", "v4/p1", "dock").unwrap();
    let after = f.snapshot(&joined);
    assert!((after.velocity * after.mass_kg - momentum).length() < 0.01);
    let rot = DMat3::from_quat(after.rotation);
    let inertia = DMat3::from_cols_array(&f.inertia(&joined)).transpose();
    let angular_after = rot * inertia * rot.transpose() * after.angular_velocity;
    assert!(
        (angular_after - angular_before).length() < 1e-3,
        "angular {:?} {:?}",
        angular_after,
        angular_before
    );
    for p in before {
        let a = f
            .part_snapshots(&joined)
            .into_iter()
            .find(|a| a.id == p.id)
            .unwrap();
        assert!((a.position - p.position).length() < 1e-4);
        assert!(a.rotation.dot(p.rotation).abs() > 1. - 1e-10);
    }
    assert!(f.dock("v3/p1", "dock", "v4/p1", "dock").is_err());
    f.set_rcs_control(
        &joined,
        RcsControl {
            enabled: true,
            force: DVec3::X * 20.,
            torque: DVec3::Y * 5.,
        },
    );
    let saved = serde_json::to_string(&f.checkpoint()).unwrap();
    let mut restored = Fleet::from_checkpoint(
        void_landing::planet_ephemeris(&void_landing::aurelia()).0,
        f.environment().clone(),
        serde_json::from_str(&saved).unwrap(),
    );
    f.advance(0.1);
    restored.advance(0.1);
    assert!((f.snapshot(&joined).position - restored.snapshot(&joined).position).length() < 1e-9);
    assert_eq!(f.rcs_control(&joined), restored.rcs_control(&joined));
    let pre_parts = f.part_snapshots(&joined);
    let pre = f.snapshot(&joined);
    let detached = f.undock("v3/p1", "dock").unwrap();
    let a = f.snapshot(&joined);
    let b = f.snapshot(&detached);
    assert!(
        (a.velocity * a.mass_kg + b.velocity * b.mass_kg - pre.velocity * pre.mass_kg).length()
            < 0.1
    );
    for original in pre_parts {
        let id = f.vessel_of_part(&original.id);
        let part = f
            .part_snapshots(&id)
            .into_iter()
            .find(|p| p.id == original.id)
            .unwrap();
        assert_eq!(part.resources, original.resources);
        assert!((part.position - original.position).length() < 1e-4);
        assert!(part.rotation.dot(original.rotation).abs() > 1.0 - 1e-10);
    }
    assert_eq!(
        f.parts().part("v3/p1").modules["dock"],
        ModuleState::DockingPort { armed: false }
    );
    assert!(
        f.dock("v3/p1", "dock", "v4/p1", "dock")
            .unwrap_err()
            .contains("disarmed")
    );
}
#[test]
fn addressed_ports_disarmed_occupied_and_rotating_tip_speed() {
    // Same part definition has two distinct ports. Bottom points towards the first top.
    let mut f = pair(DVec3::Y * 2.1, DVec3::ZERO, DQuat::IDENTITY, DVec3::ZERO);
    assert!(
        f.dock("v3/p1", "dock", "v3/p1", "dock-bottom")
            .unwrap_err()
            .contains("self")
    );
    f.arm_docking_port("v3/p1", "dock", false);
    assert!(
        f.dock("v3/p1", "dock", "v4/p1", "dock-bottom")
            .unwrap_err()
            .contains("disarmed")
    );
    f.arm_docking_port("v3/p1", "dock", true);
    assert!(f.dock("v3/p1", "dock", "v4/p1", "dock-bottom").is_ok());
    let pose = f.snapshot("v3");
    let third = f.launch(
        &rendezvous_pod(),
        void_frames::State {
            position: pose.position,
            velocity: pose.velocity,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    assert!(
        f.dock("v3/p1", "dock", &format!("{third}/p1"), "dock-bottom")
            .unwrap_err()
            .contains("occupied")
    );
    let mut f = pair(
        DVec3::Y * 2.1,
        DVec3::ZERO,
        DQuat::from_rotation_x(std::f64::consts::PI),
        DVec3::Z * 0.5,
    );
    // COM velocities match, but omega cross r exceeds the capture speed limit.
    assert!(
        f.dock("v3/p1", "dock", "v4/p1", "dock")
            .unwrap_err()
            .contains("speed")
    );
    let mut f = pair(
        DVec3::Y * 2.1,
        DVec3::ZERO,
        DQuat::from_rotation_x(std::f64::consts::PI + 0.13),
        DVec3::ZERO,
    );
    assert!(
        f.dock("v3/p1", "dock", "v4/p1", "dock")
            .unwrap_err()
            .contains("directions")
    );
}
#[test]
fn asymmetric_and_multi_resource_nozzles_are_actual_consumers() {
    let (mut g, ids) = graph();
    let disabled: Vec<_> = g
        .part(&ids[0])
        .definition
        .modules
        .iter()
        .filter_map(|m| match m {
            Module::Rcs { id, direction, .. } if direction.x > 0. => Some(id.clone()),
            _ => None,
        })
        .collect();
    for id in disabled {
        g.set_module_state(&ids[0], &id, ModuleState::Rcs { enabled: false });
    }
    let request = RcsControl {
        enabled: true,
        force: DVec3::X * 80.,
        torque: DVec3::ZERO,
    };
    let a = allocate(&g, &ids, DVec3::ZERO, request);
    assert_eq!(a.force, DVec3::ZERO);
    assert_eq!(a.force_residual, request.force);
    let mut d = definition("rcs-pod").unwrap().clone();
    d.modules.push(Module::Tank {
        id: "liquid".into(),
        resource: ResourceId::LiquidPropellant,
        capacity_kg: 5.,
    });
    for m in &mut d.modules {
        if let Module::Rcs { id, resource, .. } = m
            && (id.starts_with("rcs-0") || id.starts_with("rcs-1"))
        {
            *resource = ResourceId::LiquidPropellant;
        }
    }
    d.modules.push(Module::Engine {
        id: "shared-mono-engine".into(),
        jet: None,
        resource: ResourceId::Monopropellant,
        thrust_newtons: 80.0,
        isp_seconds: 240.0,
        nozzle_exit_area_m2: 0.0,
        direction: DVec3::Y,
    });
    let d = Box::leak(Box::new(d));
    let mut g = PartGraph::new();
    g.insert(Part {
        id: "mixed".into(),
        definition: d,
        resources: full_resources(d),
        modules: initial_modules(d),
        stage: None,
        module_stages: default_module_stages(d, None),
        pose: PartPose {
            position: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
        },
    });
    let ids = vec!["mixed".into()];
    let request = RcsControl {
        enabled: true,
        force: DVec3::X * 200.,
        torque: DVec3::ZERO,
    };
    g.stage_module("mixed", "shared-mono-engine");
    let p = rcs_propulsion(&g, &ids, DVec3::ZERO, request).combined(propulsion(
        &g,
        &ids,
        1.0,
        DVec3::ZERO,
        &Conditions::VACUUM,
    ));
    assert_eq!(p.groups.len(), 2);
    let mono_flow = p
        .groups
        .iter()
        .find(|g| g.resource == ResourceId::Monopropellant)
        .unwrap()
        .flow_kg_per_second;
    let liquid_flow = p
        .groups
        .iter()
        .find(|g| g.resource == ResourceId::LiquidPropellant)
        .unwrap()
        .flow_kg_per_second;
    burn(&mut g, &p.groups, 1.);
    assert!(g.part("mixed").resource(ResourceId::Monopropellant) < 20.);
    assert!(g.part("mixed").resource(ResourceId::LiquidPropellant) < 5.);
    assert!(
        (g.part("mixed").resource(ResourceId::Monopropellant) - (20.0 - mono_flow)).abs() < 1e-12
    );
    assert!(
        (g.part("mixed").resource(ResourceId::LiquidPropellant) - (5.0 - liquid_flow)).abs()
            < 1e-12
    );
    g.set_resource("mixed", ResourceId::LiquidPropellant, 0.);
    let a = allocate(&g, &ids, DVec3::ZERO, request);
    assert!(
        a.nozzles
            .iter()
            .all(|n| n.resource == ResourceId::Monopropellant)
    );
}
#[test]
fn rcs_rotation_uses_physical_torque_and_empty_fuel_has_no_authority() {
    let mut f = pair(DVec3::Y * 20., DVec3::ZERO, DQuat::IDENTITY, DVec3::ZERO);
    f.set_rcs_control(
        "v3",
        RcsControl {
            enabled: true,
            force: DVec3::ZERO,
            torque: DVec3::Y * 20.,
        },
    );
    let before = f.snapshot("v3");
    f.advance(0.1);
    let after = f.snapshot("v3");
    assert!(after.angular_velocity.y > 0.01);
    assert!(after.rotation.dot(before.rotation).abs() < 1.);
    assert_eq!(f.control("v3").turn, DVec3::ZERO);
    assert!(f.parts().part("v3/p1").resource(ResourceId::Monopropellant) < 20.);
}

#[test]
fn an_empty_pod_never_generates_force_or_rotation() {
    let mut f = pair(DVec3::Y * 20.0, DVec3::ZERO, DQuat::IDENTITY, DVec3::ZERO);
    let mut craft = rendezvous_pod();
    craft.parts[0]
        .resources
        .insert(ResourceId::Monopropellant, 0.0);
    let old = f.snapshot("v3");
    let id = f.launch(
        &craft,
        void_frames::State {
            position: old.position + DVec3::X * 5000.0,
            velocity: old.velocity,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    f.set_rcs_control(
        &id,
        RcsControl {
            enabled: true,
            force: DVec3::X * 40.0,
            torque: DVec3::Y * 20.0,
        },
    );
    assert_eq!(f.thrust(&id).force, DVec3::ZERO);
    assert_eq!(f.thrust(&id).torque, DVec3::ZERO);
    f.advance(0.1);
    assert_eq!(f.snapshot(&id).angular_velocity, DVec3::ZERO);
}
#[test]
fn shared_crossfeed_only_supplies_rcs_in_the_connected_open_group() {
    let craft = add_part(&rendezvous_pod(), "rcs-pod", "p1", "bottom", "top").unwrap();
    let compiled = compile(&craft).unwrap();
    let mut open = PartGraph::new();
    let ids = open.add(&compiled, "feed");
    open.set_resource(&ids[0], ResourceId::Monopropellant, 0.0);
    let modules: Vec<_> = open
        .part(&ids[1])
        .definition
        .modules
        .iter()
        .filter(|m| matches!(m, Module::Rcs { .. }))
        .map(|m| m.id().to_string())
        .collect();
    for module in modules {
        open.set_module_state(&ids[1], &module, ModuleState::Rcs { enabled: false });
    }
    let control = RcsControl {
        enabled: true,
        force: DVec3::X * 40.0,
        torque: DVec3::ZERO,
    };
    assert!(rcs_propulsion(&open, &ids, DVec3::ZERO, control).force.x > 1.0);
    assert_eq!(
        rcs_propulsion(&open, &ids[..1], DVec3::ZERO, control).force,
        DVec3::ZERO
    );
    let mut blocked = PartGraph::new();
    for part in open.parts() {
        let mut part = part.clone();
        if part.id == ids[1] {
            let mut d = part.definition.clone();
            d.crossfeed = false;
            part.definition = Box::leak(Box::new(d));
        }
        blocked.insert(part);
    }
    blocked.restore_connections(open.connections().to_vec());
    assert_eq!(
        rcs_propulsion(&blocked, &ids, DVec3::ZERO, control).force,
        DVec3::ZERO
    );
}
#[test]
fn just_inside_capture_limits_succeeds() {
    for (offset, velocity, rotation, spin) in [
        (
            DVec3::Y * 2.199,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::ZERO,
        ),
        (
            DVec3::Y * 2.1,
            DVec3::X * 0.399,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::ZERO,
        ),
        (
            DVec3::Y * 2.1,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::PI + 0.119),
            DVec3::ZERO,
        ),
        (
            DVec3::Y * 2.1,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::PI),
            DVec3::Y * 0.099,
        ),
    ] {
        let mut f = pair(offset, velocity, rotation, spin);
        assert!(f.dock("v3/p1", "dock", "v4/p1", "dock").is_ok());
    }
}

/// Two equal asymmetric supplies accelerate together, keeping a bubble owner throughout the burn.
/// Only the upper pod has fuel, so mass depletion moves the live scene COM away from its parts origin.
fn depleted_asymmetric_scene() -> (Fleet, String) {
    let mut fleet = scene(Setup::Coast).fleet;
    let start = fleet.snapshot("v1");
    let mut craft = add_part(&rendezvous_pod(), "rcs-pod", "p1", "bottom", "top").unwrap();
    craft.parts[1]
        .resources
        .insert(ResourceId::Monopropellant, 0.0);
    let mut ships = vec![];
    for i in 0..2 {
        let id = fleet.launch(
            &craft,
            void_frames::State {
                position: start.position + DVec3::X * (1000.0 + i as f64 * 100.0),
                velocity: start.velocity,
            },
            DQuat::IDENTITY,
            DVec3::Z * 0.08,
        );
        fleet.set_rcs_control(
            &id,
            RcsControl {
                enabled: true,
                force: DVec3::Y * 320.0,
                torque: DVec3::ZERO,
            },
        );
        ships.push(id);
    }
    fleet.advance(0.0);
    assert_eq!(fleet.snapshot(&ships[0]).mode, VesselMode::Bubble);
    assert!(fleet.centre_of_mass_local(&ships[0]).length() < 1e-6);
    fleet.advance(30.0);
    for id in &ships {
        fleet.set_rcs_control(id, RcsControl::default());
    }
    let id = ships.remove(0);
    assert_eq!(fleet.snapshot(&id).mode, VesselMode::Bubble);
    assert!(
        fleet
            .parts()
            .part(&format!("{id}/p1"))
            .resource(ResourceId::Monopropellant)
            < 17.0
    );
    assert!(
        fleet.centre_of_mass_local(&id).length() > 0.01,
        "fuel depletion must leave a measurable scene COM offset"
    );
    (fleet, id)
}

/// Author the target from the frame tree's actual node state, without repeating gate COM algebra.
fn target_at_actual_port(
    fleet: &mut Fleet,
    source: &str,
    gap: f64,
    relative_velocity: DVec3,
) -> String {
    let part = format!("{source}/p1");
    let top = node(fleet.parts().part(&part).definition, "top").unwrap();
    let transform = fleet
        .frames()
        .transform(fleet.part_frame(&part), fleet.origin_frame());
    let port = transform.apply_state(void_frames::State {
        position: top.position,
        velocity: DVec3::ZERO,
    });
    let rotation = transform.rotation() * DQuat::from_rotation_x(std::f64::consts::PI);
    let direction = transform.apply_direction(top.direction);
    fleet.launch(
        &rendezvous_pod(),
        void_frames::State {
            position: port.position + direction * gap - rotation * DVec3::Y,
            velocity: port.velocity + relative_velocity,
        },
        rotation,
        DVec3::ZERO,
    )
}

fn actual_ports_in_scene(fleet: &Fleet, source: &str, target: &str) -> (DVec3, DVec3) {
    let scene = fleet
        .snapshot(source)
        .scene
        .expect("source must retain its depleted scene owner");
    let reference = fleet.scene_frames(scene).1;
    let state = |id: &str| {
        let part = format!("{id}/p1");
        let node = node(fleet.parts().part(&part).definition, "top").unwrap();
        fleet
            .frames()
            .transform(fleet.part_frame(&part), reference)
            .apply_state(void_frames::State {
                position: node.position,
                velocity: DVec3::ZERO,
            })
    };
    let a = state(source);
    let b = state(target);
    (b.position - a.position, b.velocity - a.velocity)
}

#[test]
fn depleted_scene_com_capture_distance_matches_frame_tree_ports() {
    for (gap, accepted) in [(0.199, true), (0.201, false)] {
        let (mut fleet, source) = depleted_asymmetric_scene();
        let target = target_at_actual_port(&mut fleet, &source, gap, DVec3::ZERO);
        let (delta, velocity) = actual_ports_in_scene(&fleet, &source, &target);
        assert!(
            (delta.length() - gap).abs() < 5e-5,
            "frame-tree gap {:?}",
            delta
        );
        assert!(
            velocity.length() < 1e-7,
            "frame-tree stationary tips {:?}",
            velocity
        );
        let result = fleet.dock(
            &format!("{source}/p1"),
            "dock",
            &format!("{target}/p1"),
            "dock",
        );
        if accepted {
            assert!(result.is_ok(), "actual gap {gap}: {result:?}");
        } else {
            assert!(result.unwrap_err().contains("distance"));
        }
    }
}

#[test]
fn depleted_scene_com_rotating_tip_speed_matches_frame_tree_ports() {
    for accepted in [true, false] {
        let (mut fleet, source) = depleted_asymmetric_scene();
        let snapshot = fleet.snapshot(&source);
        assert!(snapshot.angular_velocity.length() < 0.1);
        // This is the observable velocity error caused specifically by using parts origin as COM.
        let missed_velocity = snapshot
            .angular_velocity
            .cross(snapshot.rotation * fleet.centre_of_mass_local(&source));
        assert!(
            missed_velocity.length() > 1e-4,
            "test must exercise omega cross COM offset"
        );
        let margin = missed_velocity.length() / 4.0;
        let requested_velocity = if accepted {
            -missed_velocity.normalize() * (0.4 - margin)
        } else {
            missed_velocity.normalize() * (0.4 + margin)
        };
        let target = target_at_actual_port(&mut fleet, &source, 0.1, requested_velocity);
        let (delta, velocity) = actual_ports_in_scene(&fleet, &source, &target);
        assert!((delta.length() - 0.1).abs() < 5e-5);
        assert!((velocity.length() - requested_velocity.length()).abs() < 1e-7);
        assert_eq!(velocity.length() < 0.4, accepted);
        let result = fleet.dock(
            &format!("{source}/p1"),
            "dock",
            &format!("{target}/p1"),
            "dock",
        );
        if accepted {
            assert!(
                result.is_ok(),
                "actual tip speed {}: {result:?}",
                velocity.length()
            );
        } else {
            assert!(result.unwrap_err().contains("speed"));
        }
    }
}

//! Docking ports: which own and target port the pilot has chosen, and the dock/undock keys.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PortAddress {
    pub vessel: String,
    pub part: String,
    pub module: String,
}

/// The pilot's chosen ports. Derived UI state, not checkpoint or journal data: revalidated after
/// keyboard actions, replay and accepted steps before drawing.
#[derive(Resource, Default)]
pub(super) struct Docking {
    pub own: Option<PortAddress>,
    pub target: Option<PortAddress>,
}

pub(super) fn ports(session: &FlightSession, own: bool) -> Vec<PortAddress> {
    let f = &session.sim().fleet;
    let selected = &session.sim().selected;
    f.vessel_ids()
        .iter()
        .filter(|id| (*id == selected) == own)
        .flat_map(|id| {
            f.part_snapshots(id).into_iter().flat_map(move |p| {
                p.definition.modules.iter().filter_map(move |m| match m {
                    Module::DockingPort { id: module, .. } => Some(PortAddress {
                        vessel: id.clone(),
                        part: p.id.clone(),
                        module: module.clone(),
                    }),
                    _ => None,
                })
            })
        })
        .collect()
}
/// A port's mount point (from the vessel's centre of mass) and outward normal, in its parts frame.
pub(super) fn port_pose(session: &FlightSession, port: &PortAddress) -> (DVec3, DVec3) {
    let f = &session.sim().fleet;
    let p = f.parts().part(&port.part);
    let Module::DockingPort { node_id, .. } = p
        .definition
        .modules
        .iter()
        .find(|m| m.id() == port.module)
        .expect("port module")
    else {
        panic!("not a port")
    };
    let n = p
        .definition
        .nodes
        .iter()
        .find(|n| n.id == *node_id)
        .expect("port node");
    (
        p.pose.position + p.pose.rotation * n.position - f.centre_of_mass_local(&port.vessel),
        p.pose.rotation * n.direction,
    )
}
fn cycle_port(selected: &mut Option<PortAddress>, candidates: Vec<PortAddress>) {
    *selected = if candidates.is_empty() {
        None
    } else {
        let next = selected
            .as_ref()
            .and_then(|p| candidates.iter().position(|c| c == p))
            .map_or(0, |i| (i + 1) % candidates.len());
        Some(candidates[next].clone())
    };
}
pub(super) fn refresh_ports(docking: &mut Docking, session: &FlightSession) {
    let own = ports(session, true);
    let mut target = ports(session, false);
    let f = &session.sim().fleet;
    let selected = &session.sim().selected;
    target.sort_by(|a, b| {
        f.relative(&a.vessel, selected)
            .position
            .length_squared()
            .total_cmp(&f.relative(&b.vessel, selected).position.length_squared())
    });
    if docking.own.as_ref().is_none_or(|p| !own.contains(p)) {
        docking.own = own.first().cloned();
    }
    if docking.target.as_ref().is_none_or(|p| !target.contains(p)) {
        docking.target = target.first().cloned();
    }
}
pub(super) fn docking_controls(pilot: &mut Pilot, keys: &ButtonInput<KeyCode>) {
    refresh_ports(&mut pilot.docking, &pilot.flight.session);
    let own = ports(&pilot.flight.session, true);
    let target = ports(&pilot.flight.session, false);
    if keys.just_pressed(KeyCode::F10) {
        cycle_port(&mut pilot.docking.own, own);
    }
    if keys.just_pressed(KeyCode::F11) {
        cycle_port(&mut pilot.docking.target, target);
    }
    if keys.just_pressed(KeyCode::F12) {
        for p in [pilot.docking.own.clone(), pilot.docking.target.clone()]
            .into_iter()
            .flatten()
        {
            pilot.flight.session.execute(Action::ArmDock {
                part: p.part,
                module: p.module,
                armed: true,
            });
        }
        pilot.notice.0 = "Selected ports armed".into();
    }
    let action = if keys.just_pressed(KeyCode::Enter) {
        match (&pilot.docking.own, &pilot.docking.target) {
            (Some(a), Some(b)) => Some(Action::Dock {
                part_a: a.part.clone(),
                module_a: a.module.clone(),
                part_b: b.part.clone(),
                module_b: b.module.clone(),
            }),
            _ => {
                pilot.notice.0 = "Dock refused: select own and target ports".into();
                None
            }
        }
    } else if keys.just_pressed(KeyCode::Backspace) {
        pilot.docking.own.as_ref().map(|p| Action::Undock {
            part: p.part.clone(),
            module: p.module.clone(),
        })
    } else {
        None
    };
    if let Some(action) = action {
        neutral_pilot(&mut pilot.flight.session);
        match pilot.flight.session.execute(action) {
            Outcome::Spawned(_) => {
                let id = pilot.flight.session.sim().selected.clone();
                select_pilot(pilot, &id);
                pilot.visuals.rebuild = true;
                pilot.forecast.coast = None;
                pilot.notice.0 = "Docking topology updated".into();
            }
            Outcome::Refused(reason) => pilot.notice.0 = format!("Docking refused: {reason}"),
            other => panic!("unexpected docking outcome {other:?}"),
        }
    }
}

pub(super) fn docking_description(session: &FlightSession, docking: &Docking) -> String {
    let f = &session.sim().fleet;
    let id = &session.sim().selected;
    let rcs = f.rcs_control(id);
    let allocation = f.rcs_allocation(id);
    let mono: f64 = f
        .part_snapshots(id)
        .iter()
        .map(|p| {
            p.resources
                .get(&void_assembly::ResourceId::Monopropellant)
                .copied()
                .unwrap_or(0.0)
        })
        .sum();
    let label = |port: &Option<PortAddress>| {
        port.as_ref().map_or("none".into(), |p| {
            format!(
                "{}/{} {:?}",
                p.part,
                p.module,
                f.parts().part(&p.part).modules[&p.module]
            )
        })
    };
    let mut text = format!(
        "RCS {} mono {:.3}kg | delivered F {:.1}N τ {:.1}Nm | residual {:.1}N/{:.1}Nm\nH RCS | manual RCS torque disengages SAS reaction wheel | Alt+W/S ±Z D/A ±X E/Q ±Y translate | WASD QE RCS torque\nF10 own {} | F11 target {} | F12 arm both | Enter dock | Backspace undock",
        if rcs.enabled { "ON" } else { "OFF" },
        mono,
        allocation.force.length(),
        allocation.torque.length(),
        allocation.force_residual.length(),
        allocation.torque_residual.length(),
        label(&docking.own),
        label(&docking.target)
    );
    if let (Some(a), Some(b)) = (&docking.own, &docking.target) {
        let sa = f.snapshot(&a.vessel);
        let sb = f.snapshot(&b.vessel);
        let (pa, na) = port_pose(session, a);
        let (pb, nb) = port_pose(session, b);
        let relative = f.relative(&b.vessel, &a.vessel);
        let ar = sa.rotation * pa;
        let br = sb.rotation * pb;
        text.push_str(&format!(
            "\nPorts {:.3}m | speed {:.3}m/s | angle {:.2}° | spin {:.3}rad/s",
            (relative.position + br - ar).length(),
            (relative.velocity + sb.angular_velocity.cross(br) - sa.angular_velocity.cross(ar))
                .length(),
            (-(sa.rotation * na).dot(sb.rotation * nb))
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees(),
            (sb.angular_velocity - sa.angular_velocity).length()
        ));
    }
    text
}

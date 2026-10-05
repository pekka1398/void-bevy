//! Deterministic bounded allocation of actual nozzles. Fixed 64 cyclic coordinate sweeps minimize
//! squared force/torque residual; torque is divided by the nozzle lever scale (metres).
//! No negative throttle, ideal steering or synthetic force. Remaining residual is observable.
use glam::DVec3;
use serde::{Deserialize, Serialize};
use void_assembly::{Module, ModuleState, PartGraph};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RcsControl {
    pub enabled: bool,
    /// Requested vessel-local force and torque in SI units. These are targets, not applied forces.
    pub force: DVec3,
    pub torque: DVec3,
}
#[derive(Clone, Debug)]
pub struct Nozzle {
    pub part: String,
    pub module: String,
    pub resource: void_assembly::ResourceId,
    pub point: DVec3,
    pub full_force: DVec3,
    pub isp_seconds: f64,
    pub throttle: f64,
}
#[derive(Clone, Debug)]
pub struct Allocation {
    pub nozzles: Vec<Nozzle>,
    pub force: DVec3,
    pub torque: DVec3,
    pub force_residual: DVec3,
    pub torque_residual: DVec3,
}
pub fn allocate(
    graph: &PartGraph,
    members: &[String],
    centre: DVec3,
    control: RcsControl,
) -> Allocation {
    assert!(
        centre.is_finite() && control.force.is_finite() && control.torque.is_finite(),
        "invalid RCS request"
    );
    let mut nozzles = vec![];
    for id in members {
        let p = graph.part(id);
        for m in &p.definition.modules {
            let Module::Rcs {
                id: module,
                resource,
                thrust_newtons,
                isp_seconds,
                direction,
                point,
            } = m
            else {
                continue;
            };
            let ModuleState::Rcs { enabled } = p.modules[module] else {
                panic!("invalid RCS state")
            };
            if p.thermally_failed()
                || !control.enabled
                || !enabled
                || !graph
                    .resource_tanks(members, id, *resource)
                    .iter()
                    .any(|t| graph.part(t).resource(*resource) > 0.0)
            {
                continue;
            }
            nozzles.push(Nozzle {
                part: id.clone(),
                module: module.clone(),
                resource: *resource,
                point: p.pose.position + p.pose.rotation * *point,
                full_force: p.pose.rotation * *direction * *thrust_newtons,
                isp_seconds: *isp_seconds,
                throttle: 0.0,
            });
        }
    }
    let lever = nozzles
        .iter()
        .map(|n| (n.point - centre).length())
        .fold(1.0, f64::max);
    let columns: Vec<_> = nozzles
        .iter()
        .map(|n| (n.full_force, (n.point - centre).cross(n.full_force) / lever))
        .collect();
    let (mut residual_f, mut residual_t) = (control.force, control.torque / lever);
    for _ in 0..64 {
        for (n, (f, t)) in nozzles.iter_mut().zip(&columns) {
            let delta = ((f.dot(residual_f) + t.dot(residual_t))
                / (f.length_squared() + t.length_squared())
                + n.throttle)
                .clamp(0.0, 1.0)
                - n.throttle;
            n.throttle += delta;
            residual_f -= *f * delta;
            residual_t -= *t * delta;
        }
    }
    let force = nozzles.iter().map(|n| n.full_force * n.throttle).sum();
    let torque = nozzles
        .iter()
        .map(|n| (n.point - centre).cross(n.full_force * n.throttle))
        .sum();
    Allocation {
        nozzles,
        force,
        torque,
        force_residual: control.force - force,
        torque_residual: control.torque - torque,
    }
}

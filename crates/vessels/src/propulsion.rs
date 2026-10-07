use glam::DVec3;
use void_assembly::{PartGraph, ResourceId};
use void_modules::{Conditions, engine};

#[derive(Clone, Debug)]
pub struct EngineForce {
    pub part_id: String,
    pub module_id: String,
    pub force: DVec3,
    pub point: DVec3,
}
#[derive(Clone, Debug)]
pub struct FuelGroup {
    pub tanks: Vec<String>,
    pub resource: ResourceId,
    pub engines: Vec<EngineForce>,
    pub flow_kg_per_second: f64,
    pub fuel_kg: f64,
}
#[derive(Clone, Debug)]
pub struct Propulsion {
    pub force: DVec3,
    pub torque: DVec3,
    pub flow_kg_per_second: f64,
    pub groups: Vec<FuelGroup>,
    pub seconds_to_flameout: f64,
}
/// Shared by orbital and contact owners: the lit engines among `members`, which are summed in
/// that order, each pushing as its engine module says in `conditions`. Tanks in one crossfeed
/// group drain proportionally.
pub fn propulsion(
    graph: &PartGraph,
    members: &[String],
    throttle: f64,
    centre: DVec3,
    conditions: &Conditions,
) -> Propulsion {
    assert!(
        (0.0..=1.0).contains(&throttle),
        "propulsion: throttle {throttle}"
    );
    let mut groups: Vec<FuelGroup> = vec![];
    if throttle > 0.0 {
        for id in members {
            let p = graph.part(id);
            for (module_id, rating) in p.engines() {
                if p.thermally_failed() || !p.engine_enabled(module_id) {
                    continue;
                }
                let resource = rating.resource;
                let tanks = graph.resource_tanks(members, id, resource);
                let fuel_kg = tanks
                    .iter()
                    .map(|t| graph.part(t).resource(resource))
                    .sum::<f64>();
                if fuel_kg == 0.0 {
                    continue;
                }
                let thrust = engine::thrust_rating(p, rating, throttle, conditions);
                if thrust.flow_kg_per_second == 0.0 {
                    continue;
                }
                let e = EngineForce {
                    part_id: id.clone(),
                    module_id: module_id.to_string(),
                    force: thrust.force,
                    point: thrust.point,
                };
                let flow = thrust.flow_kg_per_second;
                assert!(flow.is_finite() && flow > 0.0, "invalid consumer flow");
                assert!(
                    !groups.iter().any(|g| g.resource == resource
                        && g.tanks != tanks
                        && g.tanks.iter().any(|t| tanks.contains(t))),
                    "partially overlapping supply pools unsupported"
                );
                if let Some(g) = groups
                    .iter_mut()
                    .find(|g| g.resource == resource && g.tanks == tanks)
                {
                    g.engines.push(e);
                    g.flow_kg_per_second += flow;
                } else {
                    groups.push(FuelGroup {
                        resource,
                        tanks,
                        engines: vec![e],
                        flow_kg_per_second: flow,
                        fuel_kg,
                    });
                }
            }
        }
    }
    let mut out = Propulsion {
        force: DVec3::ZERO,
        torque: DVec3::ZERO,
        flow_kg_per_second: 0.0,
        seconds_to_flameout: f64::INFINITY,
        groups,
    };
    for g in &out.groups {
        for e in &g.engines {
            out.force += e.force;
            out.torque += (e.point - centre).cross(e.force);
        }
        out.flow_kg_per_second += g.flow_kg_per_second;
        out.seconds_to_flameout = out
            .seconds_to_flameout
            .min(g.fuel_kg / g.flow_kg_per_second);
    }
    out
}
pub fn burn(graph: &mut PartGraph, groups: &[FuelGroup], seconds: f64) -> f64 {
    assert!(
        seconds >= 0.0 && seconds.is_finite(),
        "burn: seconds {seconds}"
    );
    let mut total = 0.0;
    for g in groups {
        let used = g.fuel_kg.min(g.flow_kg_per_second * seconds);
        for id in &g.tanks {
            let fuel_kg = graph.part(id).resource(g.resource);
            let remaining = if used >= g.fuel_kg {
                0.0
            } else {
                fuel_kg * (1.0 - used / g.fuel_kg)
            };
            graph.set_resource(id, g.resource, remaining);
        }
        total += used;
    }
    total
}
pub fn step_thrust(p: &Propulsion, dt: f64, centre: DVec3) -> (DVec3, DVec3, f64) {
    assert!(dt > 0.0 && dt.is_finite(), "thrust step: dt {dt}");
    let (mut force, mut torque, mut burned) = (DVec3::ZERO, DVec3::ZERO, 0.0);
    for g in &p.groups {
        let fraction = (g.fuel_kg / (g.flow_kg_per_second * dt)).min(1.0);
        burned += g.flow_kg_per_second * dt * fraction;
        for e in &g.engines {
            let f = e.force * fraction;
            force += f;
            torque += (e.point - centre).cross(f);
        }
    }
    (force, torque, burned)
}

/// RCS uses the same typed crossfeed pools and fuel exhaustion semantics as engines.
pub fn rcs_propulsion(
    graph: &PartGraph,
    members: &[String],
    centre: DVec3,
    control: void_modules::rcs::RcsControl,
) -> Propulsion {
    let allocation = void_modules::rcs::allocate(graph, members, centre, control);
    let mut out = Propulsion {
        force: DVec3::ZERO,
        torque: DVec3::ZERO,
        flow_kg_per_second: 0.0,
        groups: vec![],
        seconds_to_flameout: f64::INFINITY,
    };
    for n in allocation.nozzles {
        if n.throttle == 0.0 {
            continue;
        }
        let tanks = graph.resource_tanks(members, &n.part, n.resource);
        let fuel_kg = tanks
            .iter()
            .map(|t| graph.part(t).resource(n.resource))
            .sum();
        let force = n.full_force * n.throttle;
        let flow = force.length() / (n.isp_seconds * void_assembly::G0);
        let engine = EngineForce {
            part_id: n.part,
            module_id: n.module,
            force,
            point: n.point,
        };
        if let Some(g) = out
            .groups
            .iter_mut()
            .find(|g| g.resource == n.resource && g.tanks == tanks)
        {
            g.engines.push(engine);
            g.flow_kg_per_second += flow;
        } else {
            out.groups.push(FuelGroup {
                tanks,
                resource: n.resource,
                engines: vec![engine],
                flow_kg_per_second: flow,
                fuel_kg,
            });
        }
    }
    for g in &out.groups {
        for e in &g.engines {
            out.force += e.force;
            out.torque += (e.point - centre).cross(e.force);
        }
        out.flow_kg_per_second += g.flow_kg_per_second;
        out.seconds_to_flameout = out
            .seconds_to_flameout
            .min(g.fuel_kg / g.flow_kg_per_second);
    }
    out
}
impl Propulsion {
    /// Merge consumers sharing exactly one supply pool before computing exhaustion/burn.
    pub fn combined(mut self, other: Self) -> Self {
        self.force += other.force;
        self.torque += other.torque;
        self.flow_kg_per_second += other.flow_kg_per_second;
        for g in other.groups {
            assert!(
                !self.groups.iter().any(|a| a.resource == g.resource
                    && a.tanks != g.tanks
                    && a.tanks.iter().any(|t| g.tanks.contains(t))),
                "partially overlapping supply pools unsupported"
            );
            if let Some(a) = self
                .groups
                .iter_mut()
                .find(|a| a.resource == g.resource && a.tanks == g.tanks)
            {
                a.engines.extend(g.engines);
                a.flow_kg_per_second += g.flow_kg_per_second;
            } else {
                self.groups.push(g);
            }
        }
        self.seconds_to_flameout = self
            .groups
            .iter()
            .map(|g| g.fuel_kg / g.flow_kg_per_second)
            .fold(f64::INFINITY, f64::min);
        self
    }
}
impl EngineForce {
    /// Shared module-force contract. A nozzle's point and force are in parts axes; placement into
    /// `frame` and the requested moment reference are explicit here.
    pub fn wrench_in(
        &self,
        frame: void_frames::FrameId,
        origin: DVec3,
        rotation: glam::DQuat,
        reference_point: DVec3,
    ) -> void_modules::Wrench {
        void_modules::Wrench::at_point(
            frame,
            reference_point,
            origin + rotation * self.point,
            rotation * self.force,
            DVec3::ZERO,
        )
    }
}

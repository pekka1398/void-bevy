use glam::DVec3;
use std::collections::{HashMap, HashSet};
use void_assembly::{
    Connection, CrossfeedPart, G0, Module, PartDefinition, PartPose, crossfeed_tanks,
};

#[derive(Clone, Debug)]
pub struct PropulsionPart {
    pub id: String,
    pub definition: &'static PartDefinition,
    pub fuel_kg: f64,
    pub stage: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct EngineForce {
    pub part_id: String,
    pub force: DVec3,
    pub point: DVec3,
}
#[derive(Clone, Debug)]
pub struct FuelGroup {
    pub tanks: Vec<String>,
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
/// Shared by orbital and contact owners. Tanks in one crossfeed group drain proportionally.
pub fn propulsion(
    parts: &[&PropulsionPart],
    poses: &[(String, PartPose)],
    connections: &[Connection],
    lit: &HashSet<String>,
    throttle: f64,
    centre: DVec3,
) -> Propulsion {
    assert!(
        (0.0..=1.0).contains(&throttle),
        "propulsion: throttle {throttle}"
    );
    let graph: Vec<_> = parts
        .iter()
        .map(|p| CrossfeedPart {
            id: &p.id,
            definition: p.definition,
        })
        .collect();
    let by_id: HashMap<_, _> = parts.iter().map(|p| (p.id.as_str(), *p)).collect();
    let mut groups: Vec<FuelGroup> = vec![];
    if throttle > 0.0 {
        for p in parts {
            if !lit.contains(&p.id) {
                continue;
            }
            let engine = p
                .definition
                .modules
                .iter()
                .find(|m| matches!(m, Module::Engine { .. }))
                .expect("lit part has no engine");
            let Module::Engine {
                thrust_newtons,
                isp_seconds,
                direction,
            } = engine
            else {
                unreachable!()
            };
            let tanks = crossfeed_tanks(&graph, connections, &p.id);
            let fuel_kg = tanks
                .iter()
                .map(|id| by_id[id.as_str()].fuel_kg)
                .sum::<f64>();
            if fuel_kg <= 0.0 {
                continue;
            }
            let pose = &poses
                .iter()
                .find(|(id, _)| id == &p.id)
                .expect("engine has no pose")
                .1;
            let thrust = thrust_newtons * throttle;
            let e = EngineForce {
                part_id: p.id.clone(),
                force: pose.rotation * *direction * thrust,
                point: pose.position,
            };
            let flow = thrust / (isp_seconds * G0);
            if let Some(g) = groups.iter_mut().find(|g| g.tanks == tanks) {
                g.engines.push(e);
                g.flow_kg_per_second += flow;
            } else {
                groups.push(FuelGroup {
                    tanks,
                    engines: vec![e],
                    flow_kg_per_second: flow,
                    fuel_kg,
                });
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
pub fn burn(
    parts: &mut HashMap<String, PropulsionPart>,
    groups: &[FuelGroup],
    seconds: f64,
) -> f64 {
    assert!(
        seconds >= 0.0 && seconds.is_finite(),
        "burn: seconds {seconds}"
    );
    let mut total = 0.0;
    for g in groups {
        let used = g.fuel_kg.min(g.flow_kg_per_second * seconds);
        for id in &g.tanks {
            let p = parts.get_mut(id).expect("unknown tank");
            p.fuel_kg = if used >= g.fuel_kg {
                0.0
            } else {
                (p.fuel_kg * (1.0 - used / g.fuel_kg)).max(0.0)
            };
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

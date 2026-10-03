//! Assembly geometry and pressure ratings adapted to Fleet's engine-free force interface.
use glam::{DMat3, DQuat, DVec3};
use std::{collections::HashMap, sync::Arc};
use void_aero::{
    AeroElement, AeroShape, AeroState, Atmosphere, BodyAero, EarthAtmosphere, NEUTRAL,
    aerodynamic_forces,
};
use void_assembly::{Connection, Module, Shape};
use void_frames::BodyId;
use void_orbit::{AirSource, CelestialBody, EphemerisSource, body_orientation};
use void_vessels::{EnvironmentPart, EnvironmentSample, FleetEnvironment, VesselSnapshot};

pub struct FleetAir {
    pub body_index: usize,
    pub atmosphere: Atmosphere,
    /// Authored nozzle areas by engine definition ID. Every engine must have an explicit rating.
    pub nozzle_areas: HashMap<String, f64>,
}
impl FleetAir {
    pub fn earth(body_index: usize, density_scale: f64) -> Self {
        Self {
            body_index,
            atmosphere: Atmosphere::Earth(EarthAtmosphere::new(density_scale)),
            nozzle_areas: [("engine-large".into(), 0.12), ("engine-small".into(), 0.15)].into(),
        }
    }
}
fn axes(body: &CelestialBody, t: f64) -> DQuat {
    let a = body_orientation(&body.rotation, t);
    DQuat::from_mat3(&DMat3::from_cols(a[0], a[1], a[2])).normalize()
}
struct CraftAir {
    atmosphere: Atmosphere,
    body: CelestialBody,
    epoch: f64,
    centre: DVec3,
    centre_velocity: DVec3,
    rotation: DQuat,
    elements: Vec<AeroElement>,
}
impl AirSource for CraftAir {
    fn acceleration(&self, t: f64, position: DVec3, velocity: DVec3, mass: f64) -> DVec3 {
        let q = axes(&self.body, t);
        let r = q.conjugate() * (position - self.centre - self.centre_velocity * (t - self.epoch));
        let altitude = r.length() - self.body.radius_meters;
        if altitude >= self.atmosphere.ceiling_meters() {
            return DVec3::ZERO;
        }
        let omega = self.body.rotation.rate();
        let v = q.conjugate() * (velocity - self.centre_velocity) - DVec3::Z.cross(r) * omega;
        let state = AeroState {
            center: r,
            velocity: v,
            rotation: (q.conjugate() * self.rotation).normalize(),
            // Match the current game's force-only scope; no aerodynamic torque or spin damping.
            angular_velocity: DVec3::ZERO,
        };
        let force = aerodynamic_forces(
            &self.elements,
            &state,
            &self.atmosphere.sample(altitude),
            DVec3::ZERO,
            &NEUTRAL,
        )
        .force;
        q * force / mass
    }
}
impl FleetEnvironment for FleetAir {
    fn sample(
        &self,
        ephemeris: &dyn EphemerisSource,
        time: f64,
        vessel: &VesselSnapshot,
        parts: &[EnvironmentPart],
        connections: &[Connection],
    ) -> EnvironmentSample {
        let body = ephemeris.bodies()[self.body_index].clone();
        let (centre, centre_velocity) = ephemeris.body_state(BodyId(self.body_index), time);
        let altitude = (vessel.position - centre).length() - body.radius_meters;
        let pressure = if altitude < self.atmosphere.ceiling_meters() {
            self.atmosphere.sample(altitude).pressure_pa
        } else {
            0.0
        };
        let mut thrust_scales = HashMap::new();
        let mut elements = Vec::new();
        for part in parts {
            let d = part.definition;
            for module in &d.modules {
                if let Module::Engine { thrust_newtons, .. } = module {
                    let area = *self
                        .nozzle_areas
                        .get(&d.id)
                        .expect("fleet air: engine has no nozzle rating");
                    assert!(
                        area.is_finite() && area >= 0.0,
                        "fleet air: invalid nozzle area"
                    );
                    thrust_scales.insert(
                        part.id.clone(),
                        (1.0 - area * pressure / thrust_newtons).max(0.0),
                    );
                }
            }
            let exposed = |node: &str| {
                let mut covered = 0.0_f64;
                for link in connections {
                    let other = if link.a == part.id && link.node_a == node {
                        Some(&link.b)
                    } else if link.b == part.id && link.node_b == node {
                        Some(&link.a)
                    } else {
                        None
                    };
                    if let Some(other) = other {
                        // Ignore edges outside this connected vessel, including separated stages.
                        if let Some(p) = parts.iter().find(|p| p.id == *other) {
                            covered = covered.max(p.definition.radius);
                        }
                    }
                }
                std::f64::consts::PI * (d.radius.powi(2) - covered.powi(2)).max(0.0)
            };
            elements.push(AeroElement {
                id: part.id.clone(),
                point: part.pose.position,
                shape: AeroShape::Body(BodyAero {
                    axis: part.pose.rotation * DVec3::Y,
                    front_area: exposed("top"),
                    rear_area: exposed("bottom"),
                    side_area: 2.0 * d.radius * d.height,
                    wet_area: std::f64::consts::TAU * d.radius * d.height,
                    length_meters: d.height,
                    front_cd: if d.shape == Shape::Cone { 0.25 } else { 0.6 },
                    rear_cd: 0.8,
                    side_cd: 1.1,
                }),
            });
        }
        EnvironmentSample {
            air: Some(Arc::new(CraftAir {
                atmosphere: self.atmosphere.clone(),
                body,
                epoch: time,
                centre,
                centre_velocity,
                rotation: vessel.rotation,
                elements,
            })),
            thrust_scales,
        }
    }
}

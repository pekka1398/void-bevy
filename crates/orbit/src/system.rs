use std::collections::HashSet;
use std::f64::consts::FRAC_PI_2;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use void_frames::Spin;

use crate::kepler::{
    EllipticElements, orbital_period_seconds, solve_kepler_elliptic, state_from_elements,
    true_anomaly,
};

/// CODATA 2018, m^3 kg^-1 s^-2.
pub const GRAVITATIONAL_CONSTANT: f64 = 6.6743e-11;

/// A system as the orbit lab's `SystemSpec` (`lab/orbit/src/orbit/SystemSpec.ts`), read from JSON.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SystemSpec {
    pub name: String,
    pub root: BodySpec,
}

impl SystemSpec {
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|e| panic!("system spec: {e}"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BodySpec {
    pub id: String,
    pub name: String,
    pub mass_kg: f64,
    pub radius_meters: f64,
    pub color: String,
    pub rotation: RotationSpec,
    /// Jacobi elements: this body's subtree barycentre orbits the barycentre of its parent plus
    /// every earlier sibling subtree, with mu = G (M_inner + M_this). Every body but the root.
    pub orbit: Option<EllipticElements>,
    /// Reference plane of the elements. Required with an orbit.
    pub orbit_plane: Option<OrbitPlane>,
    /// Zonal J2 about the spin axis, felt by vessels. Absent: a point mass.
    pub gravity_field: Option<GravityField>,
    pub children: Vec<BodySpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrbitPlane {
    Ecliptic,
    /// The parent body's equator: x at its equinox node, z along its spin axis.
    ParentEquator,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GravityField {
    pub j2: f64,
    pub reference_radius_meters: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RotationSpec {
    Locked(LockedRotationSpec),
    Spin(SpinSpec),
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpinSpec {
    pub period_seconds: f64,
    /// Angle between the spin axis and ecliptic north, radians in [0, pi].
    pub obliquity_radians: f64,
    pub pole_longitude_radians: f64,
    pub angle_at_epoch_radians: f64,
}

impl From<SpinSpec> for Spin {
    fn from(s: SpinSpec) -> Self {
        Spin {
            period_seconds: s.period_seconds,
            obliquity_radians: s.obliquity_radians,
            pole_longitude_radians: s.pole_longitude_radians,
            angle_at_epoch_radians: s.angle_at_epoch_radians,
        }
    }
}

/// Tidally locked rotation, resolved from the body's initial orbit about its parent body:
/// - period: given, the mean sidereal period of the perturbed orbit;
/// - spin axis: the orbit normal tilted by `obliquity_to_orbit` toward and past ecliptic north,
///   in the plane of both (Cassini state 2, like the Moon);
/// - prime meridian facing the parent's mean direction at t = 0 (true direction minus the
///   equation of centre).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedRotationSpec {
    pub kind: Locked,
    pub period_seconds: f64,
    /// Angle between spin axis and orbit normal, radians in [0, pi/2).
    pub obliquity_to_orbit_radians: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Locked {
    Locked,
}

#[derive(Clone, Debug)]
pub struct CelestialBody {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub mass_kg: f64,
    pub gm: f64,
    pub radius_meters: f64,
    pub color: String,
    pub rotation: Spin,
    /// 0 for a point mass.
    pub j2: f64,
    pub j2_reference_radius_meters: f64,
    pub parent_index: Option<usize>,
    /// Jacobi two-body period, for step selection and display. None for the root.
    pub orbit_period_seconds: Option<f64>,
    /// Jacobi periapsis over semi-major axis. None for the root.
    pub periapsis_fraction: Option<f64>,
    /// Laplace sphere of influence a (m / M_inner)^(2/5). None for the root.
    pub sphere_of_influence_meters: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct BuiltSystem {
    pub name: String,
    pub bodies: Vec<CelestialBody>,
    /// Barycentric; total momentum and barycentre at zero.
    pub positions: Vec<DVec3>,
    pub velocities: Vec<DVec3>,
}

fn assert_body_spec(spec: &BodySpec, is_root: bool) {
    let id = &spec.id;
    assert!(
        spec.mass_kg > 0.0 && spec.mass_kg.is_finite(),
        "{id}: mass {}",
        spec.mass_kg
    );
    assert!(
        spec.radius_meters > 0.0 && spec.radius_meters.is_finite(),
        "{id}: radius {}",
        spec.radius_meters
    );
    match spec.rotation {
        RotationSpec::Locked(rot) => {
            assert!(!is_root, "{id}: the root body cannot be tidally locked");
            assert!(
                rot.period_seconds > 0.0 && rot.period_seconds.is_finite(),
                "{id}: locked period {}",
                rot.period_seconds
            );
            assert!(
                (0.0..FRAC_PI_2).contains(&rot.obliquity_to_orbit_radians),
                "{id}: obliquity to orbit {}",
                rot.obliquity_to_orbit_radians
            );
        }
        RotationSpec::Spin(rot) => {
            assert!(
                rot.period_seconds > 0.0 && rot.period_seconds.is_finite(),
                "{id}: rotation period {}",
                rot.period_seconds
            );
            assert!(
                (0.0..=std::f64::consts::PI).contains(&rot.obliquity_radians),
                "{id}: obliquity {}",
                rot.obliquity_radians
            );
            assert!(
                rot.pole_longitude_radians.is_finite() && rot.angle_at_epoch_radians.is_finite(),
                "{id}: rotation angles"
            );
        }
    }
    if let Some(field) = spec.gravity_field {
        assert!(field.j2 > 0.0 && field.j2 < 0.1, "{id}: J2 {}", field.j2);
        assert!(
            field.reference_radius_meters > 0.0 && field.reference_radius_meters.is_finite(),
            "{id}: J2 radius {}",
            field.reference_radius_meters
        );
    }
    assert!(
        !(is_root && spec.orbit.is_some()),
        "{id}: the root body cannot have an orbit"
    );
    assert!(
        is_root || spec.orbit.is_some(),
        "{id}: a non-root body requires an orbit"
    );
    assert!(
        spec.orbit.is_some() == spec.orbit_plane.is_some(),
        "{id}: orbit and orbitPlane go together"
    );
    if let Some(orbit) = &spec.orbit {
        orbit.assert_valid(id);
    }
}

/// A state given in the parent's equatorial axes, in ecliptic axes.
fn to_ecliptic((position, velocity): (DVec3, DVec3), parent: &BodySpec) -> (DVec3, DVec3) {
    let RotationSpec::Spin(rot) = parent.rotation else {
        panic!(
            "{}: a tidally locked body cannot be an equatorial reference plane",
            parent.id
        );
    };
    let [x, y, z] = Spin::from(rot).equatorial_basis();
    let map = |v: DVec3| x * v.x + y * v.y + z * v.z;
    (map(position), map(velocity))
}

/// r, v: the locked body relative to its parent body at t = 0.
fn locked_rotation(
    spec: &LockedRotationSpec,
    orbit: &EllipticElements,
    r: DVec3,
    v: DVec3,
) -> Spin {
    let normal = r.cross(v).normalize();
    let ob = spec.obliquity_to_orbit_radians;
    let mut axis = normal;
    if ob > 0.0 {
        let toward_north = DVec3::Z - normal * DVec3::Z.dot(normal);
        assert!(
            toward_north.length() > 1e-12,
            "locked rotation: tilt direction undefined for an orbit in the ecliptic"
        );
        axis = normal * ob.cos() + toward_north.normalize() * ob.sin();
    }
    let obliquity = axis.z.clamp(-1.0, 1.0).acos();
    let lon = axis.y.atan2(axis.x);
    // Same equatorial axes as Spin::equatorial_basis.
    let node = DVec3::new(-lon.sin(), lon.cos(), 0.0);
    let quadrature = axis.cross(node);
    let to_parent = -r;
    let mean = orbit.mean_anomaly_radians;
    let nu = true_anomaly(
        solve_kepler_elliptic(mean, orbit.eccentricity),
        orbit.eccentricity,
    );
    let centre = (nu - mean).sin().atan2((nu - mean).cos());
    Spin {
        period_seconds: spec.period_seconds,
        obliquity_radians: obliquity,
        pole_longitude_radians: lon,
        angle_at_epoch_radians: to_parent.dot(quadrature).atan2(to_parent.dot(node)) - centre,
    }
}

struct Placed {
    index: usize,
    position: DVec3,
    velocity: DVec3,
}

fn subtree_mass(node: &BodySpec) -> f64 {
    node.children
        .iter()
        .fold(node.mass_kg, |sum, child| sum + subtree_mass(child))
}

/// Bodies as the orbit lab's `buildSystem`: Jacobi elements placed subtree by subtree, then
/// shifted so the barycentre is at rest at the origin.
pub fn build_system(spec: &SystemSpec) -> BuiltSystem {
    struct Builder {
        bodies: Vec<CelestialBody>,
        ids: HashSet<String>,
    }

    impl Builder {
        /// Every body of the subtree, relative to the subtree barycentre.
        fn place(&mut self, node: &BodySpec, parent_index: Option<usize>) -> Vec<Placed> {
            assert_body_spec(node, parent_index.is_none());
            assert!(
                self.ids.insert(node.id.clone()),
                "duplicate body id {}",
                node.id
            );
            let index = self.bodies.len();
            self.bodies.push(CelestialBody {
                index,
                id: node.id.clone(),
                name: node.name.clone(),
                mass_kg: node.mass_kg,
                gm: GRAVITATIONAL_CONSTANT * node.mass_kg,
                radius_meters: node.radius_meters,
                color: node.color.clone(),
                // A locked rotation is resolved below, once the orbit is placed.
                rotation: match node.rotation {
                    RotationSpec::Spin(s) => s.into(),
                    RotationSpec::Locked(_) => Spin {
                        period_seconds: f64::NAN,
                        obliquity_radians: f64::NAN,
                        pole_longitude_radians: f64::NAN,
                        angle_at_epoch_radians: f64::NAN,
                    },
                },
                j2: node.gravity_field.map_or(0.0, |f| f.j2),
                j2_reference_radius_meters: node
                    .gravity_field
                    .map_or(0.0, |f| f.reference_radius_meters),
                parent_index,
                orbit_period_seconds: None,
                periapsis_fraction: None,
                sphere_of_influence_meters: None,
            });

            let mut placed = vec![Placed {
                index,
                position: DVec3::ZERO,
                velocity: DVec3::ZERO,
            }];
            let mut inner_mass = node.mass_kg;
            let mut inner_position = DVec3::ZERO;
            let mut inner_velocity = DVec3::ZERO;

            for child in &node.children {
                let child_placed = self.place(child, Some(index));
                let child_mass = subtree_mass(child);
                let orbit = child
                    .orbit
                    .as_ref()
                    .unwrap_or_else(|| panic!("{}: missing orbit", child.id));
                let gm = GRAVITATIONAL_CONSTANT * (inner_mass + child_mass);
                let relative = state_from_elements(orbit, gm);
                let (rel_position, rel_velocity) = match child.orbit_plane {
                    Some(OrbitPlane::ParentEquator) => to_ecliptic(relative, node),
                    Some(OrbitPlane::Ecliptic) => relative,
                    None => panic!("{}: orbit without orbitPlane", child.id),
                };
                let child_position = inner_position + rel_position;
                let child_velocity = inner_velocity + rel_velocity;
                let first = &child_placed[0];
                let body = &mut self.bodies[first.index];
                body.orbit_period_seconds =
                    Some(orbital_period_seconds(orbit.semi_major_axis_meters, gm));
                body.periapsis_fraction = Some(1.0 - orbit.eccentricity);
                body.sphere_of_influence_meters =
                    Some(orbit.semi_major_axis_meters * (child_mass / inner_mass).powf(0.4));
                if let RotationSpec::Locked(locked) = &child.rotation {
                    // This node sits at the origin of the frame the child is placed in.
                    body.rotation = locked_rotation(
                        locked,
                        orbit,
                        child_position + first.position,
                        child_velocity + first.velocity,
                    );
                }
                placed.extend(child_placed.iter().map(|p| Placed {
                    index: p.index,
                    position: child_position + p.position,
                    velocity: child_velocity + p.velocity,
                }));
                let total = inner_mass + child_mass;
                inner_position =
                    (inner_position * inner_mass + child_position * child_mass) * (1.0 / total);
                inner_velocity =
                    (inner_velocity * inner_mass + child_velocity * child_mass) * (1.0 / total);
                inner_mass = total;
            }

            for p in &mut placed {
                p.position -= inner_position;
                p.velocity -= inner_velocity;
            }
            placed
        }
    }

    let mut builder = Builder {
        bodies: Vec::new(),
        ids: HashSet::new(),
    };
    let placed = builder.place(&spec.root, None);
    let mut positions = vec![DVec3::NAN; builder.bodies.len()];
    let mut velocities = vec![DVec3::NAN; builder.bodies.len()];
    for p in placed {
        positions[p.index] = p.position;
        velocities[p.index] = p.velocity;
    }
    for body in &builder.bodies {
        body.rotation.assert_valid();
    }
    BuiltSystem {
        name: spec.name.clone(),
        bodies: builder.bodies,
        positions,
        velocities,
    }
}

/// Body-fixed axes at time t in the ecliptic, as the orbit lab's `bodyOrientation`
/// (`BodyRotation.ts`): z the spin axis, x the prime meridian. `Spin::body_axes` is the one
/// formula; it removes whole turns exactly first, so it differs from the lab's
/// `angle_at_epoch + 2π t / period` in the last digits at large t.
pub fn body_orientation(spin: &Spin, t: f64) -> [DVec3; 3] {
    spin.body_axes(t)
}

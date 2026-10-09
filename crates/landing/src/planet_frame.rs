//! The planet's rotating, body-fixed frame as a frame to do physics in.

use glam::DVec3;
use void_frames::{FrameId, State};
use void_orbit::{CelestialBody, EphemerisSource, SystemFrames, body_orientation, gravity};

/// A frame to do contact physics in: `ContactWorld` kicks every body by this acceleration (gravity
/// and the frame's own terms); Rapier's gravity is off. `PlanetFrame` (body-fixed, rotating) is
/// one; a free-falling frame is another.
pub trait ContactFrame {
    /// The body whose terrain is stationary in this frame, with the origin at its centre and
    /// body-fixed axes. Frames that cannot host collision terrain must return None.
    fn terrain_body(&self) -> Option<&CelestialBody>;
    /// Acceleration of a free particle at frame position r and velocity v at time t, contacts
    /// excluded. The ephemeris covers t.
    fn acceleration(&self, ephemeris: &dyn EphemerisSource, t: f64, r: DVec3, v: DVec3) -> DVec3;
    /// The frame's own angular velocity in its axes, rad/s (zero for a frame that does not turn).
    fn spin(&self) -> DVec3;
}

/// The planet's rotating, body-fixed frame (z the spin axis, x the prime meridian, origin at the
/// planet's centre). The ground is at rest here; a moving object feels, besides gravity, the
/// fictitious accelerations of the rotation (constant spin, so no Euler term):
/// centrifugal −ω × (ω × r) and Coriolis −2 ω × v; and, because the origin falls freely with the
/// planet, only the tidal part of other bodies' gravity.
#[derive(Clone, Debug)]
pub struct PlanetFrame {
    pub body: CelestialBody,
    /// Spin rate about body-fixed +z, rad/s.
    pub omega: f64,
    /// The ephemeris' frame tree; this frame is the body's surface frame in it.
    frames: SystemFrames,
}

/// Body axes to ecliptic: a.x X + a.y Y + a.z Z with the axes as columns.
fn to_ecliptic(axes: &[DVec3; 3], a: DVec3) -> DVec3 {
    DVec3::new(
        a.x * axes[0].x + a.y * axes[1].x + a.z * axes[2].x,
        a.x * axes[0].y + a.y * axes[1].y + a.z * axes[2].y,
        a.x * axes[0].z + a.y * axes[1].z + a.z * axes[2].z,
    )
}

impl PlanetFrame {
    pub fn new(ephemeris: &dyn EphemerisSource, body_index: usize) -> Self {
        let body = ephemeris
            .bodies()
            .get(body_index)
            .unwrap_or_else(|| panic!("planet frame: body {body_index}"))
            .clone();
        let omega = 2.0 * std::f64::consts::PI / body.rotation.period_seconds;
        Self {
            body,
            omega,
            frames: SystemFrames::new(ephemeris),
        }
    }

    fn surface(&self) -> FrameId {
        self.frames.surface[self.body.index]
    }

    /// The ephemeris' physics-view state (its origin system) to body-fixed, through the tree.
    pub fn to_body_fixed(&self, ephemeris: &dyn EphemerisSource, t: f64, inertial: State) -> State {
        self.frames
            .tree
            .at(t, ephemeris)
            .transform(self.frames.origin, self.surface())
            .apply_state(inertial)
    }

    /// Body-fixed state to the ephemeris' physics view, through the tree.
    pub fn to_inertial(&self, ephemeris: &dyn EphemerisSource, t: f64, local: State) -> State {
        self.frames
            .tree
            .at(t, ephemeris)
            .transform(self.surface(), self.frames.origin)
            .apply_state(local)
    }
}

impl ContactFrame for PlanetFrame {
    fn terrain_body(&self) -> Option<&CelestialBody> {
        Some(&self.body)
    }

    fn acceleration(&self, ephemeris: &dyn EphemerisSource, t: f64, r: DVec3, v: DVec3) -> DVec3 {
        let b = &self.body;
        // Own gravity, in body-fixed axes where the spin axis is +z.
        let mut a = gravity::pull(b.gm, gravity::oblateness(b), DVec3::Z, r);
        // Other bodies: their pull here minus their pull on the planet's centre.
        let bodies = ephemeris.bodies();
        if bodies.len() > 1 {
            let axes = body_orientation(&b.rotation, t);
            let mut positions = vec![DVec3::ZERO; bodies.len()];
            ephemeris.positions_at(t, &mut positions);
            let c = positions[b.index];
            // The particle in inertial axes, relative to the planet's centre.
            let p = to_ecliptic(&axes, r);
            let mut tide = DVec3::ZERO;
            for (k, o) in bodies.iter().enumerate() {
                if k == b.index {
                    continue;
                }
                let centre = c - positions[k];
                tide += gravity::body_pull(o, centre + p) - gravity::body_pull(o, centre);
            }
            a += DVec3::new(axes[0].dot(tide), axes[1].dot(tide), axes[2].dot(tide));
        }
        let (mut ax, mut ay, az) = (a.x, a.y, a.z);
        // Centrifugal ω² (x, y, 0) and Coriolis −2 ω × v with ω = (0, 0, ω).
        let w = self.omega;
        ax += w * w * r.x + 2.0 * w * v.y;
        ay += w * w * r.y - 2.0 * w * v.x;
        DVec3::new(ax, ay, az)
    }

    fn spin(&self) -> DVec3 {
        DVec3::new(0.0, 0.0, self.omega)
    }
}

/// Rotation taking local +y to the outward vertical at a body-fixed point.
pub fn upright_at(p: DVec3) -> glam::DQuat {
    let u = p.normalize();
    // Shortest arc from (0, 1, 0) to u.
    let w = 1.0 + u.y;
    if w < 1e-12 {
        return glam::DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0);
    }
    glam::DQuat::from_xyzw(u.z, 0.0, -u.x, w).normalize()
}

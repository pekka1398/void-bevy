use glam::DVec3;

use crate::ephemeris::EphemerisSource;
use crate::system::CelestialBody;
use crate::trajectory::Trajectory;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApsisKind {
    Periapsis,
    Apoapsis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Apsis {
    pub kind: ApsisKind,
    pub time: f64,
    /// Barycentric vessel position at the apsis.
    pub position: DVec3,
    /// Distance from the reference body's centre.
    pub distance_meters: f64,
}

const TIME_RESOLUTION_SECONDS: f64 = 1e-3;

/// Actual (not osculating) apsides of a trajectory relative to one body, as
/// `lab/orbit/src/orbit/Apsides.ts`: zeros of the radial velocity, bracketed at step points and
/// refined by bisection on the trajectory's Hermite interpolation.
pub fn find_apsides(
    trajectory: &Trajectory,
    ephemeris: &dyn EphemerisSource,
    body: usize,
    from_time: f64,
    max_count: usize,
) -> Vec<Apsis> {
    let mut found = Vec::new();
    if trajectory.count() < 2 {
        return found;
    }
    let n = ephemeris.bodies().len();
    let (mut positions, mut velocities) = (vec![DVec3::ZERO; n], vec![DVec3::ZERO; n]);
    // Radial rate and distance relative to the body.
    let mut radial = |t: f64, p: DVec3, v: DVec3| -> (f64, f64) {
        ephemeris.states_at(t, &mut positions, Some(&mut velocities));
        let r = p - positions[body];
        (r.dot(v - velocities[body]), r.length())
    };
    let last = trajectory.count() - 1;
    let mut i = 0;
    while i < last && trajectory.time(i + 1) <= from_time {
        i += 1;
    }
    let mut previous = radial(
        trajectory.time(i),
        trajectory.position(i),
        trajectory.velocity(i),
    )
    .0;
    while i < last && found.len() < max_count {
        let next = radial(
            trajectory.time(i + 1),
            trajectory.position(i + 1),
            trajectory.velocity(i + 1),
        )
        .0;
        if previous != 0.0 && sign(previous) != sign(next) {
            let kind = if previous < 0.0 {
                ApsisKind::Periapsis
            } else {
                ApsisKind::Apoapsis
            };
            let (mut lo, mut hi) = (trajectory.time(i), trajectory.time(i + 1));
            let lo_sign = sign(previous);
            while hi - lo > TIME_RESOLUTION_SECONDS {
                let mid = 0.5 * (lo + hi);
                let (p, v) = trajectory.sample(mid);
                if sign(radial(mid, p, v).0) == lo_sign {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let t = 0.5 * (lo + hi);
            let (p, v) = trajectory.sample(t);
            if t >= from_time {
                found.push(Apsis {
                    kind,
                    time: t,
                    position: p,
                    distance_meters: radial(t, p, v).1,
                });
            }
        }
        previous = next;
        i += 1;
    }
    found
}

/// `Math.sign`: −1, 0 or 1 (NaN never reaches here; the ephemeris and trajectory panic first).
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// The body whose Laplace sphere of influence most deeply contains a point, walking down the body
/// tree from the root, as `lab/orbit/src/orbit/Dominance.ts`. Only a choice of reference for
/// osculating elements and display; the dynamics are always full N-body.
pub struct DominanceTree {
    soi: Vec<Option<f64>>,
    children: Vec<Vec<usize>>,
    root: usize,
}

impl DominanceTree {
    pub fn new(bodies: &[CelestialBody]) -> Self {
        let roots: Vec<usize> = bodies
            .iter()
            .filter(|b| b.parent_index.is_none())
            .map(|b| b.index)
            .collect();
        assert!(
            roots.len() == 1,
            "dominance tree: expected one root, found {}",
            roots.len()
        );
        let mut children = vec![Vec::new(); bodies.len()];
        for b in bodies {
            if let Some(parent) = b.parent_index {
                children[parent].push(b.index);
            }
        }
        Self {
            soi: bodies
                .iter()
                .map(|b| b.sphere_of_influence_meters)
                .collect(),
            children,
            root: roots[0],
        }
    }

    /// `positions`: barycentric body positions at the same instant as `point`.
    pub fn dominant(&self, positions: &[DVec3], point: DVec3) -> usize {
        let mut current = self.root;
        loop {
            let mut best = None;
            let mut best_ratio = 1.0;
            for &child in &self.children[current] {
                let soi = self.soi[child].unwrap_or_else(|| {
                    panic!("dominance tree: body {child} has no sphere of influence")
                });
                let ratio = (point - positions[child]).length() / soi;
                if ratio < best_ratio {
                    best_ratio = ratio;
                    best = Some(child);
                }
            }
            match best {
                Some(child) => current = child,
                None => return current,
            }
        }
    }
}

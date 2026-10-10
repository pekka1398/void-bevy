//! Numerical reference-plane crossings of a trajectory, in its time-dependent plotting frame.
use crate::{EphemerisSource, FrameEvaluator, FrameSpec, Trajectory};
use glam::DVec3;
use void_frames::{State, SystemId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitNode {
    pub kind: NodeKind,
    pub time: f64,
    /// Position in the ephemeris physics view.
    pub position: DVec3,
    pub in_frame: DVec3,
    /// Signed derivative of frame z, including translation and rotation.
    pub normal_speed_mps: f64,
    /// Apparent angle of r cross v to the frame normal. Undefined at a degenerate centre.
    pub apparent_inclination_radians: Option<f64>,
}

/// Bracket crossings on the numerical trajectory and refine its Hermite interpolant. Each
/// integration interval is subdivided to catch crossings at sample boundaries and curved spans.
/// Coplanar spans and tangencies are excluded by a sign bracket and normal-speed threshold.
pub fn find_nodes(
    trajectory: &Trajectory,
    ephemeris: &dyn EphemerisSource,
    spec: FrameSpec,
    system: SystemId,
    from_time: f64,
    max_count: usize,
) -> Vec<OrbitNode> {
    assert!(from_time.is_finite(), "node start time not finite");
    let mut result = Vec::new();
    if trajectory.count() < 2 || max_count == 0 {
        return result;
    }
    let evaluator = FrameEvaluator::new_in_system(ephemeris, spec, system);
    let state = |t: f64| {
        let (position, velocity) = trajectory.sample(t);
        evaluator.state_at(ephemeris, t, State { position, velocity })
    };
    // Spatial tolerance is relative to distance from the plotting centre, with a micrometre floor.
    let side = |s: State| {
        let eps = 1e-6_f64.max(s.position.length() * 1e-12);
        if s.position.z > eps {
            1
        } else if s.position.z < -eps {
            -1
        } else {
            0
        }
    };
    let mut previous: Option<(f64, i32)> = None;
    for i in 0..trajectory.count() - 1 {
        let start = trajectory.time(i).max(from_time);
        let end = trajectory.time(i + 1);
        if end < start {
            continue;
        }
        for j in 0..=8 {
            let t = start + (end - start) * j as f64 / 8.0;
            let s = state(t);
            let sign = side(s);
            if sign == 0 {
                continue;
            }
            if let Some((last, last_sign)) = previous
                && sign != last_sign
            {
                let (mut lo, mut hi) = (last, t);
                for _ in 0..64 {
                    if hi - lo <= 1e-6 {
                        break;
                    }
                    let mid = (lo + hi) * 0.5;
                    if state(mid).position.z.signum() == last_sign as f64 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let time = (lo + hi) * 0.5;
                let at = state(time);
                let rate = at.velocity.z;
                if rate.abs() > 1e-8 && rate.signum() == sign as f64 {
                    let h = at.position.cross(at.velocity);
                    let inclination =
                        if matches!(spec, FrameSpec::Barycentric) || h.length() <= 1e-12 {
                            None
                        } else {
                            Some((h.z / h.length()).clamp(-1.0, 1.0).acos())
                        };
                    result.push(OrbitNode {
                        kind: if sign > 0 {
                            NodeKind::Ascending
                        } else {
                            NodeKind::Descending
                        },
                        time,
                        position: trajectory.sample(time).0,
                        in_frame: at.position,
                        normal_speed_mps: rate,
                        apparent_inclination_radians: inclination,
                    });
                    if result.len() == max_count {
                        return result;
                    }
                }
            }
            previous = Some((t, sign));
        }
    }
    result
}

//! Coast prediction and the encounter range gate, as `lab/landing/src/vessel/CoastPrediction.ts`
//! and `EncounterPhysics.ts`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use glam::DVec3;
use void_math::hypot;
use void_orbit::{
    AdvanceOutcome, EphemerisSource, PropagationRun, Tolerances, Trajectory, VesselPropagator,
    VesselState,
};
use void_terrain::Terrain;

use crate::planet_frame::{FrameState, PlanetFrame};

#[derive(Clone, Debug)]
pub struct CoastPrediction {
    /// Body-fixed positions with their times, ending at the terrain crossing when there is one.
    pub points: Vec<(f64, DVec3)>,
    pub impact: Option<(f64, DVec3)>,
    /// The same coast as barycentric inertial integrator samples (for map views and apsides). It
    /// ends at the first step past the terrain crossing, at most one sampling interval (about a
    /// second near the ground) beyond it.
    pub trajectory: Trajectory,
}

fn clearance(position: DVec3, terrain: &Terrain) -> f64 {
    let p = position;
    let r = hypot([p.x, p.y, p.z]);
    r - terrain.radius_meters - terrain.height(DVec3::new(p.x / r, p.y / r, p.z / r))
}

/// Coast from the current state through orbit physics, stopping at the sampled terrain.
#[allow(clippy::too_many_arguments)]
pub fn predict_coast(
    ephemeris: &mut dyn EphemerisSource,
    frame: &PlanetFrame,
    terrain: &Terrain,
    tolerances: Tolerances,
    time: f64,
    state: FrameState,
    mass_kg: f64,
    horizon_seconds: f64,
) -> CoastPrediction {
    let mut result = CoastPrediction {
        points: vec![(time, state.position)],
        impact: None,
        trajectory: Trajectory::new(),
    };
    let inertial = frame.to_inertial(ephemeris, time, state);
    let mut run = PropagationRun::new(VesselState {
        time,
        position: inertial.position,
        velocity: inertial.velocity,
        mass_kg,
    });
    result.trajectory.append(time, &run.y);
    if clearance(state.position, terrain) <= 0.0 {
        return result;
    }
    let mut propagator = VesselPropagator::new(ephemeris, tolerances);
    let (mut previous, mut previous_time) = (state, time);
    let end = time + horizon_seconds;
    while run.time < end - 1e-8 {
        let v = previous.velocity;
        let h = (clearance(previous.position, terrain) / hypot([v.x, v.y, v.z]).max(1.0))
            .clamp(1.0, 15.0);
        let target = end.min(run.time + h);
        let outcome = propagator.advance(
            ephemeris,
            &mut run,
            target,
            10_000,
            Some(&mut result.trajectory),
            None,
        );
        assert!(
            outcome != AdvanceOutcome::Budget,
            "coast prediction step budget exhausted"
        );
        let s = run.state();
        let now = frame.to_body_fixed(
            ephemeris,
            run.time,
            FrameState {
                position: s.position,
                velocity: s.velocity,
            },
        );
        if clearance(now.position, terrain) <= 0.0
            || matches!(outcome, AdvanceOutcome::Impact { .. })
        {
            // Interpolate the two body-fixed states to the terrain crossing; near the ground the
            // sampling interval is at most one second.
            let at = |f: f64| previous.position + (now.position - previous.position) * f;
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..20 {
                let f = (lo + hi) / 2.0;
                if clearance(at(f), terrain) > 0.0 {
                    lo = f;
                } else {
                    hi = f;
                }
            }
            let f = (lo + hi) / 2.0;
            let impact = (previous_time + f * (run.time - previous_time), at(f));
            result.impact = Some(impact);
            result.points.push(impact);
            break;
        }
        result.points.push((run.time, now.position));
        previous = now;
        previous_time = run.time;
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncounterRanges {
    /// Enter detailed, mutually collidable physics inside this distance.
    pub unpack_meters: f64,
    /// Return to independent orbital propagation only beyond this distance.
    pub pack_meters: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EncounterPairState {
    pub first: String,
    pub second: String,
    pub distance_meters: f64,
    pub physics: bool,
    pub changed: bool,
    pub closest_approach_meters: f64,
    pub time_to_closest_approach_seconds: f64,
}

/// Hysteresis gate for an orbital encounter. Callers pass both vessel states in one frame and the
/// length of the next propagation interval; predicting the closest approach within it avoids
/// stepping over the unpack sphere. A true pair means both vessels must move together into one
/// local physics scene before mutual collision or docking can be simulated.
#[derive(Clone, Debug)]
pub struct EncounterPhysicsGate {
    pub ranges: EncounterRanges,
    active: BTreeMap<(String, String), ()>,
}

impl Default for EncounterPhysicsGate {
    fn default() -> Self {
        Self::new(EncounterRanges {
            unpack_meters: 10_000.0,
            pack_meters: 15_000.0,
        })
    }
}

impl EncounterPhysicsGate {
    pub fn new(ranges: EncounterRanges) -> Self {
        assert!(
            ranges.unpack_meters > 0.0 && ranges.pack_meters > ranges.unpack_meters,
            "encounter gate: ranges {ranges:?}"
        );
        Self {
            ranges,
            active: BTreeMap::new(),
        }
    }

    pub fn update(
        &mut self,
        first_id: &str,
        first: FrameState,
        second_id: &str,
        second: FrameState,
        lookahead_seconds: f64,
    ) -> EncounterPairState {
        assert!(
            !first_id.is_empty() && !second_id.is_empty() && first_id != second_id,
            "encounter gate: expected two distinct vessel ids"
        );
        assert!(
            lookahead_seconds >= 0.0 && lookahead_seconds.is_finite(),
            "encounter gate: lookahead {lookahead_seconds}"
        );
        let key = if first_id < second_id {
            (first_id.to_string(), second_id.to_string())
        } else {
            (second_id.to_string(), first_id.to_string())
        };
        let d = second.position - first.position;
        let distance_meters = hypot([d.x, d.y, d.z]);
        assert!(
            distance_meters.is_finite(),
            "encounter gate: non-finite position"
        );
        let v = second.velocity - first.velocity;
        let speed_squared = v.x * v.x + v.y * v.y + v.z * v.z;
        let time = if speed_squared > 0.0 {
            (-(d.x * v.x + d.y * v.y + d.z * v.z) / speed_squared).clamp(0.0, lookahead_seconds)
        } else {
            0.0
        };
        let c = DVec3::new(d.x + v.x * time, d.y + v.y * time, d.z + v.z * time);
        let closest = hypot([c.x, c.y, c.z]);
        let was_active = self.active.contains_key(&key);
        let threshold = if was_active {
            self.ranges.pack_meters
        } else {
            self.ranges.unpack_meters
        };
        let physics = distance_meters <= threshold || closest < threshold;
        if physics && !was_active {
            self.active.insert(key, ());
        } else if !physics && was_active {
            self.active.remove(&key);
        }
        EncounterPairState {
            first: first_id.into(),
            second: second_id.into(),
            distance_meters,
            physics,
            changed: physics != was_active,
            closest_approach_meters: closest,
            time_to_closest_approach_seconds: time,
        }
    }

    pub fn is_physics_active(&self, vessel_id: &str) -> bool {
        self.active
            .keys()
            .any(|(a, b)| a == vessel_id || b == vessel_id)
    }

    pub fn active_pairs(&self) -> Vec<(String, String)> {
        self.active.keys().cloned().collect()
    }

    pub fn remove_vessel(&mut self, vessel_id: &str) {
        self.active
            .retain(|(a, b), _| a != vessel_id && b != vessel_id);
    }
}

impl EncounterPhysicsGate {
    /// Restore hysteresis independently of current pair distance. Recomputing it would change
    /// ownership for a pair inside the pack/unpack band immediately after loading.
    pub fn restore_pairs(&mut self, pairs: Vec<(String, String)>) {
        assert!(
            self.active.is_empty(),
            "encounter checkpoint: gate already has pairs"
        );
        for (a, b) in pairs {
            assert!(
                a < b && self.active.insert((a, b), ()).is_none(),
                "encounter checkpoint: invalid/duplicate pair"
            );
        }
    }
}

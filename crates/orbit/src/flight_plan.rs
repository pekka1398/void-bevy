//! Burns at full thrust and the trajectory they give.

use glam::DVec3;
use void_frames::BodyId;

use crate::apsides::{ApsisKind, find_apsides};
use crate::ephemeris::EphemerisSource;
use crate::kepler::osculating_orbit;
use crate::propagator::{
    AdvanceOutcome, AttitudeLaw, Control, Impact, PropagationRun, ThrustControl, Tolerances,
    VesselPropagator, VesselState,
};
use crate::trajectory::Trajectory;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReferenceMode {
    /// Whoever edits the plan keeps the reference on the body whose sphere of influence holds
    /// the planned trajectory at ignition.
    Auto,
    Fixed,
}

/// A planned burn: Δv components along the Frenet axes relative to `reference_body`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManeuverSpec {
    pub start_time: f64,
    pub reference_body: usize,
    pub reference_mode: ReferenceMode,
    /// m/s along the velocity relative to the reference body.
    pub prograde: f64,
    /// m/s along the orbit normal r × v.
    pub normal: f64,
    /// m/s along prograde × normal (radial out on a circular orbit).
    pub radial: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanEngine {
    pub thrust_newtons: f64,
    pub exhaust_velocity: f64,
    pub dry_mass_kg: f64,
}

/// A maneuver made concrete: full thrust from `start_time` until the Δv is spent.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BurnSchedule {
    pub start_time: f64,
    pub end_time: f64,
    pub delta_v: f64,
    pub mass_before_kg: f64,
    pub mass_after_kg: f64,
    /// None for a zero Δv, which takes no time and does nothing.
    pub control: Option<Control>,
}

/// A burn, or why it cannot happen.
pub type ManeuverStatus = Result<BurnSchedule, String>;

/// Coasting steps allowed when searching for an apsis to centre a burn on.
const APSIS_SEARCH_MAX_STEPS: u64 = 200_000;
/// Steps `position_at` may spend integrating ahead.
const POSITION_MAX_STEPS: u64 = 1_000_000;

/// A sequence of burns executed at full thrust, and the trajectory they give from an anchor
/// state. Burns are checked in order; the first one that cannot happen (in the past, overlapping,
/// not enough propellant) ends the executable plan, and every later burn is blocked by it.
pub struct FlightPlan {
    pub trajectory: Trajectory,
    /// Increments whenever the trajectory restarts.
    pub generation: u64,
    /// Burns executed and removed since construction.
    pub completed_count: u64,
    propagator: VesselPropagator,
    engine: PlanEngine,
    body_count: usize,
    coast: f64,
    specs: Vec<ManeuverSpec>,
    statuses: Vec<ManeuverStatus>,
    schedule: Vec<BurnSchedule>,
    anchor: Option<PropagationRun>,
    run: Option<PropagationRun>,
}

mod checkpoint;
pub use checkpoint::FlightPlanCheckpoint;

fn checked_coast(value: f64) -> f64 {
    assert!(
        value > 0.0 && value.is_finite(),
        "flight plan: coast {value}"
    );
    value
}

impl FlightPlan {
    pub fn new(
        ephemeris: &dyn EphemerisSource,
        tolerances: Tolerances,
        engine: PlanEngine,
        coast_seconds: f64,
    ) -> Self {
        assert!(
            engine.thrust_newtons > 0.0
                && engine.thrust_newtons.is_finite()
                && engine.exhaust_velocity > 0.0
                && engine.exhaust_velocity.is_finite()
                && engine.dry_mass_kg > 0.0
                && engine.dry_mass_kg.is_finite(),
            "flight plan: engine {engine:?}"
        );
        Self {
            trajectory: Trajectory::new(),
            generation: 0,
            completed_count: 0,
            propagator: VesselPropagator::new(ephemeris, tolerances),
            engine,
            body_count: ephemeris.bodies().len(),
            coast: checked_coast(coast_seconds),
            specs: Vec::new(),
            statuses: Vec::new(),
            schedule: Vec::new(),
            anchor: None,
            run: None,
        }
    }

    /// Recheck future burns against the current staged propulsion without losing completion history.
    pub fn set_engine(&mut self, engine: PlanEngine) {
        assert!(
            engine.thrust_newtons.is_finite()
                && engine.thrust_newtons > 0.0
                && engine.exhaust_velocity.is_finite()
                && engine.exhaust_velocity > 0.0
                && engine.dry_mass_kg.is_finite()
                && engine.dry_mass_kg > 0.0,
            "flight plan: invalid engine"
        );
        self.engine = engine;
        self.restart();
    }

    pub fn count(&self) -> usize {
        self.specs.len()
    }

    /// The executable prefix of the plan.
    pub fn burns(&self) -> &[BurnSchedule] {
        &self.schedule
    }

    pub fn coast_seconds(&self) -> f64 {
        self.coast
    }

    /// Lengthening keeps what is integrated and continues; shortening restarts.
    pub fn set_coast_seconds(&mut self, value: f64) {
        let shorter = checked_coast(value) < self.coast;
        self.coast = value;
        if shorter {
            self.restart();
        }
    }

    pub fn anchor_time(&self) -> f64 {
        self.anchor.as_ref().expect("flight plan: no anchor").time
    }

    /// Coasting ends this long after the last executable burn.
    pub fn end_time(&self) -> f64 {
        self.schedule
            .last()
            .map_or_else(|| self.anchor_time(), |b| b.end_time)
            + self.coast
    }

    /// How far the trajectory has been integrated.
    pub fn computed_until(&self) -> f64 {
        self.run.as_ref().expect("flight plan: no anchor").time
    }

    pub fn impact(&self) -> Option<Impact> {
        self.run.as_ref().and_then(|r| r.impact)
    }

    pub fn complete(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.impact.is_some() || r.time >= self.end_time())
    }

    pub fn maneuver(&self, i: usize) -> ManeuverSpec {
        *self
            .specs
            .get(i)
            .unwrap_or_else(|| panic!("flight plan: maneuver {i} of {}", self.specs.len()))
    }

    pub fn status(&self, i: usize) -> &ManeuverStatus {
        self.statuses
            .get(i)
            .unwrap_or_else(|| panic!("flight plan: maneuver {i} of {}", self.specs.len()))
    }

    /// Start planning from this state; the run is copied.
    pub fn rebase(&mut self, state: &PropagationRun) {
        assert!(
            state.impact.is_none(),
            "flight plan: cannot plan from an impact"
        );
        self.anchor = Some(state.restarted());
        self.restart();
    }

    pub fn add(&mut self, spec: ManeuverSpec) -> usize {
        let spec = self.checked(spec);
        self.specs.push(spec);
        self.restart();
        self.specs.len() - 1
    }

    pub fn replace(&mut self, i: usize, spec: ManeuverSpec) {
        self.maneuver(i);
        self.specs[i] = self.checked(spec);
        self.restart();
    }

    pub fn remove(&mut self, i: usize) {
        self.maneuver(i);
        self.specs.remove(i);
        self.restart();
    }

    pub fn clear(&mut self) {
        self.specs.clear();
        self.restart();
    }

    /// The first burn has been flown: drop it and continue planning from the state it left.
    pub fn complete_first(&mut self, state: &PropagationRun) {
        assert!(
            !self.schedule.is_empty(),
            "flight plan: no executable burn to complete"
        );
        self.specs.remove(0);
        self.completed_count += 1;
        self.rebase(state);
    }

    /// Integrate the planned trajectory further by at most `max_steps` accepted steps.
    pub fn extend(&mut self, ephemeris: &mut dyn EphemerisSource, max_steps: u64) {
        if !self.specs.is_empty() {
            self.integrate(ephemeris, self.end_time(), max_steps);
        }
    }

    /// Barycentric vessel position on the plan at t, integrating that far now. None when the plan
    /// hits a surface before t or t precedes the plan.
    pub fn position_at(&mut self, ephemeris: &mut dyn EphemerisSource, t: f64) -> Option<DVec3> {
        assert!(self.run.is_some(), "flight plan: no anchor");
        if t < self.trajectory.first_time() {
            return None;
        }
        let before = self.propagator.accepted_steps;
        loop {
            let run = self.run.as_ref().expect("checked above");
            if run.time >= t || run.impact.is_some() {
                break;
            }
            assert!(
                self.propagator.accepted_steps - before <= POSITION_MAX_STEPS,
                "flight plan: more than {POSITION_MAX_STEPS} steps to reach T+{t}"
            );
            self.integrate(ephemeris, t, 5000);
        }
        if let Some(impact) = self.impact()
            && impact.time < t
        {
            return None;
        }
        Some(self.trajectory.sample(t).0)
    }

    fn integrate(&mut self, ephemeris: &mut dyn EphemerisSource, end: f64, max_steps: u64) {
        let Some(run) = self.run.as_mut() else { return };
        let mut left = max_steps;
        while run.impact.is_none() && run.time < end && left > 0 {
            let mut leg_end = end;
            let mut control = None;
            if let Some(burn) = self.schedule.iter().find(|b| b.end_time > run.time) {
                if run.time < burn.start_time {
                    leg_end = end.min(burn.start_time);
                } else {
                    leg_end = end.min(burn.end_time);
                    control = burn.control;
                }
            }
            let before = self.propagator.accepted_steps;
            let outcome = self.propagator.advance(
                ephemeris,
                run,
                leg_end,
                left,
                Some(&mut self.trajectory),
                control,
            );
            left -= self.propagator.accepted_steps - before;
            if outcome != AdvanceOutcome::Reached {
                break;
            }
        }
    }

    /// Predicted state immediately after existing burns, suitable as an append-only planning
    /// anchor. Uses the existing finite-thrust schedule without changing or removing nodes.
    pub fn tail_state(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
    ) -> Result<PropagationRun, String> {
        if let Some(reason) = self.statuses.iter().find_map(|s| s.as_ref().err()) {
            return Err(format!("existing plan is not executable: {reason}"));
        }
        let anchor = self.anchor.as_ref().expect("flight plan: no anchor");
        let Some(burn) = self.schedule.last().copied() else {
            return Ok(anchor.restarted());
        };
        self.integrate(ephemeris, burn.end_time, POSITION_MAX_STEPS);
        if self.impact().is_some_and(|i| i.time <= burn.end_time) {
            return Err("existing plan impacts before its final burn ends".into());
        }
        if self.computed_until() < burn.end_time {
            return Err("existing plan exhausted its prediction budget".into());
        }
        let (position, velocity) = self.trajectory.sample(burn.end_time);
        Ok(PropagationRun::new(VesselState {
            time: burn.end_time,
            position,
            velocity,
            mass_kg: burn.mass_after_kg,
        }))
    }

    /// Drop trajectory samples before t (keeping the one bracketing it).
    pub fn trim_before(&mut self, t: f64) {
        if self.trajectory.count() > 0 {
            self.trajectory.trim_before(t);
        }
    }

    /// A start time centring maneuver i on the next apsis of the coast before it (the state after
    /// burn i − 1, or the anchor), at or after `not_before`.
    pub fn start_at_apsis(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        i: usize,
        kind: ApsisKind,
        not_before: f64,
    ) -> Result<f64, String> {
        let spec = self.maneuver(i);
        let anchor = self.anchor.as_ref().expect("flight plan: no anchor");
        let (mut from, earliest) = if i == 0 {
            (anchor.restarted(), anchor.time.max(not_before))
        } else {
            let previous = match &self.statuses[i - 1] {
                Ok(burn) => *burn,
                Err(_) => return Err(format!("burn {i} is not executable")),
            };
            let t = previous.end_time;
            // The coast starts where burn i − 1 ends; integrate the plan that far now.
            let before = self.propagator.accepted_steps;
            loop {
                let run = self.run.as_ref().expect("an anchored plan has a run");
                if run.time >= t
                    || run.impact.is_some()
                    || self.propagator.accepted_steps - before >= APSIS_SEARCH_MAX_STEPS
                {
                    break;
                }
                self.extend(ephemeris, 5000);
            }
            let run = self.run.as_ref().expect("an anchored plan has a run");
            if run.impact.is_some_and(|impact| impact.time <= t) {
                return Err("the plan hits a surface before this burn".into());
            }
            if run.time < t {
                return Err("the plan before this burn ran out of steps".into());
            }
            let (position, velocity) = self.trajectory.sample(t);
            let state = VesselState {
                time: t,
                position,
                velocity,
                mass_kg: previous.mass_after_kg,
            };
            (PropagationRun::new(state), t.max(not_before))
        };
        let mass = from.state().mass_kg;
        let dv = DVec3::new(spec.prograde, spec.normal, spec.radial).length();
        let mass_after = mass * (-dv / self.engine.exhaust_velocity).exp();
        if mass_after < self.engine.dry_mass_kg {
            return Err("not enough propellant for this burn".into());
        }
        let half_burn =
            0.5 * (mass - mass_after) * self.engine.exhaust_velocity / self.engine.thrust_newtons;

        let reference = spec.reference_body;
        ephemeris.extend_to(from.time);
        let (center_position, center_velocity) = ephemeris.body_state(BodyId(reference), from.time);
        let state = from.state();
        let body = &ephemeris.bodies()[reference];
        let (gm, name) = (body.gm, body.name.clone());
        let osc = osculating_orbit(
            state.position - center_position,
            state.velocity - center_velocity,
            gm,
        );
        let window = if osc.period_seconds.is_finite() {
            2.2 * osc.period_seconds
        } else {
            self.coast
        } + (earliest - from.time)
            + half_burn;
        let mut path = Trajectory::new();
        path.append(from.time, &from.y);
        let search_end = from.time + window;
        let outcome = self.propagator.advance(
            ephemeris,
            &mut from,
            search_end,
            APSIS_SEARCH_MAX_STEPS,
            Some(&mut path),
            None,
        );
        if outcome == AdvanceOutcome::Budget {
            return Err("apsis search ran out of steps".into());
        }
        let apsis = find_apsides(&path, ephemeris, reference, earliest, 16)
            .into_iter()
            .find(|a| a.kind == kind && a.time - half_burn >= earliest);
        match apsis {
            Some(a) => Ok(a.time - half_burn),
            None => {
                let what = match kind {
                    ApsisKind::Periapsis => "periapsis",
                    ApsisKind::Apoapsis => "apoapsis",
                };
                let place = if matches!(outcome, AdvanceOutcome::Impact { .. }) {
                    " before impact"
                } else {
                    ""
                };
                Err(format!("no {what} of {name}{place}"))
            }
        }
    }

    fn checked(&self, spec: ManeuverSpec) -> ManeuverSpec {
        for (name, value) in [
            ("start time", spec.start_time),
            ("prograde", spec.prograde),
            ("normal", spec.normal),
            ("radial", spec.radial),
        ] {
            assert!(value.is_finite(), "flight plan: maneuver {name} = {value}");
        }
        assert!(
            spec.reference_body < self.body_count,
            "flight plan: reference body {}",
            spec.reference_body
        );
        spec
    }

    /// Re-check every burn and restart the trajectory from the anchor.
    fn restart(&mut self) {
        self.generation += 1;
        self.trajectory.clear();
        self.statuses.clear();
        self.schedule.clear();
        let Some(anchor) = &self.anchor else {
            self.run = None;
            assert!(
                self.specs.is_empty(),
                "flight plan: maneuvers without an anchor"
            );
            return;
        };
        let PlanEngine {
            thrust_newtons,
            exhaust_velocity,
            dry_mass_kg,
        } = self.engine;
        let mut mass = anchor.state().mass_kg;
        let mut previous_end = anchor.time;
        let mut blocked = false;
        for (i, spec) in self.specs.iter().enumerate() {
            if blocked {
                self.statuses.push(Err("blocked by an earlier burn".into()));
                continue;
            }
            let dv = DVec3::new(spec.prograde, spec.normal, spec.radial).length();
            let mass_after = mass * (-dv / exhaust_velocity).exp();
            let reason = if spec.start_time < previous_end {
                Some(if i == 0 {
                    "starts in the past".to_string()
                } else {
                    format!("starts before burn {i} ends")
                })
            } else if mass_after < dry_mass_kg {
                Some(format!(
                    "needs {dv:.1} m/s, {:.1} m/s left",
                    exhaust_velocity * (mass / dry_mass_kg).ln()
                ))
            } else {
                None
            };
            if let Some(reason) = reason {
                self.statuses.push(Err(reason));
                blocked = true;
                continue;
            }
            let duration = (mass - mass_after) * exhaust_velocity / thrust_newtons;
            let burn = BurnSchedule {
                start_time: spec.start_time,
                end_time: spec.start_time + duration,
                delta_v: dv,
                mass_before_kg: mass,
                mass_after_kg: mass_after,
                control: (dv != 0.0).then(|| {
                    Control::Thrust(ThrustControl {
                        thrust_newtons,
                        exhaust_velocity,
                        minimum_mass_kg: dry_mass_kg,
                        attitude: AttitudeLaw::Frenet {
                            reference_body: spec.reference_body,
                            tangent: spec.prograde / dv,
                            normal: spec.normal / dv,
                            radial: spec.radial / dv,
                        },
                    })
                }),
            };
            self.statuses.push(Ok(burn));
            self.schedule.push(burn);
            mass = mass_after;
            previous_end = burn.end_time;
        }
        let run = anchor.restarted();
        self.trajectory.append(run.time, &run.y);
        self.run = Some(run);
    }
}

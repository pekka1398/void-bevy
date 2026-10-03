//! Preserve a plan's anchor, computed trajectory and propagation memory without recomputing it.
use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightPlanCheckpoint {
    version: u32,
    trajectory: Trajectory,
    generation: u64,
    completed_count: u64,
    engine: PlanEngine,
    body_count: usize,
    coast: f64,
    specs: Vec<ManeuverSpec>,
    statuses: Vec<ManeuverStatus>,
    schedule: Vec<BurnSchedule>,
    anchor: Option<PropagationRun>,
    run: Option<PropagationRun>,
    tolerances: Tolerances,
}
impl FlightPlan {
    pub fn checkpoint(&self) -> FlightPlanCheckpoint {
        FlightPlanCheckpoint {
            version: 1,
            trajectory: self.trajectory.clone(),
            generation: self.generation,
            completed_count: self.completed_count,
            engine: self.engine,
            body_count: self.body_count,
            coast: self.coast,
            specs: self.specs.clone(),
            statuses: self.statuses.clone(),
            schedule: self.schedule.clone(),
            anchor: self.anchor.clone(),
            run: self.run.clone(),
            tolerances: self.propagator.tolerances,
        }
    }
    pub fn from_checkpoint(source: &dyn EphemerisSource, saved: FlightPlanCheckpoint) -> Self {
        assert_eq!(saved.version, 1, "plan checkpoint: unsupported version");
        assert_eq!(
            saved.body_count,
            source.bodies().len(),
            "plan checkpoint: body count changed"
        );
        assert_eq!(
            saved.statuses.len(),
            saved.specs.len(),
            "plan checkpoint: missing statuses"
        );
        assert!(
            saved.schedule.len() <= saved.specs.len(),
            "plan checkpoint: extra scheduled burns"
        );
        let mut plan = Self::new(source, saved.tolerances, saved.engine, saved.coast);
        for spec in &saved.specs {
            plan.checked(*spec);
        }
        for run in saved.anchor.iter().chain(saved.run.iter()) {
            assert!(
                run.time.is_finite()
                    && run.y.iter().chain(&run.dy).all(|x| x.is_finite())
                    && run.y[6] > 0.0
                    && run.step_hint > 0.0
                    && run.step_hint.is_finite(),
                "plan checkpoint: invalid propagation state"
            );
        }
        assert_eq!(
            saved.anchor.is_some(),
            saved.run.is_some(),
            "plan checkpoint: incomplete anchor"
        );
        if saved.anchor.is_none() {
            assert!(
                saved.specs.is_empty() && saved.trajectory.count() == 0,
                "plan checkpoint: data without an anchor"
            );
        }
        let mut previous = f64::NEG_INFINITY;
        for i in 0..saved.trajectory.count() {
            let time = saved.trajectory.time(i);
            assert!(
                time.is_finite()
                    && time > previous
                    && saved.trajectory.position(i).is_finite()
                    && saved.trajectory.velocity(i).is_finite(),
                "plan checkpoint: invalid trajectory"
            );
            previous = time;
        }
        for burn in &saved.schedule {
            assert!(
                burn.start_time.is_finite()
                    && burn.end_time.is_finite()
                    && burn.end_time >= burn.start_time
                    && burn.delta_v.is_finite()
                    && burn.delta_v >= 0.0
                    && burn.mass_before_kg.is_finite()
                    && burn.mass_after_kg.is_finite()
                    && burn.mass_before_kg >= burn.mass_after_kg
                    && burn.mass_after_kg > 0.0,
                "plan checkpoint: invalid burn"
            );
            if let Some(control) = burn.control {
                control.assert_valid(saved.body_count);
            }
        }
        // Cached decisions must agree with the logical inputs, not merely look finite.
        plan.anchor = saved.anchor.clone();
        plan.specs = saved.specs.clone();
        plan.restart();
        assert_eq!(
            plan.statuses, saved.statuses,
            "plan checkpoint: inconsistent burn statuses"
        );
        assert_eq!(
            plan.schedule, saved.schedule,
            "plan checkpoint: inconsistent burn schedule"
        );
        if let (Some(anchor), Some(run)) = (&saved.anchor, &saved.run) {
            assert!(
                run.time >= anchor.time,
                "plan checkpoint: run precedes anchor"
            );
        }
        plan.trajectory = saved.trajectory;
        plan.generation = saved.generation;
        plan.completed_count = saved.completed_count;
        plan.specs = saved.specs;
        plan.statuses = saved.statuses;
        plan.schedule = saved.schedule;
        plan.anchor = saved.anchor;
        plan.run = saved.run;
        plan
    }
}

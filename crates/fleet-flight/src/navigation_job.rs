//! Paused planning jobs own their prediction data. Completion is an ordinary journal command.
use crate::{FleetFlight, plans::VesselPlan};
use serde::{Deserialize, Serialize};
use void_orbit::{
    FlightPlan, FlightPlanCheckpoint, NavigationRequest, PlanEngine, PropagationRun, Tolerances,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationInputs {
    vessel: String,
    world: serde_json::Value,
    system: void_frames::SystemId,
    origin: void_frames::SplitPosition,
    state: PropagationRun,
    engine: PlanEngine,
    tolerances: Tolerances,
    specs: Vec<void_orbit::ManeuverSpec>,
    completed: u64,
    generation: u64,
    coast: f64,
}
impl NavigationInputs {
    fn same_as(&self, other: &Self) -> bool {
        self.vessel == other.vessel
            && self.world == other.world
            && self.system == other.system
            && self.origin == other.origin
            && self.state.time == other.state.time
            && self.state.y == other.state.y
            && self.state.dy == other.state.dy
            && self.state.step_hint == other.state.step_hint
            && self.state.impact == other.state.impact
            && self.engine == other.engine
            && self.tolerances == other.tolerances
            && self.specs == other.specs
            && self.completed == other.completed
            && self.generation == other.generation
            && self.coast == other.coast
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedNavigation {
    inputs: NavigationInputs,
    plan: FlightPlanCheckpoint,
    message: String,
    ephemeris_end: f64,
    reserved_bytes: u64,
    ephemeris_steps: u64,
    vessel_trials: u64,
}
pub(crate) struct NavigationWork {
    inputs: NavigationInputs,
    source: Box<dyn void_orbit::PredictionSnapshot>,
    request: NavigationRequest,
    auto_reference: bool,
}
pub(crate) struct NavigationResult {
    pub prepared: PreparedNavigation,
    pub source: Box<dyn void_orbit::PredictionSnapshot>,
    pub encoded: EncodedNavigation,
}
/// Only the worker can construct this payload; live journals cannot accept external raw JSON.
pub(crate) struct EncodedNavigation(Vec<u8>);
impl EncodedNavigation {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
#[derive(Serialize)]
#[serde(tag = "kind")]
enum BorrowedNavigationAction<'a> {
    CommitNavigation { prepared: &'a PreparedNavigation },
}
struct JsonSize(usize);
impl std::io::Write for JsonSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl PreparedNavigation {
    pub(crate) fn message(&self) -> String {
        self.message.clone()
    }
    pub(crate) fn metrics(&self) -> String {
        format!(
            "{:.1} MiB reserved · {} celestial steps · {} vessel trials",
            self.reserved_bytes as f64 / (1024. * 1024.),
            self.ephemeris_steps,
            self.vessel_trials
        )
    }
}
impl FleetFlight {
    fn navigation_inputs(&self, id: &str) -> Result<NavigationInputs, String> {
        if !self.fleet.vessel_ids().iter().any(|v| v == id) {
            return Err("Navigation vessel no longer exists".into());
        }
        let existing = self.plans.get(id);
        if existing.is_some_and(|p| p.executing) {
            return Err("Abort the executing maneuver before generating navigation".into());
        }
        let precise = self.fleet.precise_snapshot(id);
        let (system, origin) =
            existing.map_or((precise.system, precise.anchor), |p| (p.system, p.origin));
        Ok(NavigationInputs {
            vessel: id.into(),
            world: serde_json::to_value(&self.world).unwrap(),
            system,
            origin,
            state: self.plan_state(id),
            engine: self.plan_engine(id)?,
            tolerances: self.fleet.options.tolerances,
            specs: existing.map_or_else(Vec::new, |p| {
                (0..p.plan.count()).map(|i| p.plan.maneuver(i)).collect()
            }),
            completed: existing.map_or(0, |p| p.plan.completed_count),
            generation: existing.map_or(0, |p| p.plan.generation),
            coast: existing.map_or(6000., |p| p.plan.coast_seconds()),
        })
    }
    pub(crate) fn navigation_work(
        &self,
        request: NavigationRequest,
        cancel: void_orbit::CancellationToken,
        auto_reference: bool,
    ) -> Result<NavigationWork, String> {
        let inputs = self.navigation_inputs(&self.selected)?;
        let view = self.plan_view(&self.selected);
        let source = view.as_deref().unwrap_or(self.fleet.ephemeris.as_ref());
        let source = source
            .prediction_snapshot(void_orbit::PredictionBudget::default(), cancel)
            .map_err(|e| e.to_string())?;
        Ok(NavigationWork {
            inputs,
            source,
            request,
            auto_reference,
        })
    }
    pub(crate) fn validated_navigation(
        &self,
        prepared: &PreparedNavigation,
    ) -> Result<(NavigationInputs, FlightPlan), String> {
        let actual = self.navigation_inputs(&prepared.inputs.vessel)?;
        if !actual.same_as(&prepared.inputs) {
            return Err("Navigation result is stale: vessel, engine, frame or plan changed".into());
        }
        let view = self.plan_view(&prepared.inputs.vessel);
        let source = view.as_deref().unwrap_or(self.fleet.ephemeris.as_ref());
        let candidate = FlightPlan::from_checkpoint(source, prepared.plan.clone());
        assert!(
            candidate.matches_navigation_inputs(
                &actual.state,
                actual.engine,
                actual.tolerances,
                &actual.specs
            ),
            "navigation checkpoint: inconsistent inputs or unfinished result"
        );
        assert_eq!(
            candidate.generation,
            actual
                .generation
                .checked_add(1)
                .expect("navigation plan generation exhausted"),
            "navigation checkpoint: generation did not advance"
        );
        assert_eq!(
            candidate.completed_count, actual.completed,
            "navigation checkpoint: completed count changed"
        );
        assert!(
            prepared.ephemeris_end.is_finite()
                && prepared.ephemeris_end >= candidate.computed_until(),
            "navigation checkpoint: invalid ephemeris horizon"
        );
        let budget = void_orbit::PredictionBudget::default();
        assert!(
            prepared.reserved_bytes <= budget.bytes as u64
                && prepared.ephemeris_steps <= budget.ephemeris_steps
                && prepared.vessel_trials <= budget.vessel_trials,
            "navigation checkpoint: invalid budget accounting"
        );
        // Reject absurd or forged horizons before any extension. A real prediction retained every
        // sample in this interval and therefore must fit even this lower-bound storage estimate.
        let samples =
            ((prepared.ephemeris_end - source.start_time()) / source.step_seconds()).max(0.);
        let minimum_bytes = samples * source.bodies().len() as f64 * 9. * 8.;
        if !minimum_bytes.is_finite() || minimum_bytes > budget.bytes as f64 {
            return Err(
                "Navigation checkpoint ephemeris horizon exceeds prediction memory budget".into(),
            );
        }
        Ok((actual, candidate))
    }
    pub(crate) fn commit_navigation(
        &mut self,
        prepared: &PreparedNavigation,
    ) -> Result<(), String> {
        let (actual, candidate) = self.validated_navigation(prepared)?;
        // Replay reconstructs only celestial coverage. Live completion already adopted the worker's exact cache.
        if self.fleet.ephemeris.end_time() < prepared.ephemeris_end {
            let snapshot = self
                .fleet
                .ephemeris
                .prediction_snapshot(
                    void_orbit::PredictionBudget::default(),
                    void_orbit::CancellationToken::new(),
                )
                .map_err(|e| e.to_string())?;
            let mut replay_source = snapshot.into_source();
            replay_source
                .try_extend_to(prepared.ephemeris_end)
                .map_err(|e| e.to_string())?;
            let snapshot = replay_source
                .export_prediction()
                .map_err(|e| e.to_string())?;
            self.fleet
                .ephemeris
                .adopt_prediction(snapshot)
                .map_err(|e| e.to_string())?;
        }
        self.install_navigation(actual, candidate, prepared.message.clone());
        Ok(())
    }
    pub(crate) fn install_navigation(
        &mut self,
        actual: NavigationInputs,
        candidate: FlightPlan,
        message: String,
    ) {
        let selected = candidate.count() - 1;
        self.cancel_maneuver_warp("navigation node generated");
        self.plans.insert(
            actual.vessel,
            VesselPlan {
                system: actual.system,
                origin: actual.origin,
                plan: candidate,
                selected,
                executing: false,
                message,
            },
        );
    }
}
impl NavigationWork {
    pub(crate) fn run(mut self) -> Result<NavigationResult, String> {
        let mut source = self.source.into_source();
        let source = source.as_mut();
        let i = &self.inputs;
        let mut candidate = FlightPlan::new(source, i.tolerances, i.engine, i.coast);
        candidate.rebase(&i.state);
        for spec in &i.specs {
            candidate.add(*spec);
        }
        candidate.completed_count = i.completed;
        let tail = candidate.tail_state(source);
        source
            .prediction_context()
            .expect("prediction source context")
            .check()
            .map_err(|e| e.to_string())?;
        let anchor = tail?;
        source
            .try_extend_to(anchor.time)
            .map_err(|e| e.to_string())?;
        let mut positions = vec![glam::DVec3::ZERO; source.bodies().len()];
        source.positions_at(anchor.time, &mut positions);
        if self.auto_reference {
            self.request.reference_body = void_orbit::DominanceTree::new(source.bodies())
                .dominant(&positions, anchor.state().position);
        }
        if self.request.earliest_departure < anchor.time {
            return Err(format!(
                "Earliest departure must be after existing burns end at T+{:.1}",
                anchor.time
            ));
        }
        let solution =
            void_orbit::solve_navigation(source, &anchor, i.engine, i.tolerances, &self.request)
                .map_err(|e| e.to_string())?;
        let selected = candidate.add(solution.maneuver);
        candidate.status(selected).as_ref().map_err(Clone::clone)?;
        let last_end = candidate.burns().last().expect("navigation burn").end_time;
        candidate.set_coast_seconds(
            candidate
                .coast_seconds()
                .max(solution.verified_until - last_end),
        );
        candidate.extend(source, 2_000_000);
        source
            .prediction_context()
            .expect("prediction source context")
            .check()
            .map_err(|e| e.to_string())?;
        if let Some(impact) = candidate.impact() {
            return Err(format!(
                "Appended navigation plan impacts body {} at T+{:.1}",
                impact.body, impact.time
            ));
        }
        if !candidate.complete() {
            return Err("Navigation cancelled or exhausted prediction budget".into());
        }
        let message = format!(
            "{:?} node {} appended: ignition T+{:.1}, Δv {:.1} m/s; closest distance {:.1} km, altitude {:.1} km, relative speed {:.1} m/s{}",
            self.request.operation,
            selected + 1,
            solution.maneuver.start_time,
            solution.delta_v_mps,
            solution.closest_distance_m / 1000.,
            solution.periapsis_altitude_m / 1000.,
            solution.relative_speed_mps,
            if solution.captured {
                "; bound orbit verified"
            } else {
                ""
            }
        );
        // Bound transient checkpoint, journal and completion copies as well as integration storage.
        source
            .prediction_context()
            .expect("prediction source context")
            .reserve_bytes(candidate.trajectory.count().saturating_mul(7 * 8 * 8))
            .map_err(|e| e.to_string())?;
        if candidate.trajectory.count() > 131_072 {
            return Err(format!(
                "Navigation result requires {} trajectory samples; delivery budget is 131072",
                candidate.trajectory.count()
            ));
        }
        candidate.generation = i
            .generation
            .checked_add(1)
            .expect("navigation plan generation exhausted");
        let ephemeris_end = source.end_time();
        let usage = source.prediction_context().unwrap().usage();
        let mut prepared = PreparedNavigation {
            inputs: self.inputs,
            plan: candidate.checkpoint(),
            message,
            ephemeris_end,
            reserved_bytes: usage.reserved_bytes,
            ephemeris_steps: usage.ephemeris_steps,
            vessel_trials: usage.vessel_trials,
        };
        // Count before allocation, including room for the updated accounting field's digits.
        // Encoding stays on this worker; the main thread still fsyncs the intent before mutation.
        let mut size = JsonSize(0);
        serde_json::to_writer(
            &mut size,
            &BorrowedNavigationAction::CommitNavigation {
                prepared: &prepared,
            },
        )
        .expect("navigation JSON is finite");
        let context = source.prediction_context().unwrap();
        context
            .reserve_bytes(size.0.saturating_add(64))
            .map_err(|e| e.to_string())?;
        prepared.reserved_bytes = context.usage().reserved_bytes;
        let mut encoded = Vec::with_capacity(size.0 + 64);
        serde_json::to_writer(
            &mut encoded,
            &BorrowedNavigationAction::CommitNavigation {
                prepared: &prepared,
            },
        )
        .expect("navigation JSON is finite");
        assert!(
            encoded.len() <= size.0 + 64,
            "navigation encoding exceeded reserved bytes"
        );
        context.check().map_err(|e| e.to_string())?;
        let snapshot = source.export_prediction().map_err(|e| e.to_string())?;
        Ok(NavigationResult {
            prepared,
            source: snapshot,
            encoded: EncodedNavigation(encoded),
        })
    }
}

pub(crate) struct NavigationJob {
    pub cancel: void_orbit::CancellationToken,
    pub result: std::sync::mpsc::Receiver<Result<NavigationResult, String>>,
    pub started: std::time::Instant,
}
impl Drop for NavigationJob {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

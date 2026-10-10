//! Per-vessel maneuver plans. Planning does not spend fuel; execution uses Fleet's engine groups.
use crate::FleetFlight;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use void_orbit::{
    Control, FlightPlan, FlightPlanCheckpoint, ManeuverSpec, PropagationRun, ReferenceMode,
    VesselState,
};
use void_vessels::GuidanceStatus;

pub struct VesselPlan {
    pub system: void_frames::SystemId,
    pub origin: void_frames::SplitPosition,
    pub plan: FlightPlan,
    pub selected: usize,
    pub executing: bool,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedVesselPlan {
    system: void_frames::SystemId,
    origin: void_frames::SplitPosition,
    plan: FlightPlanCheckpoint,
    selected: usize,
    executing: bool,
    message: String,
}
impl FleetFlight {
    pub(crate) fn install_navigation_plan(&mut self, id: &str, saved: SavedVesselPlan) {
        assert!(
            !saved.executing,
            "navigation result must not ignite engines"
        );
        let precise = self.fleet.precise_snapshot(id);
        let expected_source = self
            .plans
            .get(id)
            .map_or((precise.system, precise.anchor), |p| (p.system, p.origin));
        assert_eq!(
            (saved.system, saved.origin),
            expected_source,
            "navigation result changed coordinate source"
        );
        let view = self.plan_view(id);
        let source = view.as_deref().unwrap_or(self.fleet.ephemeris.as_ref());
        let plan = FlightPlan::from_checkpoint(source, saved.plan);
        let previous_count = self.plans.get(id).map_or(0, |p| p.plan.count());
        assert_eq!(
            plan.count(),
            previous_count + 1,
            "navigation result must append one node"
        );
        assert_eq!(
            saved.selected, previous_count,
            "navigation result must select appended node"
        );
        if let Some(previous) = self.plans.get(id) {
            for i in 0..previous_count {
                assert_eq!(
                    plan.maneuver(i),
                    previous.plan.maneuver(i),
                    "navigation result changed existing maneuver"
                );
            }
        }
        assert!(
            plan.complete() && plan.impact().is_none(),
            "unverified navigation result"
        );
        assert!(saved.selected < plan.count(), "missing navigation node");
        self.cancel_maneuver_warp("navigation node generated");
        self.plans.insert(
            id.into(),
            VesselPlan {
                system: saved.system,
                origin: saved.origin,
                plan,
                selected: saved.selected,
                executing: false,
                message: saved.message,
            },
        );
    }
    pub fn plan_checkpoints(&self) -> BTreeMap<String, SavedVesselPlan> {
        self.plans
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    SavedVesselPlan {
                        system: p.system,
                        origin: p.origin,
                        plan: p.plan.checkpoint(),
                        selected: p.selected,
                        executing: p.executing,
                        message: p.message.clone(),
                    },
                )
            })
            .collect()
    }
    pub(crate) fn restore_plans(&mut self, saved: BTreeMap<String, SavedVesselPlan>) {
        for (id, p) in saved {
            self.fleet.snapshot(&id);
            let view = self.plan_view(&id);
            let source = view.as_deref().unwrap_or(self.fleet.ephemeris.as_ref());
            let plan = FlightPlan::from_checkpoint(source, p.plan);
            assert!(
                p.selected < plan.count().max(1),
                "world checkpoint: invalid selected maneuver"
            );
            assert!(
                !p.executing
                    || self
                        .fleet
                        .guidance(&id)
                        .is_some_and(|g| g.status == GuidanceStatus::Armed),
                "world checkpoint: executing plan has no armed guidance"
            );
            self.plans.insert(
                id,
                VesselPlan {
                    system: p.system,
                    origin: p.origin,
                    plan,
                    selected: p.selected,
                    executing: p.executing,
                    message: p.message,
                },
            );
        }
    }
    fn plan_view(&self, id: &str) -> Option<Box<dyn void_orbit::EphemerisSource>> {
        let (system, origin) = self
            .plans
            .get(id)
            .map(|p| (p.system, p.origin))
            .unwrap_or_else(|| {
                let s = self.fleet.precise_snapshot(id);
                (s.system, s.anchor)
            });
        let mut view = self.fleet.ephemeris.local_view(system);
        if let Some(view) = &mut view {
            view.set_physics_offset(origin);
        } else {
            assert_eq!(
                origin,
                void_frames::SplitPosition::ORIGIN,
                "plan: unsupported anchor"
            );
        }
        view
    }
    fn plan_state(&self, id: &str) -> PropagationRun {
        let precise = self.fleet.precise_snapshot(id);
        let (position, velocity) = if let Some(plan) = self.plans.get(id) {
            let (system_origin, system_velocity) = self
                .fleet
                .ephemeris
                .system_state(plan.system, self.fleet.time());
            (
                precise
                    .position
                    .relative(&system_origin.compose(&plan.origin)),
                precise.velocity - system_velocity,
            )
        } else {
            (precise.residual.position, precise.residual.velocity)
        };
        PropagationRun::new(VesselState {
            time: self.fleet.time(),
            position,
            velocity,
            mass_kg: precise.local.mass_kg,
        })
    }
    pub(crate) fn refresh_plan(&mut self, id: &str) -> Result<(), String> {
        if self.plans.get(id).is_some_and(|p| p.executing) {
            return Err("Abort the executing maneuver before editing".into());
        }
        let engine = self.plan_engine(id)?;
        let state = self.plan_state(id);
        if let Some(p) = self.plans.get_mut(id) {
            p.plan.set_engine(engine);
            p.plan.rebase(&state);
            p.message = "Plan changed; previous navigation metrics are no longer valid".into();
        } else {
            self.plans.insert(
                id.into(),
                VesselPlan {
                    system: self.fleet.vessel_system(id),
                    origin: self.fleet.precise_snapshot(id).anchor,
                    plan: self.new_plan(id, 6000.0)?,
                    selected: 0,
                    executing: false,
                    message: String::new(),
                },
            );
        }
        Ok(())
    }
    /// The default maneuver reference at the end of all already scheduled burns.
    /// Uses the same split-position local ephemeris as planning, never global f32 rendering data.
    pub fn navigation_reference(&mut self, id: &str) -> Result<usize, String> {
        if self.plans.get(id).is_some_and(|p| p.executing) {
            return Err("Abort the executing maneuver before generating navigation".into());
        }
        let engine = self.plan_engine(id)?;
        let state = self.plan_state(id);
        let mut view = self.plan_view(id);
        let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
            Some(view) => view.as_mut(),
            None => self.fleet.ephemeris.as_mut(),
        };
        let mut candidate = if let Some(existing) = self.plans.get(id) {
            FlightPlan::from_checkpoint(source, existing.plan.checkpoint())
        } else {
            FlightPlan::new(source, self.fleet.options.tolerances, engine, 6000.0)
        };
        candidate.set_engine(engine);
        candidate.rebase(&state);
        let tail = candidate.tail_state(source)?;
        let mut positions = vec![glam::DVec3::ZERO; source.bodies().len()];
        source.positions_at(tail.time, &mut positions);
        Ok(void_orbit::DominanceTree::new(source.bodies())
            .dominant(&positions, tail.state().position))
    }
    /// Generate exactly one finite-burn node without changing live fuel or vessel state.
    /// Work on a candidate so refusals also preserve the existing plan and warp state.
    pub fn generate_navigation(
        &mut self,
        id: &str,
        request: &void_orbit::NavigationRequest,
    ) -> Result<(), String> {
        if self.plans.get(id).is_some_and(|p| p.executing) {
            return Err("Abort the executing maneuver before generating navigation".into());
        }
        let engine = self.plan_engine(id)?;
        let live_state = self.plan_state(id);
        let precise = self.fleet.precise_snapshot(id);
        let (system, origin) = self
            .plans
            .get(id)
            .map(|p| (p.system, p.origin))
            .unwrap_or((precise.system, precise.anchor));
        let mut view = self.plan_view(id);
        let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
            Some(view) => view.as_mut(),
            None => self.fleet.ephemeris.as_mut(),
        };
        let mut candidate = if let Some(existing) = self.plans.get(id) {
            FlightPlan::from_checkpoint(source, existing.plan.checkpoint())
        } else {
            FlightPlan::new(source, self.fleet.options.tolerances, engine, 6000.0)
        };
        candidate.set_engine(engine);
        candidate.rebase(&live_state);
        let anchor = candidate.tail_state(source)?;
        if request.earliest_departure < anchor.time {
            return Err(format!(
                "Earliest departure must be after existing burns end at T+{:.1}",
                anchor.time
            ));
        }
        let solution = void_orbit::solve_navigation(
            source,
            &anchor,
            engine,
            self.fleet.options.tolerances,
            request,
        )
        .map_err(|reason| reason.to_string())?;
        let selected = candidate.add(solution.maneuver);
        candidate.status(selected).as_ref().map_err(Clone::clone)?;
        let last_end = candidate
            .burns()
            .last()
            .expect("navigation adds a valid burn")
            .end_time;
        candidate.set_coast_seconds(
            candidate
                .coast_seconds()
                .max(solution.verified_until - last_end),
        );
        // Generate a reviewable trajectory even while the game is paused. The solver
        // verified its trial; now verify the actual appended plan from the live anchor.
        candidate.extend(source, 2_000_000);
        if let Some(impact) = candidate.impact() {
            return Err(format!(
                "Appended navigation plan impacts body {} at T+{:.1}",
                impact.body, impact.time
            ));
        }
        if !candidate.complete() {
            return Err("Appended navigation plan exhausted its prediction budget".into());
        }
        let operation = match request.operation {
            void_orbit::NavigationOperation::Departure => "Departure",
            void_orbit::NavigationOperation::Correction => "Correction",
            void_orbit::NavigationOperation::Capture => "Capture",
        };
        let message = format!(
            "{operation} node {} appended: ignition T+{:.1}, Δv {:.1} m/s; predicted closest T+{:.1}, distance {:.1} km, altitude {:.1} km, relative speed {:.1} m/s{}",
            selected + 1,
            solution.maneuver.start_time,
            solution.delta_v_mps,
            solution.closest_time,
            solution.closest_distance_m / 1000.0,
            solution.periapsis_altitude_m / 1000.0,
            solution.relative_speed_mps,
            if solution.captured {
                "; bound orbit verified"
            } else {
                ""
            },
        );
        self.cancel_maneuver_warp("navigation node generated");
        self.plans.insert(
            id.into(),
            VesselPlan {
                system,
                origin,
                plan: candidate,
                selected,
                executing: false,
                message,
            },
        );
        Ok(())
    }
    pub fn add_maneuver(&mut self, id: &str, spec: ManeuverSpec) -> Result<(), String> {
        self.cancel_maneuver_warp("maneuver edited");
        self.refresh_plan(id)?;
        let p = self.plans.get_mut(id).unwrap();
        p.selected = p.plan.add(spec);
        Ok(())
    }
    pub fn edit_maneuver(
        &mut self,
        id: &str,
        index: usize,
        spec: ManeuverSpec,
    ) -> Result<(), String> {
        self.cancel_maneuver_warp("maneuver edited");
        self.refresh_plan(id)?;
        let p = self.plans.get_mut(id).unwrap();
        p.plan.replace(index, spec);
        p.selected = index;
        Ok(())
    }
    pub fn remove_maneuver(&mut self, id: &str, index: usize) -> Result<(), String> {
        self.cancel_maneuver_warp("maneuver edited");
        self.refresh_plan(id)?;
        let p = self.plans.get_mut(id).unwrap();
        p.plan.remove(index);
        p.selected = p.selected.min(p.plan.count().saturating_sub(1));
        Ok(())
    }
    pub fn place_maneuver_at_apsis(
        &mut self,
        id: &str,
        index: usize,
        kind: void_orbit::ApsisKind,
    ) -> Result<(), String> {
        self.cancel_maneuver_warp("maneuver edited");
        self.refresh_plan(id)?;
        let time = self.fleet.time();
        let mut view = self.plan_view(id);
        let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
            Some(view) => view.as_mut(),
            None => self.fleet.ephemeris.as_mut(),
        };
        let p = self.plans.get_mut(id).unwrap();
        let start = p.plan.start_at_apsis(source, index, kind, time)?;
        let mut spec = p.plan.maneuver(index);
        spec.start_time = start;
        p.plan.replace(index, spec);
        p.selected = index;
        Ok(())
    }
    pub fn select_maneuver(&mut self, id: &str, index: usize) {
        let p = self.plans.get_mut(id).expect("no plan on vessel");
        assert!(index < p.plan.count(), "no maneuver at index");
        p.selected = index;
    }
    pub fn execute_maneuver(&mut self, id: &str) -> Result<(), String> {
        self.cancel_maneuver_warp("maneuver armed");
        self.refresh_plan(id)?;
        // Auto references follow the predicted ignition point, not today's position.
        let mut view = self.plan_view(id);
        let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
            Some(view) => view.as_mut(),
            None => self.fleet.ephemeris.as_mut(),
        };
        let count = self.plans[id].plan.count();
        for i in 0..count {
            let mut spec = self.plans[id].plan.maneuver(i);
            if spec.reference_mode == ReferenceMode::Auto {
                let p = self.plans.get_mut(id).unwrap();
                let position = p
                    .plan
                    .position_at(source, spec.start_time)
                    .ok_or("planned ignition is beyond an impact")?;
                let mut positions = vec![glam::DVec3::ZERO; source.bodies().len()];
                source.positions_at(spec.start_time, &mut positions);
                spec.reference_body =
                    void_orbit::DominanceTree::new(source.bodies()).dominant(&positions, position);
                p.plan.replace(i, spec);
            }
        }
        let p = self.plans.get_mut(id).unwrap();
        if p.plan.count() == 0 {
            return Err("No maneuver to execute".into());
        }
        let burn = *p.plan.status(0).as_ref().map_err(Clone::clone)?;
        let Some(Control::Thrust(control)) = burn.control else {
            return Err("A zero-delta-v maneuver has no burn to execute".into());
        };
        self.fleet
            .arm_guided_burn(id, burn.start_time, burn.end_time, control.attitude)?;
        p.executing = true;
        p.message =
            "First maneuver armed; subsequent maneuvers require another execute command".into();
        Ok(())
    }
    pub fn abort_maneuver(&mut self, id: &str) {
        self.cancel_maneuver_warp("pilot aborted maneuver");
        self.fleet.cancel_guidance(id, "pilot aborted maneuver");
        self.update_plans();
    }
    pub(crate) fn update_plans(&mut self) {
        let ids = self.plans.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            // A join can remove a logical vessel. The graph changes invalidate its plan explicitly.
            if !self.fleet.vessel_ids().contains(&id) {
                self.plans.remove(&id);
                continue;
            }
            let state = self.plan_state(&id);
            let mut view = self.plan_view(&id);
            let p = self.plans.get_mut(&id).unwrap();
            if p.executing {
                match &self
                    .fleet
                    .guidance(&id)
                    .expect("executing plan lost guidance")
                    .status
                {
                    GuidanceStatus::Armed => {}
                    GuidanceStatus::Completed => {
                        p.plan.complete_first(&state);
                        p.selected = p.selected.saturating_sub(1);
                        p.executing = false;
                        p.message = "Maneuver complete".into();
                    }
                    GuidanceStatus::Aborted(reason) => {
                        p.executing = false;
                        p.message = format!("Maneuver aborted: {reason}");
                        p.plan.rebase(&state);
                    }
                }
            }
            let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
                Some(view) => view.as_mut(),
                None => self.fleet.ephemeris.as_mut(),
            };
            p.plan.extend(source, 256);
        }
    }
}

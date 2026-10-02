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
    pub plan: FlightPlan,
    pub selected: usize,
    pub executing: bool,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedVesselPlan {
    plan: FlightPlanCheckpoint,
    selected: usize,
    executing: bool,
    message: String,
}
impl FleetFlight {
    pub fn plan_checkpoints(&self) -> BTreeMap<String, SavedVesselPlan> {
        self.plans
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    SavedVesselPlan {
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
            let plan = FlightPlan::from_checkpoint(&self.fleet.ephemeris, p.plan);
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
                    plan,
                    selected: p.selected,
                    executing: p.executing,
                    message: p.message,
                },
            );
        }
    }
    fn plan_state(&self, id: &str) -> PropagationRun {
        let ship = self.fleet.snapshot(id);
        PropagationRun::new(VesselState {
            time: self.fleet.time(),
            position: ship.position,
            velocity: ship.velocity,
            mass_kg: ship.mass_kg,
        })
    }
    fn refresh_plan(&mut self, id: &str) -> Result<(), String> {
        if self.plans.get(id).is_some_and(|p| p.executing) {
            return Err("Abort the executing maneuver before editing".into());
        }
        let engine = self.plan_engine(id)?;
        let state = self.plan_state(id);
        if let Some(p) = self.plans.get_mut(id) {
            p.plan.set_engine(engine);
            p.plan.rebase(&state);
        } else {
            self.plans.insert(
                id.into(),
                VesselPlan {
                    plan: self.new_plan(id, 6000.0)?,
                    selected: 0,
                    executing: false,
                    message: String::new(),
                },
            );
        }
        Ok(())
    }
    pub fn add_maneuver(&mut self, id: &str, spec: ManeuverSpec) -> Result<(), String> {
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
        self.refresh_plan(id)?;
        let p = self.plans.get_mut(id).unwrap();
        p.plan.replace(index, spec);
        p.selected = index;
        Ok(())
    }
    pub fn remove_maneuver(&mut self, id: &str, index: usize) -> Result<(), String> {
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
        self.refresh_plan(id)?;
        let time = self.fleet.time();
        let p = self.plans.get_mut(id).unwrap();
        let start = p
            .plan
            .start_at_apsis(&mut self.fleet.ephemeris, index, kind, time)?;
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
        self.refresh_plan(id)?;
        // Auto references follow the predicted ignition point, not today's position.
        let count = self.plans[id].plan.count();
        for i in 0..count {
            let mut spec = self.plans[id].plan.maneuver(i);
            if spec.reference_mode == ReferenceMode::Auto {
                let p = self.plans.get_mut(id).unwrap();
                let position = p
                    .plan
                    .position_at(&mut self.fleet.ephemeris, spec.start_time)
                    .ok_or("planned ignition is beyond an impact")?;
                let mut positions = vec![glam::DVec3::ZERO; self.fleet.ephemeris.bodies().len()];
                self.fleet
                    .ephemeris
                    .positions_at(spec.start_time, &mut positions);
                spec.reference_body = void_orbit::DominanceTree::new(self.fleet.ephemeris.bodies())
                    .dominant(&positions, position);
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
            p.plan.extend(&mut self.fleet.ephemeris, 256);
        }
    }
}

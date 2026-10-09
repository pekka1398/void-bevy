//! Isolated navigation work and replayable, conditional publication of its result.
//! A worker restores a snapshot; only the verified plan can return to the live world.
use crate::{
    FleetFlight, checkpoint::FlightCheckpoint, plans::SavedVesselPlan, session::world_mark,
};
use serde::{Deserialize, Serialize};
use void_orbit::NavigationRequest;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationJob {
    pub checkpoint: FlightCheckpoint,
    pub request: NavigationRequest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationResult {
    pub vessel: String,
    pub baseline: serde_json::Value,
    pub plan: SavedVesselPlan,
    pub prediction_until: f64,
}
/// Presentation changes (camera, pause, plot frame) do not invalidate a physical trial.
pub fn baseline(sim: &FleetFlight) -> serde_json::Value {
    let mut mark = world_mark(sim);
    mark.as_object_mut().unwrap().remove("presentation");
    mark
}
impl NavigationJob {
    pub fn solve(self) -> Result<NavigationResult, String> {
        let mut sim = self.checkpoint.restore();
        let baseline = baseline(&sim);
        let vessel = sim.selected.clone();
        let mut request = self.request;
        request.reference_body = sim.navigation_reference(&vessel)?;
        sim.generate_navigation(&vessel, &request)?;
        let prediction_until = sim.plans[&vessel].plan.trajectory.last_time();
        let plan = sim
            .plan_checkpoints()
            .remove(&vessel)
            .expect("generated plan");
        Ok(NavigationResult {
            vessel,
            baseline,
            plan,
            prediction_until,
        })
    }
}
impl FleetFlight {
    pub fn accept_navigation(&mut self, result: &NavigationResult) -> Result<(), String> {
        if self.selected != result.vessel || baseline(self) != result.baseline {
            return Err(
                "Navigation result expired: vessel, controls or plan changed; generate again"
                    .into(),
            );
        }
        assert!(
            result.prediction_until.is_finite() && result.prediction_until >= self.fleet.time(),
            "invalid navigation prediction horizon"
        );
        // GUI warms this cache in bounded slices before publishing. Headless replay
        // restores the same ordinary ephemeris without rerunning the search.
        self.fleet.ephemeris.extend_to(result.prediction_until);
        self.install_navigation_plan(&result.vessel, result.plan.clone());
        Ok(())
    }
}

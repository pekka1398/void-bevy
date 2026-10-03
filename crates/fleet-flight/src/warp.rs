//! Maneuver approach intent, shared by the window, headless replay and direct checkpoints.
use crate::FleetFlight;
use serde::{Deserialize, Serialize};
use void_vessels::VesselMode;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "state", deny_unknown_fields)]
pub enum ManeuverWarp {
    #[default]
    Idle,
    Coasting {
        vessel: String,
        ignition_time: f64,
        stop_time: f64,
    },
    Stopped {
        message: String,
    },
}
impl ManeuverWarp {
    pub fn active(&self) -> bool {
        matches!(self, Self::Coasting { .. })
    }
}
impl FleetFlight {
    pub fn begin_maneuver_warp(&mut self, id: &str) -> Result<(), String> {
        if self.fleet.snapshot(id).mode != VesselMode::Orbit {
            return Err("maneuver warp requires orbital ownership".into());
        }
        if let Some(reason) = self.fleet.rails_blocker() {
            return Err(reason);
        }
        self.refresh_plan(id)?;
        let burn = self.plans[id]
            .plan
            .burns()
            .first()
            .ok_or("no future executable maneuver")?;
        let stop_time = burn.start_time - 30.0;
        if stop_time <= self.fleet.time() + self.fleet.pending_seconds() {
            return Err("maneuver must be more than 30 seconds away".into());
        }
        self.maneuver_warp = ManeuverWarp::Coasting {
            vessel: id.into(),
            ignition_time: burn.start_time,
            stop_time,
        };
        Ok(())
    }
    pub fn cancel_maneuver_warp(&mut self, message: &str) {
        if self.maneuver_warp.active() {
            self.maneuver_warp = ManeuverWarp::Stopped {
                message: message.into(),
            };
        }
    }
    pub(crate) fn warp_duration(&mut self, requested: f64) -> Result<f64, String> {
        let ManeuverWarp::Coasting {
            vessel,
            ignition_time,
            stop_time,
        } = self.maneuver_warp.clone()
        else {
            return Ok(requested);
        };
        let valid = self.fleet.vessel_ids().contains(&vessel)
            && self.plans.get(&vessel).is_some_and(|p| {
                !p.executing
                    && p.plan
                        .burns()
                        .first()
                        .is_some_and(|burn| burn.start_time == ignition_time)
            });
        if !valid {
            self.cancel_maneuver_warp("maneuver changed or vessel removed");
            return Err("maneuver changed or vessel removed".into());
        }
        if self.fleet.snapshot(&vessel).mode != VesselMode::Orbit {
            self.cancel_maneuver_warp("warp target entered contact physics");
            return Err("warp target entered contact physics".into());
        }
        if let Some(reason) = self.fleet.rails_blocker() {
            self.cancel_maneuver_warp(&reason);
            return Err(reason);
        }
        Ok(requested.min((stop_time - self.fleet.time() - self.fleet.pending_seconds()).max(0.0)))
    }
    pub(crate) fn finish_warp_step(&mut self, advanced: bool) {
        let ManeuverWarp::Coasting { stop_time, .. } = self.maneuver_warp else {
            return;
        };
        if !advanced {
            self.cancel_maneuver_warp("stopped at an encounter or ground band");
        } else if self.fleet.time() + self.fleet.pending_seconds() >= stop_time - 1e-9 {
            self.cancel_maneuver_warp("30 seconds before maneuver; physics time resumed");
        }
    }
    pub(crate) fn validate_warp(&self) {
        if let ManeuverWarp::Coasting {
            ref vessel,
            ignition_time,
            stop_time,
        } = self.maneuver_warp
        {
            assert!(
                ignition_time.is_finite()
                    && stop_time.is_finite()
                    && stop_time == ignition_time - 30.0
                    && stop_time > self.fleet.time() + self.fleet.pending_seconds(),
                "checkpoint: invalid maneuver warp clock"
            );
            self.fleet.snapshot(vessel);
            assert!(
                self.plans.get(vessel).is_some_and(|p| !p.executing
                    && p.plan
                        .burns()
                        .first()
                        .is_some_and(|burn| burn.start_time == ignition_time)),
                "checkpoint: maneuver warp has no matching burn"
            );
        }
    }
}

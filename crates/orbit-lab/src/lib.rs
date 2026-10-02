//! The standalone orbit lab's controls and plotted scene, independent of Bevy's rendering.
pub mod scene;

use glam::DVec3;
use void_orbit::*;

pub const DAY: f64 = 86400.0;
pub const WARPS: [f64; 8] = [1.0, 10.0, 100.0, 1e3, 1e4, 1e5, 1e6, 1e7];
pub const TRAIL_SPANS: [f64; 5] = [DAY, 7.0 * DAY, 30.0 * DAY, 90.0 * DAY, 365.25 * DAY];
pub const VESSEL_SPANS: [f64; 5] = [3600.0, 21600.0, DAY, 7.0 * DAY, 30.0 * DAY];
pub const PREDICTION_SPANS: [f64; 7] = [
    10800.0,
    43200.0,
    DAY,
    7.0 * DAY,
    30.0 * DAY,
    90.0 * DAY,
    365.25 * DAY,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPreset {
    Sol,
    Binary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Vessel,
    Body(usize),
}

pub struct OrbitLab {
    pub system: SystemPreset,
    pub sim: Simulation,
    pub frame: FrameSpec,
    pub focus: Focus,
    pub home: usize,
    pub target: Option<usize>,
    pub trail_span: f64,
    pub vessel_span: f64,
    pub selected_burn: Option<usize>,
    pub warp: usize,
    pub paused: bool,
    pub warp_target: Option<f64>,
    pub last_report: AdvanceReport,
    pub achieved_warp: f64,
    seen_completed: u64,
}
impl OrbitLab {
    pub fn new(system: SystemPreset) -> Self {
        let (json, home, moon) = match system {
            SystemPreset::Sol => (
                include_str!("../../orbit/systems/sol.json"),
                "aurelia",
                "selene",
            ),
            SystemPreset::Binary => (
                include_str!("../../orbit/systems/binary.json"),
                "aurelia-veil",
                "lumen",
            ),
        };
        let sim = Simulation::new(SimulationOptions {
            system: SystemSpec::from_json(json),
            steps_per_orbit: 256.0,
            tolerances: Tolerances {
                position_meters: 1e-4,
                velocity_meters_per_second: 1e-7,
            },
            vessel_start: VesselStartSpec {
                home_body_id: home.into(),
                altitude_meters: 400e3,
                plane: StartPlane::OrbitOf {
                    body_id: moon.into(),
                },
            },
            engine: EngineSpec {
                thrust_newtons: 250e3,
                specific_impulse_seconds: 350.0,
                dry_mass_kg: 10e3,
                fuel_mass_kg: 30e3,
            },
            retention_seconds: 30.0 * DAY + DAY,
            prediction_horizon_seconds: 43200.0,
            plan_coast_seconds: 7.0 * DAY,
        });
        let home = sim.body_index(home);
        let target = Some(sim.body_index(moon));
        Self {
            system,
            sim,
            frame: FrameSpec::BodyInertial { body: home },
            focus: Focus::Body(home),
            home,
            target,
            trail_span: 30.0 * DAY,
            vessel_span: 21600.0,
            selected_burn: None,
            warp: 2,
            paused: false,
            warp_target: None,
            last_report: AdvanceReport {
                completed: true,
                steps: 0,
                thrusted: false,
            },
            achieved_warp: 0.0,
            seen_completed: 0,
        }
    }
    pub fn set_spans(&mut self, trail: f64, vessel: f64) {
        assert!(trail > 0.0 && trail.is_finite() && vessel > 0.0 && vessel.is_finite());
        self.trail_span = trail;
        self.vessel_span = vessel;
        self.sim.set_retention_seconds(trail.max(vessel) + DAY);
    }
    pub fn set_frame(&mut self, frame: FrameSpec) {
        frame.assert_valid(self.sim.system.bodies.len());
        self.frame = frame;
    }
    pub fn set_focus(&mut self, focus: Focus) {
        if let Focus::Body(i) = focus {
            assert!(i < self.sim.system.bodies.len());
        }
        self.focus = focus;
    }
    pub fn start_planes(&self) -> Vec<StartPlane> {
        let mut planes = vec![StartPlane::Equatorial {
            inclination_radians: 0.0,
        }];
        planes.extend(
            self.sim
                .system
                .bodies
                .iter()
                .filter(|b| b.parent_index == Some(self.home))
                .map(|b| StartPlane::OrbitOf {
                    body_id: b.id.clone(),
                }),
        );
        planes
    }
    pub fn reset_vessel(&mut self) {
        self.sim.reset_vessel();
        self.selected_burn = None;
        self.warp_target = None;
        self.seen_completed = self.sim.plan.completed_count;
    }
    pub fn set_start_plane(&mut self, plane: StartPlane) {
        let spec = VesselStartSpec {
            plane,
            ..self.sim.vessel_start()
        };
        self.sim.set_vessel_start(spec);
        self.reset_vessel();
    }
    pub fn editable(&self) -> bool {
        self.selected_burn.is_some()
            && self.sim.impact.is_none()
            && !(self.selected_burn == Some(0) && self.sim.executing_burn().is_some())
    }
    pub fn add_burn(&mut self) -> Result<(), String> {
        if self.sim.impact.is_some() {
            return Err("Cannot plan after impact".into());
        }
        let start = self
            .sim
            .plan
            .burns()
            .last()
            .map_or(self.sim.time, |b| b.end_time)
            .max(self.sim.time)
            + 600.0;
        self.selected_burn = Some(self.sim.add_maneuver(ManeuverSpec {
            start_time: start,
            reference_body: self.sim.navigation_reference(),
            reference_mode: ReferenceMode::Auto,
            prograde: 0.0,
            normal: 0.0,
            radial: 0.0,
        }));
        Ok(())
    }
    pub fn edit_burn(&mut self, change: impl FnOnce(&mut ManeuverSpec)) -> Result<(), String> {
        if !self.editable() {
            return Err("Select an editable burn".into());
        }
        let i = self.selected_burn.unwrap();
        let mut spec = self.sim.plan.maneuver(i);
        change(&mut spec);
        self.sim.replace_maneuver(i, spec);
        Ok(())
    }
    pub fn remove_burn(&mut self) -> Result<(), String> {
        if !self.editable() {
            return Err("Select an editable burn".into());
        }
        let i = self.selected_burn.unwrap();
        self.sim.remove_maneuver(i);
        self.selected_burn = (self.sim.plan.count() > 0).then(|| i.min(self.sim.plan.count() - 1));
        Ok(())
    }
    pub fn snap_burn(&mut self, kind: ApsisKind) -> Result<(), String> {
        if !self.editable() {
            return Err("Select an editable burn".into());
        }
        self.sim
            .place_maneuver_at_apsis(self.selected_burn.unwrap(), kind)
            .map(|_| ())
    }
    pub fn warp_to_burn(&mut self) -> Result<(), String> {
        let Some(b) = self.sim.plan.burns().first() else {
            return Err("No executable burn".into());
        };
        if b.start_time <= self.sim.time + 30.0 {
            return Err("Burn is less than 30 s away".into());
        }
        self.warp_target = Some(b.start_time - 30.0);
        self.paused = false;
        Ok(())
    }
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        self.warp_target = None;
    }
    pub fn tick(&mut self, wall: f64) {
        assert!(wall >= 0.0 && wall.is_finite());
        let before = self.sim.time;
        let warp = if self.warp_target.is_some() {
            WARPS[7]
        } else {
            WARPS[self.warp]
        };
        let dt = self
            .warp_target
            .map_or(wall * warp, |t| (wall * warp).min(t - self.sim.time));
        if !self.paused && dt > 0.0 {
            self.last_report = self.sim.advance(dt, 20000);
        }
        if self.warp_target.is_some_and(|t| self.sim.time >= t) {
            self.warp_target = None;
            self.warp = 0;
        }
        let flown = self.sim.plan.completed_count - self.seen_completed;
        self.seen_completed = self.sim.plan.completed_count;
        self.selected_burn = self
            .selected_burn
            .and_then(|i| i.checked_sub(flown as usize))
            .filter(|&i| i < self.sim.plan.count());
        self.sim.extend_prediction(4000);
        self.sim.extend_plan(4000);
        self.achieved_warp = if wall > 0.0 {
            (self.sim.time - before) / wall
        } else {
            0.0
        };
    }
    pub fn focus_position(&self) -> DVec3 {
        match self.focus {
            Focus::Vessel => self.sim.vessel_position_at(self.sim.time),
            Focus::Body(i) => self.sim.ephemeris.body_position(i, self.sim.time),
        }
    }
}

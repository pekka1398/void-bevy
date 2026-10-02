//! First integration boundary for assembly/Fleet flight. No Bevy and no fixed two-stage rocket.
mod air;
pub mod checkpoint;
pub mod session;
pub use air::FleetAir;
use glam::{DQuat, DVec3};
use std::sync::Arc;
use void_assembly::Craft;
use void_landing::{
    CoastPrediction, ContactWorldOptions, FrameState, LandingPlanet, PlanetFrame,
    level_for_tile_size, planet_ephemeris, predict_coast,
};
use void_vessels::{Fleet, FleetOptions, GroundSpec, VesselControl, VesselMode};

pub struct FleetFlight {
    pub fleet: Fleet,
    pub planet: LandingPlanet,
    pub home: usize,
    pub selected: String,
    pub launch_site: DVec3,
}
impl FleetFlight {
    pub fn new(planet: LandingPlanet, craft: &Craft, site: DVec3, air: bool) -> Self {
        let (ephemeris, home) = planet_ephemeris(&planet);
        let ground = GroundSpec {
            body_index: home,
            terrain: planet.terrain.clone(),
            band_enter_meters: 200.0,
            band_exit_meters: 400.0,
            tiles: ContactWorldOptions {
                step_seconds: 1.0 / 60.0,
                tile_level: level_for_tile_size(planet.terrain.radius_meters, 300.0),
                tile_resolution: 33,
                tile_reach_meters: 300.0,
                tile_keep_meters: 600.0,
                recenter_meters: 5000.0,
                sleeping: true,
            },
        };
        let mut fleet = Fleet::new(ephemeris, 0.0, vec![ground], FleetOptions::default());
        if air {
            let scale = planet
                .air_density_scale
                .expect("fleet flight: requested air on an airless planet");
            fleet.set_environment(Some(Arc::new(FleetAir::earth(home, scale))));
        }
        let selected = fleet.launch_landed(craft, home, site);
        fleet.advance(0.0);
        Self {
            fleet,
            planet,
            home,
            selected,
            launch_site: site,
        }
    }
    pub fn select(&mut self, id: &str) {
        self.fleet.snapshot(id); // Unknown vessels are errors.
        self.selected = id.into();
    }
    pub fn control(&mut self, control: VesselControl) {
        self.fleet.set_control(&self.selected, control);
    }
    pub fn stage(&mut self) -> Vec<String> {
        let children = self.fleet.stage(&self.selected);
        self.fleet.advance(0.0);
        children
    }
    pub fn sas(&mut self, enabled: bool) {
        self.fleet.set_sas(&self.selected, enabled);
    }
    /// High warp is coast-only. A rejected request is explicit; the UI may tell the pilot why.
    pub fn advance(&mut self, seconds: f64, rails: bool) -> Result<bool, String> {
        if rails {
            if let Some(reason) = self.fleet.rails_blocker() {
                return Err(reason);
            }
            Ok(self.fleet.advance_on_rails(seconds))
        } else {
            self.fleet.advance(seconds);
            Ok(true)
        }
    }
    /// Vacuum coast, as the current game's cyan line. No engine or atmosphere in the prediction.
    pub fn predict(&mut self, horizon: f64) -> CoastPrediction {
        let snap = self.fleet.snapshot(&self.selected);
        let frame = PlanetFrame::new(&self.fleet.ephemeris, self.home);
        let state = frame.to_body_fixed(
            &self.fleet.ephemeris,
            self.fleet.time(),
            FrameState {
                position: snap.position,
                velocity: snap.velocity,
            },
        );
        let time = self.fleet.time();
        predict_coast(
            &mut self.fleet.ephemeris,
            &frame,
            &self.planet.terrain,
            self.fleet.options.tolerances,
            time,
            state,
            snap.mass_kg,
            horizon,
        )
    }
    /// A repeatable orbital fixture for checking scale, warp and multi-vessel flight without launch.
    pub fn launch_orbital(&mut self, craft: &Craft, offset: DVec3) -> String {
        let frame = PlanetFrame::new(&self.fleet.ephemeris, self.home);
        let body = &frame.body;
        let r = body.radius_meters + 400_000.0;
        let local = FrameState {
            position: DVec3::X * r + offset,
            velocity: DVec3::Y * ((body.gm / r).sqrt() - frame.omega * r),
        };
        let state = frame.to_inertial(&self.fleet.ephemeris, self.fleet.time(), local);
        let a = void_orbit::body_orientation(&body.rotation, self.fleet.time());
        let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(a[0], a[1], a[2])).normalize();
        let id = self.fleet.launch(craft, state, rotation, DVec3::ZERO);
        self.fleet.advance(0.0);
        id
    }
    pub fn mode(&self) -> VesselMode {
        self.fleet.snapshot(&self.selected).mode
    }
    /// A plan's constant engine ends at the first fuel-group flameout. Later groups can change
    /// effective Isp or thrust, so a single PlanEngine must not promise the whole tank inventory.
    pub fn plan_engine(&self, vessel: &str) -> Result<void_orbit::PlanEngine, String> {
        if self.fleet.snapshot(vessel).mode == VesselMode::Ground {
            return Err("Reach free flight before planning a maneuver".into());
        }
        let p = self.fleet.full_throttle_vacuum_thrust(vessel);
        let thrust = p.force.length();
        if thrust == 0.0 || p.flow_kg_per_second == 0.0 {
            return Err("No staged engine with accessible fuel".into());
        }
        if p.torque.length() > 1e-6 {
            return Err(
                "Active engines have unbalanced torque; constant-engine planning is invalid".into(),
            );
        }
        let mass = self.fleet.snapshot(vessel).mass_kg;
        Ok(void_orbit::PlanEngine {
            thrust_newtons: thrust,
            exhaust_velocity: thrust / p.flow_kg_per_second,
            dry_mass_kg: mass - p.flow_kg_per_second * p.seconds_to_flameout,
        })
    }
    pub fn new_plan(
        &self,
        vessel: &str,
        coast_seconds: f64,
    ) -> Result<void_orbit::FlightPlan, String> {
        let engine = self.plan_engine(vessel)?;
        let snapshot = self.fleet.snapshot(vessel);
        let mut plan = void_orbit::FlightPlan::new(
            &self.fleet.ephemeris,
            self.fleet.options.tolerances,
            engine,
            coast_seconds,
        );
        plan.rebase(&void_orbit::PropagationRun::new(void_orbit::VesselState {
            time: self.fleet.time(),
            position: snapshot.position,
            velocity: snapshot.velocity,
            mass_kg: snapshot.mass_kg,
        }));
        Ok(plan)
    }
}

//! First integration boundary for assembly/Fleet flight. No Bevy and no fixed two-stage rocket.
mod air;
pub mod checkpoint;
pub mod plans;
pub mod presentation;
pub mod session;
pub mod warp;
pub use air::FleetAir;
use glam::DVec3;
use std::sync::Arc;
use void_assembly::Craft;
use void_environment::{Atmosphere, BodyEnvironment, EarthAtmosphere, Environment};
use void_landing::{
    CoastPrediction, ContactWorldOptions, FrameState, LandingPlanet, PlanetFrame,
    level_for_tile_size, planet_ephemeris, predict_coast,
};
use void_orbit::EphemerisSource;
use void_vessels::{Fleet, FleetOptions, GroundSpec, VesselControl, VesselMode};

/// The world's environment: every body's gravity, and the home planet's terrain and, when the
/// flight simulates air, its atmosphere (altitude from the terrain's reference sphere).
pub fn world_environment(
    planet: &LandingPlanet,
    ephemeris: &dyn EphemerisSource,
    home: usize,
    air: bool,
) -> Arc<Environment> {
    let atmosphere = air.then(|| {
        let scale = planet
            .air_density_scale
            .expect("fleet flight: requested air on an airless planet");
        Atmosphere::Earth(EarthAtmosphere::new(scale))
    });
    Arc::new(Environment::new(ephemeris).with(
        home,
        BodyEnvironment {
            atmosphere,
            air_datum_meters: 0.0,
            terrain: Some(planet.terrain.clone()),
            sea_level_meters: None,
        },
    ))
}

pub struct FleetFlight {
    pub presentation: presentation::Presentation,
    pub fleet: Fleet,
    pub planet: LandingPlanet,
    pub home: usize,
    pub selected: String,
    pub launch_site: DVec3,
    pub maneuver_warp: warp::ManeuverWarp,
    pub plans: std::collections::BTreeMap<String, plans::VesselPlan>,
}
impl FleetFlight {
    pub fn new(planet: LandingPlanet, craft: &Craft, site: DVec3, air: bool) -> Self {
        let (ephemeris, home) = planet_ephemeris(&planet);
        let environment = world_environment(&planet, &ephemeris, home, air);
        let ground = GroundSpec {
            body_index: home,
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
        let mut fleet = Fleet::new(
            ephemeris,
            environment,
            0.0,
            vec![ground],
            FleetOptions::default(),
        );
        if air {
            fleet.set_forces(Some(Arc::new(FleetAir::new(home))));
        }
        let selected = fleet.launch_landed(craft, home, site);
        fleet.advance(0.0);
        let ship = fleet.snapshot(&selected);
        let presentation = presentation::Presentation::new(
            ship.position,
            fleet.ephemeris.body_position(home, fleet.time()),
            fleet.time(),
        );
        Self {
            presentation,
            fleet,
            planet,
            home,
            selected,
            launch_site: site,
            plans: std::collections::BTreeMap::new(),
            maneuver_warp: warp::ManeuverWarp::Idle,
        }
    }
    pub fn select(&mut self, id: &str) {
        self.fleet.snapshot(id); // Unknown vessels are errors.
        self.selected = id.into();
    }
    pub fn control(&mut self, control: VesselControl) {
        self.cancel_maneuver_warp("manual control");
        self.fleet.set_control(&self.selected, control);
        self.update_plans();
    }
    pub fn stage(&mut self) -> Vec<String> {
        self.cancel_maneuver_warp("staging");
        let children = self.fleet.stage(&self.selected);
        self.fleet.advance(0.0);
        self.update_plans();
        children
    }
    pub fn sas(&mut self, enabled: bool) {
        self.fleet.set_sas(&self.selected, enabled);
    }
    /// High warp is coast-only. A rejected request is explicit; the UI may tell the pilot why.
    pub fn advance(&mut self, seconds: f64, rails: bool) -> Result<bool, String> {
        assert!(
            seconds.is_finite() && seconds >= 0.0,
            "flight: invalid duration"
        );
        let seconds = self.warp_duration(seconds)?;
        let done = if rails {
            if let Some(reason) = self.fleet.rails_blocker() {
                return Err(reason);
            }
            self.fleet.advance_on_rails(seconds)
        } else {
            self.fleet.advance(seconds);
            true
        };
        self.finish_warp_step(done);
        self.update_plans();
        Ok(done)
    }
    /// Vacuum coast, as the current game's cyan line. No engine or atmosphere in the prediction.
    pub fn predict(&mut self, horizon: f64) -> CoastPrediction {
        let snap = self.fleet.snapshot(&self.selected);
        let frame = PlanetFrame::new(&self.fleet.ephemeris, self.home);
        let state = self.body_fixed(
            self.home,
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
        let ground = self.fleet.frames().transform(
            self.fleet.body_frames(self.home).1,
            self.fleet.origin_frame(),
        );
        let state = ground.apply_state(void_frames::State {
            position: local.position,
            velocity: local.velocity,
        });
        let state = FrameState {
            position: state.position,
            velocity: state.velocity,
        };
        let rotation = ground.rotation();
        let id = self.fleet.launch(craft, state, rotation, DVec3::ZERO);
        self.fleet.advance(0.0);
        id
    }
    /// An origin-frame state in a body's surface (body-fixed) frame.
    pub fn body_fixed(&self, body: usize, inertial: FrameState) -> FrameState {
        let s = self
            .fleet
            .frames()
            .transform(self.fleet.origin_frame(), self.fleet.body_frames(body).1)
            .apply_state(void_frames::State {
                position: inertial.position,
                velocity: inertial.velocity,
            });
        FrameState {
            position: s.position,
            velocity: s.velocity,
        }
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

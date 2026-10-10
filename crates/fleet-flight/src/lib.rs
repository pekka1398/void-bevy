//! First integration boundary for assembly/Fleet flight. No Bevy and no fixed two-stage rocket.
pub mod checkpoint;
pub mod placement;
pub mod plans;
pub mod presentation;
pub mod session;
pub mod warp;
pub mod world;
use glam::DVec3;
use void_assembly::Craft;
use void_frames::State;
use void_landing::{CoastPrediction, LandingPlanet, PlanetFrame, predict_coast};
use void_vessels::{Fleet, FleetOptions, VesselControl, VesselMode};

pub struct FleetFlight {
    pub presentation: presentation::Presentation,
    pub fleet: Fleet,
    pub world: world::WorldDescription,
    pub coupled_world: Option<void_multiscale::SharedWorld>,
    pub terrains: std::collections::BTreeMap<usize, std::sync::Arc<void_terrain::Terrain>>,
    pub planet: LandingPlanet,
    pub home: usize,
    pub selected: String,
    pub launch_site: DVec3,
    pub maneuver_warp: warp::ManeuverWarp,
    pub plans: std::collections::BTreeMap<String, plans::VesselPlan>,
}
impl FleetFlight {
    pub fn new(planet: LandingPlanet, craft: &Craft, site: DVec3, air: bool) -> Self {
        Self::from_world(
            world::WorldDescription::single(&planet, air),
            &planet.body_id,
            craft,
            site,
        )
    }
    pub fn from_world(
        world: world::WorldDescription,
        launch_body: &str,
        craft: &Craft,
        site: DVec3,
    ) -> Self {
        world.validate_launch(launch_body, site);
        let planet = world.landing_planet(launch_body);
        let home = world.body_index(launch_body);
        let built = world.build();
        let terrains = built.terrains;
        let mut fleet = Fleet::new(
            built.ephemeris,
            built.environment,
            0.0,
            built.grounds,
            FleetOptions::default(),
        );
        let selected = fleet.launch_landed(craft, home, site);
        fleet.advance(0.0);
        let ship = fleet.snapshot(&selected);
        let mut presentation = presentation::Presentation::new(
            ship.position,
            fleet.ephemeris.body_position(home, fleet.time()),
            fleet.time(),
        );
        presentation.plotting_frame = void_orbit::FrameSpec::BodyInertial { body: home };
        Self {
            presentation,
            fleet,
            world,
            coupled_world: built.coupled_world,
            terrains,
            planet,
            home,
            selected,
            launch_site: site,
            plans: std::collections::BTreeMap::new(),
            maneuver_warp: warp::ManeuverWarp::Idle,
        }
    }
    /// Geometric nearest configured terrain, evaluated in body-local frames before subtraction.
    /// Home remains the launch identity; navigation and observation do not change collision worlds.
    pub fn nearby_body(&self, vessel: &str) -> usize {
        self.terrains
            .keys()
            .copied()
            .min_by(|&a, &b| {
                let distance = |body| {
                    let local = self
                        .fleet
                        .frames()
                        .transform(
                            self.fleet.vessel_frame(vessel),
                            self.fleet.body_frames(body).1,
                        )
                        .apply_point(self.fleet.root_position_local(vessel));
                    local.length() - self.fleet.ephemeris.bodies()[body].radius_meters
                };
                distance(a).total_cmp(&distance(b))
            })
            .expect("flight: no terrain bodies")
    }
    /// Gravitational navigation reference; distinct from launch identity and terrain proximity.
    pub fn navigation_body(&self, vessel: &str) -> usize {
        let query = self.fleet.vessel_anchor_frame(vessel);
        let bodies = self.fleet.ephemeris.bodies();
        let positions = bodies
            .iter()
            .map(|body| {
                self.fleet
                    .frames()
                    .transform(self.fleet.body_frames(body.index).0, query)
                    .apply_point(DVec3::ZERO)
            })
            .collect::<Vec<_>>();
        void_orbit::DominanceTree::new(bodies).dominant(
            &positions,
            self.fleet.precise_snapshot(vessel).residual.position,
        )
    }
    pub fn observation_body(&self) -> usize {
        self.presentation
            .focus_body
            .unwrap_or_else(|| self.navigation_body(&self.selected))
    }
    pub fn launch_ground_at(&mut self, body_id: &str, craft: &Craft, site: DVec3) -> String {
        self.world.validate_launch(body_id, site);
        let id = self
            .fleet
            .launch_landed(craft, self.world.body_index(body_id), site);
        self.fleet.advance(0.0);
        id
    }
    /// Explicit body-local flight fixture. It is a journaled initial state, never a transfer claim.
    pub fn launch_flight_at(&mut self, body_id: &str, craft: &Craft, local: State) -> String {
        assert!(
            local.position.is_finite()
                && local.position.length_squared() > 0.0
                && local.velocity.is_finite(),
            "flight: invalid fixture"
        );
        let body = self.world.body_index(body_id);
        let transform = self.fleet.frames().transform(
            self.fleet.body_frames(body).1,
            self.fleet.system_frames().systems[self.fleet.ephemeris.system_of(body).0],
        );
        let state = transform.apply_state(void_frames::State {
            position: local.position,
            velocity: local.velocity,
        });
        let up = local.position.normalize();
        let rotation = transform.rotation() * void_landing::upright_at(up);
        let id = self.fleet.launch_in_system(
            craft,
            self.fleet.ephemeris.system_of(body),
            State {
                position: state.position,
                velocity: state.velocity,
            },
            rotation,
            DVec3::ZERO,
        );
        self.fleet.advance(0.0);
        id
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
        let time = self.fleet.time();
        let body = self.nearby_body(&self.selected);
        let mass = self.fleet.snapshot(&self.selected).mass_kg;
        let state = self.fleet.body_fixed_state(&self.selected, body);
        let mut view = self
            .fleet
            .ephemeris
            .local_view(self.fleet.ephemeris.system_of(body));
        let source: &mut dyn void_orbit::EphemerisSource = match view.as_mut() {
            Some(view) => view.as_mut(),
            None => self.fleet.ephemeris.as_mut(),
        };
        let frame = PlanetFrame::new(source, body);
        predict_coast(
            source,
            &frame,
            &self.terrains[&body],
            self.fleet.options.tolerances,
            time,
            state,
            mass,
            horizon,
        )
    }
    /// A repeatable orbital fixture for checking scale, warp and multi-vessel flight without launch.
    pub fn launch_orbital(&mut self, craft: &Craft, offset: DVec3) -> String {
        self.launch_orbital_at(self.home, craft, offset)
    }
    pub fn launch_orbital_at(&mut self, body_index: usize, craft: &Craft, offset: DVec3) -> String {
        let frame = PlanetFrame::new(&self.fleet.ephemeris, body_index);
        let body = &frame.body;
        let r = body.radius_meters + 400_000.0;
        let local = State {
            position: DVec3::X * r + offset,
            velocity: DVec3::Y * ((body.gm / r).sqrt() - frame.omega * r),
        };
        let ground = self.fleet.frames().transform(
            self.fleet.body_frames(body_index).1,
            self.fleet.system_frames().systems[self.fleet.ephemeris.system_of(body_index).0],
        );
        let state = ground.apply_state(void_frames::State {
            position: local.position,
            velocity: local.velocity,
        });
        let state = State {
            position: state.position,
            velocity: state.velocity,
        };
        let rotation = ground.rotation();
        let id = self.fleet.launch_in_system(
            craft,
            self.fleet.ephemeris.system_of(body_index),
            state,
            rotation,
            DVec3::ZERO,
        );
        self.fleet.advance(0.0);
        id
    }
    /// An origin-frame state in a body's surface (body-fixed) frame.
    pub fn body_fixed(&self, body: usize, inertial: State) -> State {
        let s = self
            .fleet
            .frames()
            .transform(self.fleet.origin_frame(), self.fleet.body_frames(body).1)
            .apply_state(void_frames::State {
                position: inertial.position,
                velocity: inertial.velocity,
            });
        State {
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
        let precise = self.fleet.precise_snapshot(vessel);
        let snapshot = precise.residual;
        let mut view = self
            .fleet
            .ephemeris
            .local_view(self.fleet.vessel_system(vessel));
        if let Some(view) = &mut view {
            view.set_physics_offset(precise.anchor);
        } else {
            assert_eq!(
                precise.anchor,
                void_frames::SplitPosition::ORIGIN,
                "plan: unsupported split anchor"
            );
        }
        let source = view.as_deref().unwrap_or(self.fleet.ephemeris.as_ref());
        let mut plan = void_orbit::FlightPlan::new(
            source,
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

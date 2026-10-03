//! The TS lab's existing Fleet collision/merge, near a planet in a distant system.
use glam::{DQuat, DVec3};
use void_frames::SplitPosition;
use void_frames::{BodyId, BodyStates};
use void_landing::FrameState;
use void_multiscale::{FrameEphemeris, SharedWorld};
use void_orbit::EphemerisSource;
use void_vessels::{Environment, Fleet, FleetOptions, pod_tank};

pub struct Encounter {
    pub world: SharedWorld,
    pub fleet: Fleet,
    pub system: String,
    pub planet_index: usize,
    pub first: String,
    pub second: String,
}
impl Encounter {
    pub fn new(world: SharedWorld, system: &str) -> Self {
        let ephemeris = FrameEphemeris::new(world.clone(), system);
        assert_eq!(
            ephemeris.start_time(),
            0.0,
            "encounter fixture starts at zero"
        );
        let planet_index = ephemeris
            .bodies()
            .iter()
            .position(|b| b.id == format!("{system}/planet"))
            .expect("encounter: fixture needs a planet");
        let planet = &ephemeris.bodies()[planet_index];
        let (p, v) = ephemeris.body_state(BodyId(planet_index), 0.0);
        let radius = planet.radius_meters + 400_000.0;
        let state = FrameState {
            position: p + DVec3::X * radius,
            velocity: v + DVec3::Y * (planet.gm / radius).sqrt(),
        };
        let environment = std::sync::Arc::new(Environment::new(&ephemeris));
        let mut fleet = Fleet::new(ephemeris, environment, 0.0, vec![], FleetOptions::default());
        let first = fleet.launch(
            &pod_tank("Near collision A"),
            state,
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        let second = fleet.launch(
            &pod_tank("Near collision B"),
            FrameState {
                position: state.position + DVec3::new(0.05, -4.6, 0.0),
                velocity: state.velocity + DVec3::new(0.0, 0.1, 0.0),
            },
            DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0),
            DVec3::ZERO,
        );
        fleet.advance(0.0);
        Self {
            world,
            fleet,
            system: system.into(),
            planet_index,
            first,
            second,
        }
    }
    pub fn time(&self) -> f64 {
        self.fleet.time()
    }
    pub fn advance(&mut self, seconds: f64) {
        self.fleet.advance(seconds);
    }
    /// A vessel's centre of mass as a galaxy position, from its frame up the tree.
    pub fn position(&self, id: &str) -> SplitPosition {
        self.fleet.frames().to_galaxy(
            self.fleet.vessel_frame(id),
            self.fleet.centre_of_mass_local(id),
        )
    }
    pub fn join(&mut self) -> String {
        let a = format!("{}/p2", self.first);
        let b = format!("{}/p2", self.second);
        let distance = (self.fleet.node_frame(&a, "bottom").0
            - self.fleet.node_frame(&b, "bottom").0)
            .length();
        assert!(
            distance <= 0.25,
            "encounter: node distance {distance} m exceeds 0.25 m"
        );
        let id = self.fleet.join(&a, "bottom", &b, "bottom");
        self.fleet.advance(0.0);
        id
    }
}

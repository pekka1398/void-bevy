//! Optional forces supplied by the caller; Fleet has no atmosphere or vehicle-shape model.
use crate::VesselSnapshot;
use std::collections::HashMap;
use std::sync::Arc;
use void_assembly::{Connection, PartDefinition, PartPose};
use void_orbit::{AirSource, EphemerisSource};

pub struct EnvironmentPart {
    pub id: String,
    pub definition: &'static PartDefinition,
    /// Pose in the vessel's local axes, relative to its instantaneous centre of mass.
    pub pose: PartPose,
}

pub struct EnvironmentSample {
    pub air: Option<Arc<dyn AirSource>>,
    /// Active engine forces are scaled; vacuum mass flow remains unchanged.
    /// Every active engine must have an entry, including engines with scale 1.
    pub thrust_scales: HashMap<String, f64>,
}

pub trait FleetEnvironment: Send + Sync {
    /// Freeze the shape and attitude for one integration leg. The returned AirSource must be
    /// pure and evaluate the candidate position/velocity at every integrator stage.
    fn sample(
        &self,
        ephemeris: &dyn EphemerisSource,
        time: f64,
        vessel: &VesselSnapshot,
        parts: &[EnvironmentPart],
        connections: &[Connection],
    ) -> EnvironmentSample;
}

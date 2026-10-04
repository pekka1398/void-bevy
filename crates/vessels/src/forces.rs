//! Optional part forces supplied by the caller: the part modules (engine back pressure, drag) that
//! turn the world's `Environment` into forces. Fleet has no atmosphere or vehicle-shape model.
use crate::VesselSnapshot;
use std::collections::HashMap;
use std::sync::Arc;
use void_assembly::{Connection, PartDefinition, PartPose};
use void_environment::Environment;
use void_orbit::{AirSource, EphemerisSource};

pub struct ForcePart {
    pub id: String,
    pub definition: &'static PartDefinition,
    /// Pose in the vessel's local axes, relative to its instantaneous centre of mass.
    pub pose: PartPose,
}

pub struct ForceSample {
    pub air: Option<Arc<dyn AirSource>>,
    /// Active engine forces are scaled; vacuum mass flow remains unchanged.
    /// Every active engine must have an entry, including engines with scale 1.
    pub thrust_scales: HashMap<String, f64>,
}

pub trait PartForces: Send + Sync {
    /// Freeze the shape and attitude for one integration leg. The returned AirSource must be
    /// pure and evaluate the candidate position/velocity at every integrator stage.
    fn sample(
        &self,
        environment: &Arc<Environment>,
        ephemeris: &dyn EphemerisSource,
        time: f64,
        vessel: &VesselSnapshot,
        parts: &[ForcePart],
        connections: &[Connection],
    ) -> ForceSample;
}

//! The orbit lab's N-body mechanics (`lab/orbit/src/orbit`), ported to Rust. Systems are JSON
//! files exported from the lab's presets (`systems/`); checks compare with the lab's own output.

mod apsides;
mod dopri5;
mod ephemeris;
mod flight_plan;
mod hermite;
mod kepler;
mod propagator;
mod simulation;
mod system;
mod trajectory;

pub use apsides::{Apsis, ApsisKind, DominanceTree, find_apsides};
pub use dopri5::Dopri5;
pub use ephemeris::{Ephemeris, EphemerisOptions, suggested_step_seconds, yoshida8_sequence};
pub use flight_plan::{
    BurnSchedule, FlightPlan, ManeuverSpec, ManeuverStatus, PlanEngine, ReferenceMode,
};
pub use hermite::HermiteBasis;
pub use kepler::{
    EllipticElements, OsculatingOrbit, orbital_period_seconds, osculating_orbit,
    solve_kepler_elliptic, state_from_elements, true_anomaly,
};
pub use propagator::{
    AdvanceOutcome, AttitudeLaw, Control, ForceControl, Impact, PropagationRun, ThrustControl,
    Tolerances, VesselPropagator, VesselState,
};
pub use simulation::{
    AdvanceReport, AttitudeMode, EngineSpec, ImpactRecord, STANDARD_GRAVITY, Simulation,
    SimulationOptions, StartPlane, VesselStartSpec,
};
pub use system::{
    BodySpec, BuiltSystem, CelestialBody, GRAVITATIONAL_CONSTANT, GravityField, LockedRotationSpec,
    OrbitPlane, RotationSpec, SpinSpec, SystemSpec, body_orientation, build_system,
};
pub use trajectory::Trajectory;

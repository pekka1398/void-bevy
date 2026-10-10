//! N-body orbital mechanics: star systems read from JSON (`systems/`), the ephemeris that
//! integrates their bodies, and vessels propagated through its gravity.

mod apsides;
mod dopri5;
mod ephemeris;
mod flight_plan;
mod frames;
pub mod gravity;
mod hermite;
mod kepler;
mod navigation;
mod nodes;
mod propagator;
mod reference_frames;
mod system;
mod trajectory;

pub use apsides::{Apsis, ApsisKind, DominanceTree, find_apsides};
pub use dopri5::Dopri5;
pub use ephemeris::{
    Ephemeris, EphemerisOptions, EphemerisSource, suggested_step_seconds, yoshida8_sequence,
};
pub use flight_plan::{
    BurnSchedule, FlightPlan, FlightPlanCheckpoint, ManeuverSpec, ManeuverStatus, PlanEngine,
    ReferenceMode,
};
pub use frames::SystemFrames;
pub use hermite::HermiteBasis;
pub use kepler::{
    EllipticElements, OsculatingOrbit, orbital_period_seconds, osculating_orbit,
    solve_kepler_elliptic, state_from_elements, true_anomaly,
};
pub use navigation::{
    NavigationError, NavigationOperation, NavigationRequest, NavigationSolution, solve_navigation,
};
pub use nodes::{NodeKind, OrbitNode, find_nodes};
pub use propagator::{
    AdvanceOutcome, AirSource, AttitudeLaw, Control, ForceControl, Impact, PropagationRun,
    ThrustControl, Tolerances, VesselPropagator, VesselState,
};
pub use reference_frames::{
    FrameEvaluator, FrameSpec, PlotFrameState, direction_to_frame, to_frame,
};
pub use system::{
    BodySpec, BuiltSystem, CelestialBody, GRAVITATIONAL_CONSTANT, GravityField, LockedRotationSpec,
    OrbitPlane, RotationSpec, SpinSpec, SystemSpec, body_orientation, build_system,
};
pub use trajectory::Trajectory;

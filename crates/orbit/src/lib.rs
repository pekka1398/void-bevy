//! The orbit lab's N-body mechanics (`lab/orbit/src/orbit`), ported to Rust. Systems are JSON
//! files exported from the lab's presets (`systems/`); checks compare with the lab's own output.

mod ephemeris;
mod hermite;
mod kepler;
mod system;

pub use ephemeris::{Ephemeris, EphemerisOptions, suggested_step_seconds};
pub use hermite::HermiteBasis;
pub use kepler::{
    EllipticElements, OsculatingOrbit, orbital_period_seconds, osculating_orbit, solve_kepler_elliptic,
    state_from_elements, true_anomaly,
};
pub use system::{
    BodySpec, BuiltSystem, CelestialBody, GRAVITATIONAL_CONSTANT, GravityField, LockedRotationSpec, OrbitPlane,
    RotationSpec, SpinSpec, SystemSpec, build_system,
};

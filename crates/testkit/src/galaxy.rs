//! Fixtures: three Sun-like systems a few light-years apart, placed
//! 30,000 light-years from the origin, and a probe sent from Aster toward Beryl.

use glam::DVec3;
use void_orbit::{BuiltSystem, SystemSpec, build_system};

use void_frames::SplitPosition;
use void_multiscale::{CoupledWorld, FramedState, LIGHT_YEAR, SystemSeed, Traveller};

pub const AU: f64 = 149_597_870_700.0;
pub const YEAR: f64 = 365.25 * 86400.0;
const SPEED_OF_LIGHT: f64 = 299_792_458.0;

/// A star of `mass_scale` Suns with one Earth on a slightly eccentric, inclined orbit.
pub fn planetary_system(name: &str, mass_scale: f64, orbit_radius: f64) -> BuiltSystem {
    let rotation = serde_json::json!({
        "periodSeconds": 86400.0, "obliquityRadians": 0.0, "poleLongitudeRadians": 0.0, "angleAtEpochRadians": 0.0,
    });
    let spec = serde_json::json!({
        "name": name,
        "root": {
            "id": "star", "name": "Star", "massKg": 1.98847e30 * mass_scale, "radiusMeters": 6.957e8,
            "color": "#ffd6a0", "rotation": rotation,
            "children": [{
                "id": "planet", "name": "Planet", "massKg": 5.9722e24, "radiusMeters": 6.371e6,
                "color": "#76bde8", "rotation": rotation, "orbitPlane": "ecliptic",
                "orbit": {
                    "semiMajorAxisMeters": orbit_radius, "eccentricity": 0.04, "inclinationRadians": 0.1,
                    "longitudeOfAscendingNodeRadians": 0.3, "argumentOfPeriapsisRadians": 0.2,
                    "meanAnomalyRadians": 0.7,
                },
                "children": [],
            }],
        },
    });
    build_system(&SystemSpec::from_json(&spec.to_string()))
}

/// The default placement: 30,000 light-years out. A coordinate placement only; there is
/// no galaxy potential.
pub fn default_galaxy() -> SplitPosition {
    SplitPosition::at(DVec3::new(
        30_000.0 * LIGHT_YEAR,
        -12_000.0 * LIGHT_YEAR,
        500.0 * LIGHT_YEAR,
    ))
}

/// Aster at `galaxy`, Beryl 4 light-years along +x, Cygnus in another direction, all drifting
/// at about 220 km/s.
pub fn wide_seeds(galaxy: SplitPosition) -> Vec<SystemSeed> {
    let origin = |offset: DVec3| galaxy.compose(&SplitPosition::at(offset));
    vec![
        SystemSeed {
            id: "Aster".into(),
            system: planetary_system("Aster", 1.0, AU),
            origin: origin(DVec3::ZERO),
            velocity: DVec3::new(220_000.0, 0.0, 0.0),
        },
        SystemSeed {
            id: "Beryl".into(),
            system: planetary_system("Beryl", 0.8, AU * 0.8),
            origin: origin(DVec3::new(4.0 * LIGHT_YEAR, 0.0, 0.0)),
            velocity: DVec3::new(220_000.0, 100.0, 0.0),
        },
        SystemSeed {
            id: "Cygnus".into(),
            system: planetary_system("Cygnus", 1.2, AU * 1.3),
            origin: origin(DVec3::new(-2.0 * LIGHT_YEAR, 3.0 * LIGHT_YEAR, LIGHT_YEAR)),
            velocity: DVec3::new(219_900.0, -50.0, 20.0),
        },
    ]
}

/// The wide fixture with one-day steps and 8,192 retained samples.
pub fn wide_world(galaxy: SplitPosition) -> CoupledWorld {
    CoupledWorld::new(wide_seeds(galaxy), 86400.0, 8192)
}

/// A probe leaving Aster at `fraction` of light speed toward Beryl, from 100 AU out.
pub fn transfer(world: &CoupledWorld, fraction: f64) -> Traveller {
    assert!(
        (0.0001..=0.05).contains(&fraction),
        "transfer: Newtonian fixture speed must be 0.0001-0.05 c"
    );
    Traveller::new(
        world,
        0.0,
        FramedState {
            frame: world.system_frame("Aster"),
            position: SplitPosition::at(DVec3::new(100.0 * AU, 100.0 * AU, 0.0)),
            velocity: DVec3::new(fraction * SPEED_OF_LIGHT, 100.0, 0.0),
        },
        Traveller::MAX_STEP,
    )
}

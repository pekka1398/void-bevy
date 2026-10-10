//! Time rates the main game offers, and how much clearance on-rails warp needs.

/// One row of time rates. Up to `PHYSICS_MAX_RATE` everything is simulated and the engine may
/// burn; above it the rocket is on rails: coasting only, and no part moving near the ground.
pub const TIME_RATES: [f64; 9] = [1.0, 2.0, 4.0, 5.0, 20.0, 100.0, 1000.0, 10_000.0, 100_000.0];
pub const PHYSICS_MAX_RATE: f64 = 4.0;

/// Lowest clearance of any part in orbital flight each on-rails rate needs, in radii of the
/// planet (Aurelia: 6.4 km, 9.6 km, 12.7 km, 64 km, 319 km, 1,274 km). A first guess, to tune.
pub fn rails_min_clearance_radii(rate: f64) -> f64 {
    match rate as u64 {
        5 => 0.001,
        20 => 0.0015,
        100 => 0.002,
        1000 => 0.01,
        10_000 => 0.05,
        100_000 => 0.2,
        _ => panic!("flight: no clearance limit for {rate}x"),
    }
}

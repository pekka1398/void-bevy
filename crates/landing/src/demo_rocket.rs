//! The two-stage lab rocket, as `lab/landing/src/vessel/DemoRocket.ts`: collider shapes, masses,
//! engines, and the contact options sized to its planet.

use glam::{DQuat, DVec3};
use void_math::hypot;
use void_orbit::Tolerances;
use void_terrain::Terrain;

use crate::contact_world::{BodyShape, ContactWorldOptions, Piece, SimpleShape};
use crate::lander::{LanderOptions, LanderSpec};
use crate::planets::level_for_tile_size;

#[derive(Clone, Debug)]
pub struct DemoRocket {
    /// The stack before separation (aggregate of both parts, booster engine).
    pub full: LanderSpec,
    pub upper: LanderSpec,
    pub booster: LanderSpec,
    pub upper_shape: BodyShape,
    pub booster_shape: BodyShape,
    pub options: LanderOptions,
    /// Body-fixed direction of the launch site.
    pub launch_site: DVec3,
}

fn piece(shape: SimpleShape, position: DVec3) -> Piece {
    Piece {
        shape,
        position,
        rotation: None,
        mass: None,
    }
}

pub fn upper_pieces() -> Vec<Piece> {
    vec![
        piece(
            SimpleShape::Cylinder {
                radius: 1.05,
                half_height: 0.875,
            },
            DVec3::new(0.0, 0.175, 0.0),
        ),
        piece(
            SimpleShape::Cone {
                radius: 0.9,
                half_height: 0.5,
            },
            DVec3::new(0.0, 1.55, 0.0),
        ),
        piece(
            SimpleShape::Cone {
                radius: 0.43,
                half_height: 0.185,
            },
            DVec3::new(0.0, -0.86, 0.0),
        ),
    ]
}

/// Booster body and engine plus four splayed legs, each a strut ending in a foot pad.
pub fn booster_pieces() -> Vec<Piece> {
    let mut pieces = vec![
        piece(
            SimpleShape::Cylinder {
                radius: 1.25,
                half_height: 1.175,
            },
            DVec3::new(0.0, 0.175, 0.0),
        ),
        piece(
            SimpleShape::Cone {
                radius: 0.58,
                half_height: 0.185,
            },
            DVec3::new(0.0, -1.21, 0.0),
        ),
    ];
    for x in [-1.0, 1.0] {
        for z in [-1.0, 1.0] {
            let root = DVec3::new(x * 0.72, 0.05, z * 0.72);
            let foot = DVec3::new(x * 1.28, -1.37, z * 1.28);
            let span = foot - root;
            let length = hypot([span.x, span.y, span.z]);
            let d = DVec3::new(span.x / length, span.y / length, span.z / length);
            // Shortest rotation from +y to the strut direction: axis y × d, half-angle quaternion.
            let w = ((1.0 + d.y) / 2.0).sqrt();
            let rotation = DQuat::from_xyzw(d.z / (2.0 * w), 0.0, -d.x / (2.0 * w), w);
            pieces.push(Piece {
                shape: SimpleShape::Cylinder {
                    radius: 0.1,
                    half_height: length / 2.0,
                },
                position: DVec3::new(
                    (root.x + foot.x) / 2.0,
                    (root.y + foot.y) / 2.0,
                    (root.z + foot.z) / 2.0,
                ),
                rotation: Some(rotation),
                mass: None,
            });
            pieces.push(piece(
                SimpleShape::Box {
                    half_extents: DVec3::new(0.21, 0.05, 0.21),
                },
                foot,
            ));
        }
    }
    pieces
}

pub fn demo_rocket(terrain: &Terrain) -> DemoRocket {
    let upper_shape = BodyShape::Compound(upper_pieces());
    let booster_shape = BodyShape::Compound(booster_pieces());
    // Sized to reach low orbit on an Earth-size planet through its air, KSP-style (light tanks,
    // strong engines): booster 120 kN, Isp 310 s, 3.68 km/s; upper 20 kN, Isp 340 s, 5.92 km/s;
    // 9.6 km/s in all, which flown by hand leaves room over Earth's roughly 9.4 km/s to orbit.
    // Liftoff thrust-to-weight is 1.61, low enough not to spend the margin fighting drag down low;
    // the upper stage lights at 1.15, which it only ever needs above the air. The tanks hold this
    // much in the same hulls as before: the shapes are the colliders and the drawn rocket.
    let full = LanderSpec {
        thrust_newtons: 120_000.0,
        specific_impulse_seconds: 310.0,
        // 107.8 kN and 279 s at sea level, 90% of vacuum, as a good kerolox engine gives.
        nozzle_exit_area_m2: 0.12,
        dry_mass_kg: 2270.0,
        fuel_mass_kg: 5350.0,
        // Reference point is the attached parts' centre of mass; feet are about 2 m below it.
        half_extents: DVec3::new(1.5, 2.05, 1.5),
        contact_shape: None,
        friction: 0.8,
        crash_tolerance_meters_per_second: None,
    };
    let upper = LanderSpec {
        thrust_newtons: 20_000.0,
        specific_impulse_seconds: 340.0,
        // A vacuum nozzle: at sea level it would keep only a quarter of its thrust, which is why
        // the stage is not meant to be lit down there.
        nozzle_exit_area_m2: 0.15,
        dry_mass_kg: 300.0,
        fuel_mass_kg: 1470.0,
        half_extents: DVec3::new(1.05, 1.15, 1.05),
        contact_shape: Some(upper_shape.clone()),
        friction: 0.8,
        crash_tolerance_meters_per_second: Some(10.0),
    };
    let booster = LanderSpec {
        thrust_newtons: full.thrust_newtons,
        specific_impulse_seconds: full.specific_impulse_seconds,
        nozzle_exit_area_m2: full.nozzle_exit_area_m2,
        dry_mass_kg: 500.0,
        fuel_mass_kg: 5350.0,
        half_extents: DVec3::new(1.5, 1.42, 1.5),
        contact_shape: Some(booster_shape.clone()),
        friction: 0.8,
        crash_tolerance_meters_per_second: Some(10.0),
    };
    let options = LanderOptions {
        contact: ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: level_for_tile_size(terrain.radius_meters, 300.0),
            tile_resolution: 33,
            tile_reach_meters: 300.0,
            tile_keep_meters: 600.0,
            recenter_meters: 1000.0,
            sleeping: true,
        },
        tolerances: Tolerances {
            position_meters: 1e-6,
            velocity_meters_per_second: 1e-9,
        },
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
    };
    DemoRocket {
        full,
        upper,
        booster,
        upper_shape,
        booster_shape,
        options,
        launch_site: DVec3::new(0.8, 0.55, 0.25),
    }
}

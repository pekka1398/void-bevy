//! Every planet: drawn terrain is the collision terrain, launch and return, and time on rails.
//! lab/landing's own checks (`landing-check.ts`) with its thresholds.

use std::sync::Arc;

use glam::DVec3;
use void_landing::{
    BodyShape, ContactWorldOptions, LanderControl, LanderOptions, LanderSpec, PartJointRocket,
    PhysicsMode, RocketPart, SimpleShape, aurelia, demo_rocket, landing_lod_options,
    level_for_tile_size, pebble, planet_by_id, planet_ephemeris,
};
use void_lod::{
    FACE_EDGES, LodView, PlanetLod, TileMeshData, TileMeshOptions, build_tile_mesh, neighbor_key,
    tile_containing, tiles_around,
};
use void_orbit::Tolerances;
use void_terrain::Terrain;

const PLANETS: [&str; 5] = ["pebble", "luna", "terra", "aurelia", "aurelia-fast"];
const LAUNCH_SITE: DVec3 = DVec3::new(0.8, 0.55, 0.25);

fn contact(radius: f64) -> ContactWorldOptions {
    ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(radius, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    }
}

fn stub(key: void_lod::TileKey) -> TileMeshData {
    TileMeshData {
        key,
        origin: DVec3::ZERO,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        heights: Vec::new(),
        grid: Vec::new(),
        min_height_meters: 0.0,
        max_height_meters: 0.0,
        error_meters: 0.0,
        skirt_depth_meters: 0.0,
        build_seconds: 0.0,
    }
}

#[test]
fn terrain_rebuilt_from_its_config_is_identical() {
    let planet = pebble();
    let key = tile_containing(DVec3::new(0.6, -0.2, 0.77), contact(100e3).tile_level);
    let options = TileMeshOptions {
        radius_meters: planet.terrain.radius_meters,
        resolution: 33,
    };
    let main = build_tile_mesh(key, &*planet.terrain, options);
    let rebuilt = build_tile_mesh(key, &Terrain::from_config(&planet.terrain_config), options);
    assert!(
        main.positions == rebuilt.positions && main.origin == rebuilt.origin,
        "{key}: rebuilt terrain differs"
    );
}

#[test]
fn drawn_terrain_equals_collision_terrain_near_every_part() {
    for id in PLANETS {
        let terrain = planet_by_id(id).terrain;
        let c = contact(terrain.radius_meters);
        let options = landing_lod_options(&terrain, &c);
        let directions: Vec<DVec3> = [
            DVec3::new(1.0, 1.0, 1.0),
            DVec3::new(1.0, 0.001, 0.3),
            DVec3::new(-0.2, 0.5, -0.8),
        ]
        .map(|d| d.normalize())
        .to_vec();
        let highest = terrain.radius_meters + terrain.max_height_meters + c.tile_reach_meters;
        let mut cases: Vec<(&str, Vec<DVec3>)> = Vec::new();
        for d in &directions {
            cases.push((
                "on the ground",
                vec![*d * (terrain.radius_meters + terrain.height(*d))],
            ));
            cases.push(("at the top of collision range", vec![*d * highest]));
        }
        // A second part 20 km along the surface from the first.
        let d0 = directions[2];
        let angle = 20_000.0 / terrain.radius_meters;
        let t = DVec3::new(-d0.y, d0.x, 0.0) / DVec3::new(-d0.y, d0.x, 0.0).length();
        let d1 = DVec3::new(
            d0.x * angle.cos() + t.x * angle.sin(),
            d0.y * angle.cos() + t.y * angle.sin(),
            d0.z * angle.cos(),
        );
        cases.push(("two parts 20 km apart", vec![d0 * highest, d1 * highest]));
        let (mut covered, mut bad, mut worst_drawn) = (0, 0, 0);
        let mut problems = Vec::new();
        for (label, observers) in &cases {
            let mut lod = PlanetLod::new(void_lod::PlanetLodOptions {
                max_cached_tiles: 50_000,
                ..options.clone()
            });
            // The same view as the game's terrain: the parts are the observers, horizon culling on.
            let view = LodView {
                observer_positions: observers.clone(),
                camera: None,
                distance_scale: 1.0,
                horizon_culling: true,
            };
            let mut selected = lod.select(&view);
            let mut iteration = 0;
            while !selected.requests.is_empty() && iteration < 4 * options.max_level {
                for request in &selected.requests {
                    lod.accept_tile(Arc::new(stub(request.key)));
                }
                selected = lod.select(&view);
                iteration += 1;
            }
            worst_drawn = worst_drawn.max(selected.render.len());
            let drawn: std::collections::HashSet<u64> = selected.render.iter().copied().collect();
            if !selected.requests.is_empty() {
                bad += 1;
                problems.push(format!("{label}: selection did not settle"));
            }
            for observer in observers {
                for k in tiles_around(
                    *observer,
                    c.tile_keep_meters,
                    c.tile_level,
                    terrain.radius_meters,
                ) {
                    covered += 1;
                    let missing: Vec<_> = std::iter::once(k)
                        .chain(FACE_EDGES.iter().map(|&e| neighbor_key(k, e)))
                        .filter(|n| !drawn.contains(&n.code()))
                        .collect();
                    if !missing.is_empty() {
                        bad += 1;
                        problems.push(format!("{label}: {k} lacks {missing:?}"));
                    }
                }
            }
        }
        println!(
            "drawn terrain equals collision terrain near every part ({id}): {} cases, {covered} collision tiles within {} m all drawn at L{} with same-level neighbours; at most {worst_drawn} tiles drawn",
            cases.len(),
            c.tile_keep_meters,
            c.tile_level
        );
        assert!(bad == 0, "{id}: {problems:?}");
    }
}

fn boxed(half: DVec3) -> Option<BodyShape> {
    Some(BodyShape::Simple(SimpleShape::Box { half_extents: half }))
}

#[test]
fn launch_and_return_on_every_planet() {
    for id in PLANETS {
        let planet = planet_by_id(id);
        let (mut eph, index) = planet_ephemeris(&planet);
        let options = LanderOptions {
            contact: contact(planet.terrain.radius_meters),
            tolerances: Tolerances {
                position_meters: 1e-6,
                velocity_meters_per_second: 1e-9,
            },
            band_enter_meters: 200.0,
            band_exit_meters: 400.0,
        };
        let upper = LanderSpec {
            thrust_newtons: 8000.0,
            specific_impulse_seconds: 330.0,
            dry_mass_kg: 300.0,
            fuel_mass_kg: 200.0,
            half_extents: DVec3::new(1.0, 1.05, 1.0),
            contact_shape: boxed(DVec3::new(1.0, 1.05, 1.0)),
            friction: 0.8,
            crash_tolerance_meters_per_second: Some(10.0),
        };
        let booster = LanderSpec {
            thrust_newtons: 28000.0,
            specific_impulse_seconds: 280.0,
            dry_mass_kg: 500.0,
            fuel_mass_kg: 900.0,
            half_extents: DVec3::new(1.0, 1.35, 1.0),
            contact_shape: boxed(DVec3::new(1.0, 1.35, 1.0)),
            friction: 0.8,
            crash_tolerance_meters_per_second: Some(10.0),
        };
        let full = LanderSpec {
            dry_mass_kg: 1000.0,
            fuel_mass_kg: 900.0,
            half_extents: DVec3::new(1.0, 2.05, 1.0),
            ..booster.clone()
        };
        let mut rocket = PartJointRocket::landed(
            &mut eph,
            index,
            planet.terrain.clone(),
            full.clone(),
            upper,
            booster.clone(),
            options,
            LAUNCH_SITE,
        );
        let coast = LanderControl {
            up: 1.0,
            ..Default::default()
        };
        rocket.advance(&mut eph, 2.0, &coast, None);
        rocket.advance(
            &mut eph,
            20.0,
            &LanderControl {
                throttle: 1.0,
                up: 1.0,
                ..Default::default()
            },
            None,
        );
        let mut peak = rocket.clearance(&eph);
        let mut g = 0;
        while rocket.mode() == PhysicsMode::Flight && g < 4000 {
            rocket.advance(&mut eph, 0.5, &coast, None);
            peak = peak.max(rocket.clearance(&eph));
            g += 1;
        }
        // A vertical burn at the stack's initial thrust-to-weight reaches at least this height; a stack
        // that bends or tips does not.
        let gravity = eph.bodies()[index].gm / planet.terrain.radius_meters.powi(2);
        let burn_only = 0.5
            * (booster.thrust_newtons / (full.dry_mass_kg + full.fuel_mass_kg) - gravity)
            * 20.0_f64.powi(2);
        let changes = &rocket.mode_changes;
        let ok = changes.len() >= 2
            && changes[0].from == PhysicsMode::Contact
            && changes[0].to == PhysicsMode::Flight
            && changes[1].from == PhysicsMode::Flight
            && changes[1].to == PhysicsMode::Contact
            && peak > 0.9 * burn_only;
        let summary: Vec<String> = changes
            .iter()
            .take(2)
            .map(|c| format!("{:?}→{:?} at {:.0} s", c.from, c.to, c.time))
            .collect();
        println!(
            "launch and return on {id}: {}; peak {:.2} km (a vertical 20 s burn alone reaches {:.2} km)",
            summary.join(", "),
            peak / 1000.0,
            burn_only / 1000.0
        );
        assert!(ok, "launch and return on {id}");
    }
}

#[test]
fn on_rails() {
    let planet = aurelia();
    let (mut eph, index) = planet_ephemeris(&planet);
    let demo = demo_rocket(&planet.terrain);
    let make = |eph: &mut void_orbit::Ephemeris| {
        PartJointRocket::landed(
            eph,
            index,
            planet.terrain.clone(),
            demo.full.clone(),
            demo.upper.clone(),
            demo.booster.clone(),
            demo.options,
            demo.launch_site,
        )
    };
    let coast = LanderControl {
        up: 1.0,
        turn: Some(DVec3::ZERO),
        ..Default::default()
    };

    // A rocket settling on the pad is awake, then Rapier puts it to sleep; asleep it can go on rails.
    let mut pad = make(&mut eph);
    let settling = pad.rails_blocker(0.0);
    pad.advance(&mut eph, 6.0, &coast, None);
    let (firing, resting) = (pad.rails_blocker(0.5), pad.rails_blocker(0.0));
    let before = pad.body_fixed_state(&eph).position;
    let ok = pad.advance_on_rails(&mut eph, 86_400.0);
    let moved = (pad.body_fixed_state(&eph).position - before).length();
    let world_time = pad.contact_worlds()[0].time;
    pad.advance(&mut eph, 2.0, &coast, None);
    let after = (pad.body_fixed_state(&eph).position - before).length();
    println!(
        "on rails: resting on the ground: settling {settling:?}; throttle up {firing:?}; asleep: a day on rails moved {moved} m, Rapier's clock followed to {world_time:.0} s; 2 s of physics after moved {after:.2e} m"
    );
    assert!(settling.is_some() && firing.is_some() && resting.is_none() && ok && moved == 0.0);
    assert!(
        (world_time - pad.time() + 2.0).abs() < 1e-6
            && after < 1e-3
            && pad.mode() == PhysicsMode::Contact
    );

    let mut awake = make(&mut eph);
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        awake.advance_on_rails(&mut eph, 1.0)
    }))
    .is_err();
    println!("on rails: refused while a part is awake near the ground: {refused}");
    assert!(refused);

    // A coasting stack: on rails is the same coast as physics time; coming down, rails stop at the band.
    let burn = LanderControl {
        throttle: 1.0,
        ..coast.clone()
    };
    let (mut railed, mut simulated) = (make(&mut eph), make(&mut eph));
    for l in [&mut railed, &mut simulated] {
        for _ in 0..80 * 60 {
            l.advance(&mut eph, 1.0 / 60.0, &burn, None);
        }
        l.advance(&mut eph, 30.0, &coast, None);
    }
    railed.advance_on_rails(&mut eph, 200.0);
    simulated.advance(&mut eph, 200.0, &coast, None);
    let gap = (railed.body_fixed_state(&eph).position - simulated.body_fixed_state(&eph).position)
        .length();
    let mut stopped = true;
    while railed.mode() == PhysicsMode::Flight {
        stopped = railed.advance_on_rails(&mut eph, 20.0);
    }
    println!(
        "on rails: coasting: 200 s on rails vs physics differ by {gap:.2e} m; coming down, rails stopped at {:.0} m in contact, then {:?}",
        railed.part_clearance(&eph, RocketPart::Upper),
        railed.rails_blocker(0.0)
    );
    assert!(
        gap < 1e-3
            && !stopped
            && railed.mode() == PhysicsMode::Contact
            && railed.rails_blocker(0.0).is_some()
    );
}

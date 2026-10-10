//! Worlds and sites only the tests use. The game's own world is `world::main_game`.
#![allow(dead_code)]
use glam::DVec3;
use void_fleet_flight::world::{
    NeighborSystem, StellarConfiguration, SystemPlacement, WorldDescription, main_game,
};
use void_landing::LandingPlanet;

/// `planet` on its own, with the main game's other Sol bodies around it.
pub fn solar_world(planet: &LandingPlanet) -> WorldDescription {
    let mut world = WorldDescription::single(planet, true);
    for (id, body) in main_game(&void_assembly::flight_rocket()).world.bodies {
        world.bodies.entry(id).or_insert(body);
    }
    world
}

/// `planet` with the main game's Selene beside it.
pub fn aurelia_selene(planet: &LandingPlanet) -> WorldDescription {
    assert_eq!(planet.body_id, "aurelia", "world: preset needs Aurelia");
    let mut world = WorldDescription::single(planet, true);
    let selene = main_game(&void_assembly::flight_rocket()).world.bodies["selene"].clone();
    world.bodies.insert("selene".into(), selene);
    world
}

/// Authored fictional neighborhood at real stellar separations. This is not a transfer
/// fixture: the ordinary launch craft starts landed with its ordinary resources and speed.
pub fn stellar_neighborhood(planet: &LandingPlanet) -> WorldDescription {
    let mut world = solar_world(planet);
    let home = SystemPlacement {
        id: "Sol".into(),
        origin: void_testkit::default_galaxy(),
        velocity: DVec3::new(220_000.0, 0.0, 0.0),
    };
    let mut neighbor_spec = world.system.clone();
    neighbor_spec.root.children.retain(|b| b.id == "aurelia");
    let neighbors = [
        ("Beryl", DVec3::new(4.24, 0.0, 0.0), 0.8),
        ("Cygnus", DVec3::new(-3.0, 5.0, 1.0), 1.1),
    ]
    .into_iter()
    .map(|(id, light_years, mass)| {
        let mut system = neighbor_spec.clone();
        system.name = format!("{id} fictional stellar system");
        system.root.name = format!("{id} Star");
        system.root.mass_kg *= mass;
        NeighborSystem {
            placement: SystemPlacement {
                id: id.into(),
                origin: home
                    .origin
                    .translate(light_years * void_multiscale::LIGHT_YEAR),
                velocity: home.velocity + DVec3::new(0.0, 100.0 * mass, 0.0),
            },
            system,
        }
    })
    .collect::<Vec<_>>();
    let original = world.bodies.clone();
    world.bodies = original
        .iter()
        .map(|(id, d)| (format!("Sol/{id}"), d.clone()))
        .collect();
    for neighbor in &neighbors {
        for id in ["sol", "aurelia", "selene"] {
            let mut description = original[id].clone();
            description.label = format!("{} · {id}", neighbor.placement.id);
            world
                .bodies
                .insert(format!("{}/{id}", neighbor.placement.id), description);
        }
    }
    world.stellar = Some(StellarConfiguration { home, neighbors });
    world
}

/// Deterministic acceptance-fixture site on real dry terrain in this body's own daylight.
/// Does not change terrain or illumination, and does not modify ordinary launch defaults.
pub fn daylight_terrain_site(world: &WorldDescription, id: &str) -> Result<DVec3, String> {
    let body = world.body_index(id);
    let built = world.build();
    let terrain = built
        .terrains
        .get(&body)
        .ok_or("daylight fixture needs authored solid terrain")?;
    let source = built.ephemeris.as_ref();
    let star = source
        .bodies()
        .iter()
        .find(|b| b.parent_index.is_none() && source.system_of(b.index) == source.system_of(body))
        .ok_or("daylight fixture needs its own stellar root")?;
    if star.index == body {
        return Err("daylight terrain fixture cannot launch on a stellar root".into());
    }
    let frames = void_orbit::SystemFrames::new(source);
    let star_local = frames
        .tree
        .at(0.0, source)
        .transform(frames.inertial[star.index], frames.surface[body])
        .apply_point(DVec3::ZERO);
    assert!(
        star_local.is_finite() && star_local.length_squared() > 0.0,
        "fixture: invalid stellar direction"
    );
    let sun = star_local.normalize();
    let axis = if sun.z.abs() < 0.9 {
        DVec3::Z
    } else {
        DVec3::X
    };
    let east = axis.cross(sun).normalize();
    let north = sun.cross(east);
    let sea = world.bodies[id].sea_level_meters;
    let angle_step = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let reach = 12.0;
    let mut best: Option<(f64, DVec3)> = None;
    for i in 0..512 {
        let mu = 0.35 + 0.65 * (i as f64 + 0.5) / 512.0;
        let angle = i as f64 * angle_step;
        let direction = (sun * mu
            + (east * angle.cos() + north * angle.sin()) * (1.0 - mu * mu).sqrt())
        .normalize();
        let height = terrain.height(direction);
        if sea.is_some_and(|sea| height <= sea + 20.0) {
            continue;
        }
        let tangent = axis.cross(direction).normalize();
        let other = direction.cross(tangent);
        let radius = terrain.radius_meters + height;
        let slope = [tangent, -tangent, other, -other]
            .into_iter()
            .map(|side| {
                (terrain.height((direction * radius + side * reach).normalize()) - height).abs()
                    / reach
            })
            .fold(0.0_f64, f64::max);
        if slope > 0.025 {
            continue;
        }
        if best.is_none_or(|(score, _)| slope < score) {
            best = Some((slope, direction));
        }
    }
    best.map(|(_, direction)| direction)
        .ok_or_else(|| format!("no dry, sufficiently level daylight terrain site on {id}"))
}

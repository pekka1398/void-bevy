//! Serializable world configuration shared by physics and rendering. No renderer handles.
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use void_environment::{Atmosphere, BodyEnvironment, EarthAtmosphere, Environment};
use void_landing::{ContactWorldOptions, LandingPlanet, level_for_tile_size};
use void_orbit::{
    Ephemeris, EphemerisOptions, EphemerisSource, SystemSpec, build_system, suggested_step_seconds,
};
use void_terrain::{Terrain, TerrainConfig};
use void_vessels::GroundSpec;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisualSettings {
    pub surface: void_scenery::solar::SurfaceRecipe,
    pub rings: Option<void_scenery::solar::RingRecipe>,
    pub surface_color: Option<[f32; 3]>,
    pub atmosphere: bool,
    /// Explicit optical model; independent of physical pressure/density. None means no optical air.
    pub scattering: Option<void_scenery::atmosphere_scene::AtmosphereProfile>,
    pub clouds: bool,
    pub cloud_profile: Option<void_scenery::atmosphere_scene::CloudProfile>,
    pub ocean: bool,
    /// Reference height for terrain colour bands/cloud placement; water rendering must match sea.
    pub color_datum_meters: f64,
    pub rock_height_meters: f64,
    pub snow_height_meters: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BodyDescription {
    pub label: String,
    pub terrain: Option<TerrainConfig>,
    pub air_density_scale: Option<f64>,
    pub air_datum_meters: f64,
    pub sea_level_meters: Option<f64>,
    pub visual: VisualSettings,
}
/// Explicit, nonrotating stellar placement. Positions stay split even in save files.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemPlacement {
    pub id: String,
    pub origin: void_frames::SplitPosition,
    pub velocity: DVec3,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborSystem {
    pub placement: SystemPlacement,
    pub system: SystemSpec,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StellarConfiguration {
    pub home: SystemPlacement,
    pub neighbors: Vec<NeighborSystem>,
}
impl StellarConfiguration {
    fn seeds(&self, home: &SystemSpec) -> Vec<void_multiscale::SystemSeed> {
        assert!(
            (1..=2).contains(&self.neighbors.len()),
            "world: neighborhood needs two or three systems"
        );
        let seed = |placement: &SystemPlacement, spec: &SystemSpec| {
            assert!(
                !placement.id.is_empty() && !placement.id.contains('/'),
                "world: invalid system ID"
            );
            let system = build_system(spec);
            assert!(
                system.bodies.iter().all(|b| !b.id.contains('/')),
                "stellar body IDs cannot contain namespace separators"
            );
            void_multiscale::SystemSeed {
                id: placement.id.clone(),
                system,
                origin: placement.origin,
                velocity: placement.velocity,
            }
        };
        std::iter::once(seed(&self.home, home))
            .chain(self.neighbors.iter().map(|n| seed(&n.placement, &n.system)))
            .collect()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDescription {
    pub schema: u32,
    pub system: SystemSpec,
    pub stellar: Option<StellarConfiguration>,
    #[serde(deserialize_with = "unique_bodies")]
    pub bodies: BTreeMap<String, BodyDescription>,
}
pub struct BuiltWorld {
    pub ephemeris: Box<dyn EphemerisSource>,
    pub coupled_world: Option<void_multiscale::SharedWorld>,
    pub environment: Arc<Environment>,
    pub grounds: Vec<GroundSpec>,
    pub terrains: BTreeMap<usize, Arc<Terrain>>,
}
impl WorldDescription {
    pub fn single(planet: &LandingPlanet, air: bool) -> Self {
        // `air` enables a body's configured atmosphere; it does not manufacture one on an
        // airless preset. This is the existing single-planet API's meaning.
        let air = air && planet.air_density_scale.is_some();
        let sea = planet.sea_level;
        let layered = matches!(planet.terrain_config, TerrainConfig::Layered(_));
        let color_datum = if layered {
            sea.expect("layered preset needs sea")
        } else if air {
            1800.0
        } else {
            0.0
        };
        let max = planet.terrain.max_height_meters;
        Self {
            schema: 5,
            system: planet.system.clone(),
            stellar: None,
            bodies: BTreeMap::from([(
                planet.body_id.clone(),
                BodyDescription {
                    label: planet.label.clone(),
                    terrain: Some(planet.terrain_config.clone()),
                    air_density_scale: if air { planet.air_density_scale } else { None },
                    air_datum_meters: planet.air_datum_meters(),
                    sea_level_meters: sea,
                    visual: VisualSettings {
                        surface: void_scenery::solar::SurfaceRecipe::SolidSurface,
                        rings: None,
                        surface_color: None,
                        atmosphere: air,
                        scattering: if air {
                            Some(
                                void_scenery::atmosphere_scene::AtmosphereProfile::EarthScaled {
                                    density_scale: planet
                                        .air_density_scale
                                        .expect("configured air"),
                                },
                            )
                        } else {
                            None
                        },
                        clouds: air,
                        cloud_profile: if air {
                            Some(void_scenery::atmosphere_scene::CloudProfile::earth())
                        } else {
                            None
                        },
                        ocean: sea.is_some(),
                        color_datum_meters: color_datum,
                        rock_height_meters: if layered {
                            color_datum + 2600.0
                        } else {
                            max * 0.5625
                        },
                        snow_height_meters: if layered {
                            color_datum + 4800.0
                        } else {
                            max * 0.75
                        },
                    },
                },
            )]),
        }
    }
    pub fn build(&self) -> BuiltWorld {
        self.build_with_coupled_checkpoint(None)
    }
    pub fn build_with_coupled_checkpoint(
        &self,
        saved: Option<void_multiscale::CoupledCheckpoint>,
    ) -> BuiltWorld {
        assert_eq!(self.schema, 5, "world: unsupported schema");
        let restoring_coupled = saved.is_some();
        let (mut ephemeris, coupled_world): (Box<dyn EphemerisSource>, _) =
            if let Some(stellar) = &self.stellar {
                let seeds = stellar.seeds(&self.system);
                let step = seeds
                    .iter()
                    .filter(|s| s.system.bodies.len() > 1)
                    .map(|s| suggested_step_seconds(&s.system.bodies, 256.0))
                    .fold(f64::INFINITY, f64::min);
                let step = if step.is_finite() { step } else { 60.0 };
                let world = match saved {
                    Some(saved) => void_multiscale::CoupledWorld::from_checkpoint(seeds, saved),
                    None => void_multiscale::CoupledWorld::new(seeds, step, 8192),
                };
                assert_eq!(
                    world.step_seconds, step,
                    "world checkpoint: coupled integration step changed"
                );
                assert_eq!(
                    world.sample_limit, 8192,
                    "world checkpoint: coupled history limit changed"
                );
                let shared = std::rc::Rc::new(std::cell::RefCell::new(world));
                let source = void_multiscale::FrameEphemeris::new(shared.clone(), &stellar.home.id);
                (Box::new(source), Some(shared))
            } else {
                assert!(saved.is_none(), "world: single system has coupled state");
                let system = build_system(&self.system);
                let step_seconds = if system.bodies.len() > 1 {
                    suggested_step_seconds(&system.bodies, 256.0)
                } else {
                    60.0
                };
                (
                    Box::new(Ephemeris::new(
                        &system,
                        EphemerisOptions {
                            step_seconds,
                            chunk_steps: 1024,
                        },
                    )),
                    None,
                )
            };
        if !restoring_coupled {
            ephemeris.extend_to(ephemeris.start_time() + ephemeris.step_seconds());
        }
        let mut environment = Environment::new(&ephemeris);
        let mut grounds = vec![];
        let mut terrains = BTreeMap::new();
        for (id, description) in &self.bodies {
            let body = ephemeris
                .bodies()
                .iter()
                .find(|b| &b.id == id)
                .unwrap_or_else(|| panic!("world: unknown body {id}"));
            assert!(
                description.air_datum_meters.is_finite()
                    && body.radius_meters + description.air_datum_meters > 0.0,
                "world: invalid air datum"
            );
            assert!(
                description.visual.color_datum_meters.is_finite()
                    && description.visual.rock_height_meters.is_finite()
                    && description.visual.snow_height_meters.is_finite(),
                "world: invalid visual heights"
            );
            description.visual.surface.validate();
            if matches!(
                description.visual.surface,
                void_scenery::solar::SurfaceRecipe::Regolith
            ) {
                assert!(
                    !description.visual.atmosphere
                        && !description.visual.clouds
                        && !description.visual.ocean
                        && description.air_density_scale.is_none()
                        && description.sea_level_meters.is_none(),
                    "regolith recipe requires an airless dry body"
                );
            }
            if matches!(
                description.visual.surface,
                void_scenery::solar::SurfaceRecipe::MartianRegolith
            ) {
                assert!(
                    matches!(description.terrain, Some(TerrainConfig::Ares(_)))
                        && !description.visual.ocean
                        && description.sea_level_meters.is_none(),
                    "Martian regolith requires dry Ares terrain"
                );
            }
            if let Some(rings) = &description.visual.rings {
                rings.validate();
            }
            match description.visual.surface {
                void_scenery::solar::SurfaceRecipe::SolidSurface
                | void_scenery::solar::SurfaceRecipe::Regolith
                | void_scenery::solar::SurfaceRecipe::MartianRegolith => {
                    assert!(description.terrain.is_some(), "solid recipe needs terrain")
                }
                _ => assert!(
                    description.terrain.is_none() && description.sea_level_meters.is_none(),
                    "gas/star recipe cannot have solid terrain or sea"
                ),
            }
            assert_eq!(
                description.visual.atmosphere,
                description.visual.scattering.is_some(),
                "enabled optical air needs profile"
            );
            if let Some(profile) = &description.visual.scattering {
                profile.parameters(body.radius_meters + description.air_datum_meters);
            }
            assert!(
                !description.visual.clouds || description.visual.atmosphere,
                "world: clouds without atmosphere"
            );
            assert!(
                !description.visual.clouds || description.visual.cloud_profile.is_some(),
                "world: enabled clouds need explicit profile"
            );
            if let Some(clouds) = &description.visual.cloud_profile {
                clouds.validate();
                let params = description
                    .visual
                    .scattering
                    .as_ref()
                    .expect("cloud profile needs optical air")
                    .parameters(body.radius_meters + description.air_datum_meters);
                assert!(
                    description.visual.color_datum_meters - description.air_datum_meters
                        + clouds.top_meters
                        < params.top_radius - params.bottom_radius,
                    "world: clouds above optical atmosphere"
                );
            }
            assert!(
                !description.visual.ocean || description.sea_level_meters.is_some(),
                "world: ocean without sea"
            );
            if description.visual.ocean {
                assert_eq!(
                    Some(description.visual.color_datum_meters),
                    description.sea_level_meters,
                    "world: rendered ocean must match physical sea"
                );
            }
            if let Some(color) = description.visual.surface_color {
                assert!(
                    color.iter().all(|x| x.is_finite() && *x >= 0.0),
                    "world: invalid surface color"
                );
            }
            if let Some(sea) = description.sea_level_meters {
                assert!(
                    sea.is_finite() && body.radius_meters + sea > 0.0,
                    "world: invalid sea"
                );
            }
            let atmosphere = description.air_density_scale.map(|scale| {
                assert!(scale.is_finite() && scale > 0.0, "world: invalid density");
                Atmosphere::Earth(EarthAtmosphere::new(scale))
            });
            let terrain = description.terrain.as_ref().map(|config| {
                match config {
                    TerrainConfig::Hills(h) => assert!(
                        h.radius_meters.is_finite()
                            && h.max_height_meters.is_finite()
                            && h.wavelength_meters.is_finite(),
                        "world: non-finite terrain"
                    ),
                    TerrainConfig::Cratered(_)
                    | TerrainConfig::Impact(_)
                    | TerrainConfig::Ares(_)
                    | TerrainConfig::Volcanic(_) => {} // constructor validates every parameter
                    TerrainConfig::Layered(l) => {
                        assert!(l.radius_meters.is_finite(), "world: non-finite terrain")
                    }
                }
                let t = Arc::new(Terrain::from_config(config));
                assert_eq!(
                    t.radius_meters, body.radius_meters,
                    "world: terrain radius mismatch for {id}"
                );
                terrains.insert(body.index, t.clone());
                grounds.push(GroundSpec {
                    body_index: body.index,
                    band_enter_meters: 200.0,
                    band_exit_meters: 400.0,
                    tiles: ContactWorldOptions {
                        step_seconds: 1.0 / 60.0,
                        tile_level: level_for_tile_size(t.radius_meters, 300.0),
                        tile_resolution: 33,
                        tile_reach_meters: 300.0,
                        tile_keep_meters: 600.0,
                        recenter_meters: 5000.0,
                        sleeping: true,
                    },
                });
                t
            });
            environment = environment.with(
                body.index,
                BodyEnvironment {
                    atmosphere,
                    air_datum_meters: description.air_datum_meters,
                    terrain,
                    sea_level_meters: description.sea_level_meters,
                },
            );
        }
        BuiltWorld {
            ephemeris,
            coupled_world,
            environment: Arc::new(environment),
            grounds,
            terrains,
        }
    }
    pub fn body_index(&self, id: &str) -> usize {
        let bodies = if let Some(stellar) = &self.stellar {
            let seeds = stellar.seeds(&self.system);
            seeds
                .into_iter()
                .flat_map(|seed| {
                    seed.system
                        .bodies
                        .into_iter()
                        .map(move |body| format!("{}/{}", seed.id, body.id))
                })
                .collect::<Vec<_>>()
        } else {
            build_system(&self.system)
                .bodies
                .into_iter()
                .map(|body| body.id)
                .collect()
        };
        bodies
            .iter()
            .position(|body| body == id)
            .unwrap_or_else(|| panic!("world: unknown body {id}"))
    }
    pub fn landing_planet(&self, id: &str) -> LandingPlanet {
        let d = self
            .bodies
            .get(id)
            .unwrap_or_else(|| panic!("world: unconfigured body {id}"));
        let config = d
            .terrain
            .as_ref()
            .expect("world: launch body needs terrain");
        LandingPlanet {
            label: d.label.clone(),
            system: self.system_for_body(id),
            body_id: id.into(),
            terrain_config: config.clone(),
            terrain: Arc::new(Terrain::from_config(config)),
            air_density_scale: d.air_density_scale,
            air_datum: d.air_datum_meters,
            sea_level: d.sea_level_meters,
        }
    }
    fn system_for_body(&self, body: &str) -> SystemSpec {
        let Some(stellar) = &self.stellar else {
            return self.system.clone();
        };
        let (system_id, _) = body
            .split_once('/')
            .expect("stellar body needs system qualifier");
        let mut spec = if system_id == stellar.home.id {
            self.system.clone()
        } else {
            stellar
                .neighbors
                .iter()
                .find(|s| s.placement.id == system_id)
                .expect("world: unknown system")
                .system
                .clone()
        };
        fn qualify(node: &mut void_orbit::BodySpec, system: &str) {
            node.id = format!("{system}/{}", node.id);
            for child in &mut node.children {
                qualify(child, system);
            }
        }
        qualify(&mut spec.root, system_id);
        spec
    }
    /// Deterministic acceptance-fixture site on real dry terrain in this body's own daylight.
    /// Does not change terrain or illumination, and does not modify ordinary launch defaults.
    pub fn daylight_terrain_site(&self, id: &str) -> Result<DVec3, String> {
        let body = self.body_index(id);
        let built = self.build();
        let terrain = built
            .terrains
            .get(&body)
            .ok_or("daylight fixture needs authored solid terrain")?;
        let source = built.ephemeris.as_ref();
        let star = source
            .bodies()
            .iter()
            .find(|b| {
                b.parent_index.is_none() && source.system_of(b.index) == source.system_of(body)
            })
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
        let sea = self.bodies[id].sea_level_meters;
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
    pub fn validate_launch(&self, id: &str, site: DVec3) {
        self.body_index(id);
        self.landing_planet(id);
        assert!(
            site.is_finite() && (site.length() - 1.0).abs() < 1e-9,
            "world: launch site must be unit direction"
        );
    }
}

/// Earth analogue and its real orbital moon, in the same ephemeris and collision world.
pub fn aurelia_selene(planet: &LandingPlanet) -> WorldDescription {
    assert_eq!(planet.body_id, "aurelia", "world: preset needs Aurelia");
    let mut world = WorldDescription::single(planet, true);
    let moon = void_landing::moon_size();
    world.bodies.insert(
        "selene".into(),
        BodyDescription {
            label: "SELENE · AIRLESS MOON".into(),
            terrain: Some(moon.terrain_config),
            air_density_scale: None,
            air_datum_meters: 0.0,
            sea_level_meters: None,
            visual: VisualSettings {
                surface: void_scenery::solar::SurfaceRecipe::SolidSurface,
                rings: None,
                surface_color: Some([0.25, 0.25, 0.25]),
                atmosphere: false,
                scattering: None,
                clouds: false,
                cloud_profile: None,
                ocean: false,
                color_datum_meters: 0.0,
                rock_height_meters: 1.0,
                snow_height_meters: 1.0e9,
            },
        },
    );
    world
}

fn unique_bodies<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, BodyDescription>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = BTreeMap<String, BodyDescription>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("unique body IDs")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((id, body)) = map.next_entry::<String, BodyDescription>()? {
                if result.insert(id.clone(), body).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate body ID {id}")));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Unique)
}

/// Authored first-pass solar scenery used by the main-game Aurelia preset.
/// Physical density is unchanged for Aurelia; other optical air is visual only.
pub fn solar_scenery(planet: &LandingPlanet) -> WorldDescription {
    use void_scenery::{
        atmosphere_scene::{AtmosphereProfile, CloudProfile},
        solar::{RingRecipe, SurfaceRecipe},
    };
    use void_terrain::CrateredOptions;
    let mut world = WorldDescription::single(planet, true);
    let system = build_system(&world.system);
    for id in [
        "sol", "cinder", "vesper", "ares", "selene", "velvet", "halo", "azure", "abyss",
    ] {
        let body = system
            .bodies
            .iter()
            .find(|b| b.id == id)
            .expect("solar body");
        let mut visual = VisualSettings {
            surface: SurfaceRecipe::SolidSurface,
            rings: None,
            surface_color: None,
            atmosphere: false,
            scattering: None,
            clouds: false,
            cloud_profile: None,
            ocean: false,
            color_datum_meters: 0.0,
            rock_height_meters: 1.0e8,
            snow_height_meters: 1.0e9,
        };
        let terrain = match id {
            "cinder" => {
                visual.surface = SurfaceRecipe::Regolith;
                Some(TerrainConfig::Impact(void_terrain::ImpactOptions::cinder(
                    body.radius_meters,
                )))
            }
            "ares" => {
                visual.surface = SurfaceRecipe::MartianRegolith;
                Some(TerrainConfig::Ares(void_terrain::AresOptions::ares(
                    body.radius_meters,
                )))
            }
            "vesper" => Some(TerrainConfig::Volcanic(
                void_terrain::VolcanicOptions::vesper(body.radius_meters),
            )),
            "selene" => {
                let (height, count, size, roughness, seed, low, high) = match id {
                    "selene" => (
                        8500.0,
                        120,
                        0.16,
                        0.5,
                        19,
                        [0.07, 0.075, 0.08],
                        [0.56, 0.55, 0.52],
                    ),
                    _ => unreachable!(),
                };
                Some(TerrainConfig::Cratered(CrateredOptions {
                    name: format!("{id} impact terrain"),
                    radius_meters: body.radius_meters,
                    max_height_meters: height,
                    crater_count: count,
                    crater_radius_radians: size,
                    roughness,
                    seed,
                    low_color: low,
                    high_color: high,
                }))
            }
            "sol" => {
                visual.surface = SurfaceRecipe::EmissiveStar {
                    color: [1.0, 0.65, 0.28],
                    radiance: 6.0,
                    granulation: 0.6,
                };
                None
            }
            "velvet" | "halo" | "azure" | "abyss" => {
                let (low, high, bands, turbulence, storm) = match id {
                    "velvet" => ([0.26, 0.10, 0.045], [0.82, 0.66, 0.44], 18.0, 1.0, 1.0),
                    "halo" => ([0.35, 0.25, 0.12], [0.80, 0.69, 0.43], 24.0, 0.3, 0.0),
                    "azure" => ([0.12, 0.40, 0.43], [0.32, 0.68, 0.69], 8.0, 0.12, 0.0),
                    "abyss" => ([0.025, 0.06, 0.28], [0.12, 0.32, 0.72], 12.0, 0.8, 0.7),
                    _ => unreachable!(),
                };
                visual.surface = SurfaceRecipe::GasEnvelope {
                    low,
                    high,
                    bands,
                    turbulence,
                    storm,
                };
                if id == "halo" {
                    visual.rings = Some(RingRecipe {
                        inner_radius: 1.25,
                        outer_radius: 2.35,
                        color: [0.65, 0.53, 0.34],
                        opacity: 0.8,
                    });
                }
                None
            }
            _ => unreachable!(),
        };
        if id == "vesper" || id == "ares" {
            let venus = id == "vesper";
            visual.atmosphere = true;
            visual.scattering = Some(AtmosphereProfile::Custom {
                height_meters: if venus { 120000.0 } else { 80000.0 },
                rayleigh_scattering: if venus {
                    [85e-6, 150e-6, 280e-6]
                } else {
                    // Effective dust colour in the existing RGB scattering profile, not a
                    // molecular CO2 Rayleigh fit. Clear, dusty air; no global dust storm.
                    [12e-6, 6.5e-6, 3.4e-6]
                },
                rayleigh_scale_height: if venus { 15000.0 } else { 11000.0 },
                mie_scattering: if venus { 5e-6 } else { 3e-6 },
                mie_extinction: if venus { 6e-6 } else { 4.5e-6 },
                mie_scale_height: if venus { 18000.0 } else { 8000.0 },
                mie_anisotropy: 0.7,
                ozone_absorption: [0.0; 3],
                ozone_center_height: 0.0,
                ozone_width: 1.0,
            });
            if venus {
                visual.clouds = true;
                visual.cloud_profile = Some(CloudProfile::vesper());
            }
        }
        world.bodies.insert(
            id.into(),
            BodyDescription {
                label: format!("{} · SOLAR SCENERY", body.name),
                terrain,
                air_density_scale: None,
                air_datum_meters: 0.0,
                sea_level_meters: None,
                visual,
            },
        );
    }
    world
}

/// Authored fictional neighborhood at real stellar separations. This is not a transfer
/// fixture: the ordinary launch craft starts landed with its ordinary resources and speed.
pub fn stellar_neighborhood(planet: &LandingPlanet) -> WorldDescription {
    let mut world = solar_scenery(planet);
    let home = SystemPlacement {
        id: "Sol".into(),
        origin: void_multiscale::default_galaxy(),
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

/// Main-game exploration catalog. The golden/lab solar scenery remains a separate fixture.
/// New solid surfaces are explicitly authored spheres with small procedural crater relief;
/// no measured irregular shape, Titan air, comet coma/tail or volatile physics is modeled.
pub fn expanded_solar_scenery(planet: &LandingPlanet) -> WorldDescription {
    use void_scenery::solar::SurfaceRecipe;
    let mut world = solar_scenery(planet);
    let mut expanded = void_orbit::expanded_sol();
    // Preserve the selected home preset's spin (including deliberate fast-spin fixtures).
    let home = world
        .system
        .root
        .children
        .iter()
        .find(|b| b.id == "aurelia")
        .expect("expanded scenery requires Aurelia Sol preset");
    expanded
        .root
        .children
        .iter_mut()
        .find(|b| b.id == "aurelia")
        .expect("expanded catalog Aurelia")
        .rotation = home.rotation;
    world.system = expanded;
    for body in build_system(&world.system).bodies {
        if world.bodies.contains_key(&body.id) {
            continue;
        }
        // Stable IDs also seed the appearance, independent of traversal/body index.
        let seed = body
            .id
            .bytes()
            .fold(0_u32, |s, b| s.wrapping_mul(31).wrapping_add(b as u32));
        let height = (body.radius_meters * 0.01).min(2500.0);
        // Authored palettes convey the major surface identities, not calibrated spectra.
        let (low, high) = match body.id.as_str() {
            "ember" => ([0.30, 0.12, 0.025], [0.85, 0.70, 0.22]),
            "rime" | "enceladus" | "tethys" | "miranda" | "triton" => {
                ([0.25, 0.27, 0.28], [0.83, 0.81, 0.73])
            }
            "haze" => ([0.26, 0.12, 0.025], [0.65, 0.42, 0.13]),
            "pluto" => ([0.20, 0.09, 0.055], [0.73, 0.68, 0.61]),
            "halley" | "67p" | "encke" | "halebopp" | "bennu" | "ryugu" => {
                ([0.025, 0.025, 0.03], [0.13, 0.12, 0.11])
            }
            "eris" | "haumea" | "makemake" => ([0.25, 0.23, 0.22], [0.78, 0.74, 0.67]),
            _ => ([0.12, 0.12, 0.13], [0.48, 0.46, 0.43]),
        };
        world.bodies.insert(
            body.id.clone(),
            BodyDescription {
                label: format!(
                    "{} · {:.2} km RADIUS",
                    body.name,
                    body.radius_meters / 1000.0
                ),
                terrain: Some(TerrainConfig::Cratered(void_terrain::CrateredOptions {
                    name: format!("{} authored cratered sphere", body.id),
                    radius_meters: body.radius_meters,
                    max_height_meters: height,
                    crater_count: 24,
                    crater_radius_radians: 0.12,
                    roughness: 0.35,
                    seed,
                    low_color: low,
                    high_color: high,
                })),
                air_density_scale: None,
                air_datum_meters: 0.0,
                sea_level_meters: None,
                visual: VisualSettings {
                    surface: SurfaceRecipe::SolidSurface,
                    rings: None,
                    surface_color: None,
                    atmosphere: false,
                    scattering: None,
                    clouds: false,
                    cloud_profile: None,
                    ocean: false,
                    color_datum_meters: 0.0,
                    rock_height_meters: height * 2.0,
                    snow_height_meters: 1.0e9,
                },
            },
        );
    }
    world
}

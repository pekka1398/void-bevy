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

/// The collision tiles of a body's ground scene. The drawn terrain is built from the same options
/// (`void_landing::landing_lod_options`), so what is drawn is what is collided with.
pub fn ground_tiles(terrain: &Terrain) -> ContactWorldOptions {
    ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(terrain.radius_meters, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 5000.0,
        sleeping: true,
    }
}

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
                    tiles: ground_tiles(&t),
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
    pub fn validate_launch(&self, id: &str, site: DVec3) {
        self.body_index(id);
        self.landing_planet(id);
        assert!(
            site.is_finite() && (site.length() - 1.0).abs() < 1e-9,
            "world: launch site must be unit direction"
        );
    }
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

/// The main game's world: the authored Sol system with Aurelia's layered terrain and sea,
/// starting landed at the Aurelia launch site with full aerodynamic forces and torques.
pub fn main_game(craft: &void_assembly::Craft) -> crate::session::InitialWorld {
    let system_spec = SystemSpec::sol();
    let system = build_system(&system_spec);
    let body = |id: &str| {
        system
            .bodies
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("main game: Sol has no {id}"))
    };
    let aurelia = body("aurelia");
    let terrain_config = TerrainConfig::Layered(void_terrain::LayeredOptions {
        radius_meters: aurelia.radius_meters,
        ..void_terrain::DEFAULT_LAYERED
    });
    // Dry lowland on the layered terrain.
    let site = crate::placement::unit_site(0.3_f64.to_degrees(), 0.5_f64.to_degrees());
    assert!(
        Terrain::from_config(&terrain_config).height(site) > void_terrain::SEA_LEVEL,
        "main game: launch site is underwater"
    );
    let sea = void_terrain::SEA_LEVEL;
    let mut bodies = BTreeMap::from([(
        "aurelia".to_string(),
        BodyDescription {
            label: format!(
                "AURELIA · SOL SYSTEM · {:.0} km RADIUS · {:.2} m/s² · {:.1} h DAY",
                aurelia.radius_meters / 1e3,
                aurelia.gm / aurelia.radius_meters.powi(2),
                aurelia.rotation.period_seconds / 3600.0
            ),
            terrain: Some(terrain_config),
            air_density_scale: Some(1.0),
            air_datum_meters: sea,
            sea_level_meters: Some(sea),
            visual: VisualSettings {
                surface: void_scenery::solar::SurfaceRecipe::SolidSurface,
                rings: None,
                surface_color: None,
                atmosphere: true,
                scattering: Some(
                    void_scenery::atmosphere_scene::AtmosphereProfile::EarthScaled {
                        density_scale: 1.0,
                    },
                ),
                clouds: true,
                cloud_profile: Some(void_scenery::atmosphere_scene::CloudProfile::earth()),
                ocean: true,
                color_datum_meters: sea,
                rock_height_meters: sea + 2600.0,
                snow_height_meters: sea + 4800.0,
            },
        },
    )]);
    for id in [
        "sol", "cinder", "vesper", "ares", "selene", "velvet", "halo", "azure", "abyss",
    ] {
        bodies.insert(id.into(), solar_body(id, body(id)));
    }
    crate::session::InitialWorld {
        air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
        world: WorldDescription {
            schema: 5,
            system: system_spec,
            stellar: None,
            bodies,
        },
        launch_body: "aurelia".into(),
        craft: craft.clone(),
        launch_site: site,
    }
}

/// How the main game draws and grounds one of Sol's other bodies. Physical air is Aurelia's
/// alone; the atmospheres here are visual only.
fn solar_body(id: &str, body: &void_orbit::CelestialBody) -> BodyDescription {
    use void_scenery::{
        atmosphere_scene::{AtmosphereProfile, CloudProfile},
        solar::{RingRecipe, SurfaceRecipe},
    };
    use void_terrain::CrateredOptions;
    {
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
            "selene" => Some(TerrainConfig::Cratered(CrateredOptions {
                name: format!("{id} impact terrain"),
                radius_meters: body.radius_meters,
                max_height_meters: 8500.0,
                crater_count: 120,
                crater_radius_radians: 0.16,
                roughness: 0.5,
                seed: 19,
                low_color: [0.07, 0.075, 0.08],
                high_color: [0.56, 0.55, 0.52],
            })),
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
            other => panic!("main game: no scenery for {other}"),
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
        BodyDescription {
            label: format!("{} · SOLAR SCENERY", body.name),
            terrain,
            air_density_scale: None,
            air_datum_meters: 0.0,
            sea_level_meters: None,
            visual,
        }
    }
}

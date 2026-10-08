//! Serializable world configuration shared by physics and rendering. No renderer handles.
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use void_environment::{Atmosphere, BodyEnvironment, EarthAtmosphere, Environment};
use void_landing::{ContactWorldOptions, LandingPlanet, level_for_tile_size};
use void_orbit::{Ephemeris, EphemerisOptions, SystemSpec, build_system, suggested_step_seconds};
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDescription {
    pub schema: u32,
    pub system: SystemSpec,
    #[serde(deserialize_with = "unique_bodies")]
    pub bodies: BTreeMap<String, BodyDescription>,
}
pub struct BuiltWorld {
    pub ephemeris: Ephemeris,
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
            schema: 3,
            system: planet.system.clone(),
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
        assert_eq!(self.schema, 3, "world: unsupported schema");
        let system = build_system(&self.system);
        let step_seconds = if system.bodies.len() > 1 {
            suggested_step_seconds(&system.bodies, 256.0)
        } else {
            60.0
        };
        let mut ephemeris = Ephemeris::new(
            &system,
            EphemerisOptions {
                step_seconds,
                chunk_steps: 1024,
            },
        );
        ephemeris.extend_to(step_seconds);
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
            if let Some(rings) = &description.visual.rings {
                rings.validate();
            }
            match description.visual.surface {
                void_scenery::solar::SurfaceRecipe::SolidSurface
                | void_scenery::solar::SurfaceRecipe::Regolith => {
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
                    TerrainConfig::Cratered(_) | TerrainConfig::Impact(_) => {} // constructor validates every parameter
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
            environment: Arc::new(environment),
            grounds,
            terrains,
        }
    }
    pub fn body_index(&self, id: &str) -> usize {
        build_system(&self.system)
            .bodies
            .iter()
            .position(|b| b.id == id)
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
            system: self.system.clone(),
            body_id: id.into(),
            terrain_config: config.clone(),
            terrain: Arc::new(Terrain::from_config(config)),
            air_density_scale: d.air_density_scale,
            air_datum: d.air_datum_meters,
            sea_level: d.sea_level_meters,
        }
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
            "selene" | "ares" | "vesper" => {
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
                    "ares" => (
                        14000.0,
                        48,
                        0.20,
                        1.0,
                        47,
                        [0.13, 0.035, 0.014],
                        [0.65, 0.25, 0.09],
                    ),
                    "vesper" => (
                        9000.0,
                        22,
                        0.12,
                        1.0,
                        73,
                        [0.10, 0.07, 0.02],
                        [0.48, 0.34, 0.12],
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
                    [18e-6, 15e-6, 8e-6]
                } else {
                    [0.5e-6, 0.3e-6, 0.15e-6]
                },
                rayleigh_scale_height: if venus { 16000.0 } else { 11000.0 },
                mie_scattering: if venus { 12e-6 } else { 0.4e-6 },
                mie_extinction: if venus { 15e-6 } else { 0.5e-6 },
                mie_scale_height: if venus { 18000.0 } else { 8000.0 },
                mie_anisotropy: 0.7,
                ozone_absorption: [0.0; 3],
                ozone_center_height: 0.0,
                ozone_width: 1.0,
            });
            if venus {
                visual.clouds = true;
                visual.cloud_profile = Some(CloudProfile {
                    bottom_meters: 45000.0,
                    top_meters: 70000.0,
                    extinction_per_meter: 0.0008,
                    coverage: 0.96,
                });
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

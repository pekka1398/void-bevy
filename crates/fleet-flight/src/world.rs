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
            schema: 2,
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
        assert_eq!(self.schema, 2, "world: unsupported schema");
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
            assert!(
                !description.visual.atmosphere || description.air_density_scale.is_some(),
                "world: visual atmosphere without physical atmosphere"
            );
            assert_eq!(
                description.visual.scattering.is_some(),
                description.air_density_scale.is_some(),
                "world: optical profile must explicitly match atmosphere presence"
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

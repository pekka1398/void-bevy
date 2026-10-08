//! Shared production world scenery. Owns GPU assets and terrain jobs; holds no flight session.
use crate::{
    air::{AirLayers, AirSettings, AirTextures},
    scenery::{GroundMaterial, GroundUniforms, table_image},
    tiles::TileField,
};
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use std::collections::HashMap;
use void_fleet_flight::{FleetFlight, presentation::CameraSample};
#[derive(Component)]
pub struct FarBody(
    pub usize,
    pub AssetId<Mesh>,
    pub AssetId<StandardMaterial>,
    pub bool,
);
pub struct BodyScene {
    pub field: TileField<GroundMaterial>,
    pub material: Handle<GroundMaterial>,
    pub color: Option<[f32; 3]>,
}
pub struct AirView<'a> {
    pub camera: &'a Transform,
    pub projection: &'a PerspectiveProjection,
    pub focal: f64,
}
pub struct AtmosphericBody {
    pub top_radius: f64,
    pub air: AirSettings,
    pub textures: AirTextures,
}
struct Appearance {
    terrain: std::sync::Arc<void_terrain::Terrain>,
    color: Option<[f32; 3]>,
}
impl void_lod::SurfaceSampler for Appearance {
    fn sample(&self, d: DVec3, cell: f64) -> void_lod::SurfaceSample {
        let (height, color) = self.terrain.sample(d, Some(cell));
        void_lod::SurfaceSample {
            height_meters: height,
            color: self.color.unwrap_or(color.map(|v| v as f32)),
        }
    }
}
type BuiltScenes = (
    HashMap<usize, BodyScene>,
    HashMap<usize, AtmosphericBody>,
    Vec<AssetId<Image>>,
);
pub struct WorldScenery {
    pub bodies: HashMap<usize, BodyScene>,
    pub atmospheres: HashMap<usize, AtmosphericBody>,
    pub world: void_fleet_flight::world::WorldDescription,
    pub images: Vec<AssetId<Image>>,
    pub active: usize,
    eye: DVec3,
    observer: Option<DVec3>,
    rotation: DQuat,
}
pub fn build_scenes(
    commands: &mut Commands,
    sim: &void_fleet_flight::FleetFlight,
    grounds: &mut Assets<GroundMaterial>,
    images: &mut Assets<Image>,
) -> BuiltScenes {
    use void_scenery::atmosphere::*;
    use void_scenery::clouds::*;
    use void_scenery::tables::*;
    let world = sim.world.clone();
    let before = images
        .iter()
        .map(|(id, _)| id)
        .collect::<std::collections::HashSet<_>>();
    let mut scenes = HashMap::new();
    let mut atmospheres = HashMap::new();
    // Shared deterministic noise assets; per-body coverage/optics remain independent.
    let weather = images.add(crate::air::weather_image(
        build_cloud_weather(2),
        WEATHER_WIDTH,
        WEATHER_HEIGHT,
    ));
    let shape = images.add(crate::air::noise_volume_image(
        build_cloud_noise(SHAPE_SIZE, false),
        SHAPE_SIZE,
    ));
    let detail = images.add(crate::air::noise_volume_image(
        build_cloud_noise(DETAIL_SIZE, true),
        DETAIL_SIZE,
    ));
    let mut resolve_textures = None;
    for (id, d) in &world.bodies {
        let body = world.body_index(id);
        let radius = sim.fleet.ephemeris.bodies()[body].radius_meters + d.air_datum_meters;
        let params = if let Some(profile) = &d.visual.scattering {
            profile.parameters(radius)
        } else {
            // Explicit vacuum tables for mandatory ground/resolve bindings, never Earth air.
            let mut p = void_scenery::earth_like_atmosphere(radius);
            p.rayleigh_scattering = [0.0; 3];
            p.ozone_absorption = [0.0; 3];
            p.mie_scattering = 0.0;
            p.mie_extinction = 0.0;
            p
        };
        let trans = build_transmittance_table(&params);
        let multiple = build_multiple_scattering_table(&params, &trans, 64, 20);
        let irradiance = build_irradiance_table(&params, &trans, &multiple, 128, 24);
        let trans = images.add(table_image(
            &trans,
            TRANSMITTANCE_WIDTH,
            TRANSMITTANCE_HEIGHT,
        ));
        let irradiance = images.add(table_image(
            &irradiance,
            IRRADIANCE_WIDTH,
            IRRADIANCE_HEIGHT,
        ));
        let textures = AirTextures {
            transmittance: trans.clone(),
            irradiance: irradiance.clone(),
            multiple: images.add(table_image(
                &multiple,
                MULTIPLE_SCATTERING_SIZE,
                MULTIPLE_SCATTERING_SIZE,
            )),
            weather: weather.clone(),
            shape: shape.clone(),
            detail: detail.clone(),
        };
        if resolve_textures.is_none() {
            resolve_textures = Some(textures.clone());
        }
        let mut air = AirSettings::new(&params);
        air.enabled = f32::from(u8::from(d.visual.atmosphere));
        air.clouds_enabled = f32::from(u8::from(d.visual.clouds));
        if let Some(clouds) = &d.visual.cloud_profile {
            air.cloud_bottom = clouds.bottom_meters as f32;
            air.cloud_top = clouds.top_meters as f32;
            air.cloud_extinction = clouds.extinction_per_meter as f32;
            air.coverage = clouds.coverage as f32;
        }
        air.exposure = 1.0;
        air.tone_mapping = crate::air::ToneMapping::None as u8 as f32;
        air.sun_disc_enabled = 0.0;
        air.sea_level = (d.visual.color_datum_meters - d.air_datum_meters) as f32;
        if d.visual.atmosphere {
            atmospheres.insert(
                body,
                AtmosphericBody {
                    top_radius: params.top_radius,
                    air,
                    textures,
                },
            );
        }
        if d.terrain.is_none() {
            continue;
        }
        let terrain = sim.terrains[&body].clone();
        let mut uniforms = GroundUniforms::new(
            &params,
            d.visual.color_datum_meters,
            d.visual.rock_height_meters,
            d.visual.snow_height_meters,
        );
        uniforms.regolith = f32::from(u8::from(matches!(
            d.visual.surface,
            void_scenery::solar::SurfaceRecipe::Regolith
        )));
        if let void_terrain::TerrainConfig::Impact(options) = terrain.config() {
            uniforms.impact_seed = options.seed;
            uniforms.impact_density = options.crater_density as f32;
            uniforms.plains_fraction = options.plains_fraction as f32;
            uniforms.impact_basin_count = options.basins.len() as u32;
            let color = |c: [f64; 3], w: f32| Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, w);
            uniforms.impact_mature =
                color(options.mature_color, options.rayed_impacts.len() as f32);
            uniforms.impact_plain_color = color(options.plains_color, 0.0);
            uniforms.impact_fresh = color(options.fresh_color, 1.0);
            for (i, ray) in options.rayed_impacts.iter().enumerate() {
                uniforms.impact_rays[i] = color(
                    ray.direction,
                    (ray.radius_meters / options.radius_meters) as f32,
                );
                uniforms.impact_ray_params[i] = Vec4::new(
                    ray.freshness as f32,
                    (ray.ray_seed % 10007) as f32,
                    0.0,
                    0.0,
                );
            }
            for (i, basin) in options.basins.iter().enumerate() {
                uniforms.impact_basins[i] = Vec4::new(
                    basin.direction[0] as f32,
                    basin.direction[1] as f32,
                    basin.direction[2] as f32,
                    (basin.radius_meters / options.radius_meters) as f32,
                );
                uniforms.impact_fills[i / 4][i % 4] = basin.fill as f32;
            }
        }
        uniforms.ocean_enabled = f32::from(u8::from(d.visual.ocean));
        uniforms.atmosphere_enabled = f32::from(u8::from(d.visual.atmosphere));
        let material = grounds.add(GroundMaterial {
            ground: uniforms,
            transmittance: trans,
            irradiance,
        });
        let demo = void_landing::demo_rocket(&terrain);
        let mut field = TileField::new(
            void_landing::landing_lod_options(&terrain, &demo.options.contact),
            Some(std::sync::Arc::new(Appearance {
                terrain,
                color: d.visual.surface_color,
            })),
            material.clone(),
        );
        field.no_frustum_culling = true;
        scenes.insert(
            body,
            BodyScene {
                field,
                material,
                color: d.visual.surface_color,
            },
        );
    }
    commands.insert_resource(resolve_textures.expect("world scenery needs a configured body"));
    let owned = images
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    (scenes, atmospheres, owned)
}
fn spawn_far(
    commands: &mut Commands,
    sim: &void_fleet_flight::FleetFlight,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
) {
    use void_scenery::solar::SurfaceRecipe;
    for body in sim.fleet.ephemeris.bodies() {
        let descriptor = sim.world.bodies.get(&body.id);
        let mut mesh = Sphere::new(1.0).mesh().uv(256, 128);
        let mut positions = match mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("sphere positions")
        {
            bevy::mesh::VertexAttributeValues::Float32x3(p) => p.clone(),
            _ => panic!("sphere position format"),
        };
        // Bevy sphere uses y poles; body-fixed spin axis is z.
        for p in &mut positions {
            *p = [p[0], p[2], -p[1]];
        }
        let colors = positions
            .iter_mut()
            .map(|p| {
                let d = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).normalize();
                if let Some(descriptor) = descriptor {
                    match &descriptor.visual.surface {
                        SurfaceRecipe::SolidSurface | SurfaceRecipe::Regolith => {
                            let terrain = &sim.terrains[&body.index];
                            let (h, c) = terrain.sample(d, None);
                            *p = (d * (1.0 + h / body.radius_meters)).as_vec3().to_array();
                            let c = descriptor
                                .visual
                                .surface_color
                                .unwrap_or(c.map(|v| v as f32));
                            [c[0], c[1], c[2], 1.0]
                        }
                        recipe => recipe.color(d),
                    }
                } else {
                    // System bodies without authored scenery keep the pre-existing map sphere.
                    let c = crate::map::color(&body.color).to_linear();
                    [c.red, c.green, c.blue, 1.0]
                }
            })
            .collect::<Vec<_>>();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.compute_smooth_normals();
        let is_star = descriptor.map_or(body.parent_index.is_none(), |d| {
            matches!(d.visual.surface, SurfaceRecipe::EmissiveStar { .. })
        });
        let material = StandardMaterial {
            base_color: Color::WHITE,
            unlit: is_star,
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        };
        let mesh = meshes.add(mesh);
        let material = standard.add(material);
        commands.spawn((
            FarBody(body.index, mesh.id(), material.id(), false),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
        ));
        if descriptor
            .is_some_and(|d| matches!(d.visual.surface, SurfaceRecipe::EmissiveStar { .. }))
        {
            // Thin optically transparent emission shells: a visual corona, no solid/collider.
            for layer in 0..48 {
                let radius = 1.006 + f32::from(layer as u8) * 0.006;
                let mesh = meshes.add(Sphere::new(radius).mesh().uv(64, 32));
                let alpha = 0.004 * (1.0 - f32::from(layer as u8) / 48.0);
                let material = standard.add(StandardMaterial {
                    base_color: Color::linear_rgba(1.0, 0.48, 0.12, alpha),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                });
                commands.spawn((
                    FarBody(body.index, mesh.id(), material.id(), true),
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::default(),
                ));
            }
        }
        if let Some(ring) = descriptor.and_then(|d| d.visual.rings.as_ref()) {
            let mesh = meshes.add(crate::solar_mesh::rings(ring));
            let material = standard.add(StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                perceptual_roughness: 1.0,
                ..default()
            });
            commands.spawn((
                FarBody(body.index, mesh.id(), material.id(), true),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::default(),
            ));
        }
    }
}

impl WorldScenery {
    pub fn new(
        commands: &mut Commands,
        sim: &FleetFlight,
        grounds: &mut Assets<GroundMaterial>,
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
        standard: &mut Assets<StandardMaterial>,
    ) -> Self {
        let (bodies, atmospheres, owned) = build_scenes(commands, sim, grounds, images);
        spawn_far(commands, sim, meshes, standard);
        Self {
            bodies,
            atmospheres,
            world: sim.world.clone(),
            images: owned,
            active: sim.observation_body(),
            eye: DVec3::ZERO,
            observer: None,
            rotation: DQuat::IDENTITY,
        }
    }
    pub fn unload(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        grounds: &mut Assets<GroundMaterial>,
        images: &mut Assets<Image>,
    ) {
        for b in self.bodies.values_mut() {
            b.field.unload(commands, meshes);
            grounds.remove(b.material.id());
        }
        for id in self.images.drain(..) {
            images.remove(id);
        }
    }
    pub fn prepare(
        &mut self,
        sim: &FleetFlight,
        sample: &CameraSample,
        axes: DQuat,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) {
        let body = sim.observation_body();
        if body != self.active {
            for (&index, b) in &mut self.bodies {
                b.field.unload(commands, meshes);
                let terrain = sim.terrains[&index].clone();
                let demo = void_landing::demo_rocket(&terrain);
                b.field = TileField::new(
                    void_landing::landing_lod_options(&terrain, &demo.options.contact),
                    Some(std::sync::Arc::new(Appearance {
                        terrain,
                        color: b.color,
                    })),
                    b.material.clone(),
                );
                b.field.no_frustum_culling = true;
            }
            self.active = body;
        }
        let into = sample.to_camera(&sim.fleet, sim.fleet.body_frames(body).1, axes);
        self.rotation = into.rotation();
        assert!(
            self.rotation.dot(DQuat::IDENTITY).abs() > 1.0 - 1e-12,
            "terrain shader requires observed-body render axes"
        );
        self.eye = -(self.rotation.conjugate() * into.apply_point(DVec3::ZERO));
        self.observer = if sim.presentation.focus_body.is_none()
            && sim.navigation_body(&sim.selected) == body
        {
            Some(
                sim.fleet
                    .frames()
                    .transform(
                        sim.fleet.vessel_frame(&sim.selected),
                        sim.fleet.body_frames(body).1,
                    )
                    .apply_point(sim.fleet.root_position_local(&sim.selected)),
            )
        } else {
            None
        };
    }
    pub fn finish_builds(&mut self) {
        if let Some(b) = self.bodies.get_mut(&self.active) {
            b.field.finish_builds();
        }
    }
    pub fn readiness(&self) -> (usize, usize, usize, usize) {
        match self.bodies.get(&self.active) {
            Some(b) => (
                b.field.building_count(),
                b.field.last_requests,
                b.field.drawn_count(),
                b.field.lod.cached_mesh_bytes(),
            ),
            None => (0, 0, 0, 0),
        }
    }
    pub fn max_level(&self) -> u32 {
        match self.bodies.get(&self.active) {
            Some(b) => b.field.lod.options.max_level,
            None => 0,
        }
    }
    pub fn wireframe(&self) -> bool {
        self.bodies
            .get(&self.active)
            .is_some_and(|b| b.field.wireframe())
    }
    pub fn set_wireframe(&mut self, commands: &mut Commands, on: bool) {
        if let Some(b) = self.bodies.get_mut(&self.active) {
            b.field.set_wireframe(commands, on);
        }
    }
    pub fn select(&mut self, view: &void_lod::LodView) {
        if let Some(b) = self.bodies.get_mut(&self.active) {
            let mut view = view.clone();
            if let Some(camera) = &mut view.camera {
                camera.position = self.eye;
                camera.max_level = b.field.lod.options.max_level;
            }
            view.observer_positions = vec![self.eye];
            view.observer_positions.extend(self.observer);
            b.field.select(&view);
        }
    }
    pub fn boundaries(&self) -> Vec<Vec<Vec3>> {
        match self.bodies.get(&self.active) {
            Some(b) => b
                .field
                .boundaries(self.eye)
                .into_iter()
                .map(|line| {
                    line.into_iter()
                        .map(|p| (self.rotation * p.as_dvec3()).as_vec3())
                        .collect()
                })
                .collect(),
            None => vec![],
        }
    }
    pub fn draw<F: bevy::ecs::query::QueryFilter>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        tiles: &mut Query<&mut Transform, F>,
    ) {
        if let Some(b) = self.bodies.get_mut(&self.active) {
            b.field.draw(commands, meshes, tiles, self.eye);
        }
    }
    pub fn update_air(
        &self,
        sim: &FleetFlight,
        sample: &CameraSample,
        axes: DQuat,
        view: AirView<'_>,
        grounds: &mut Assets<GroundMaterial>,
    ) -> (AirSettings, AirLayers, DVec3) {
        let AirView {
            camera,
            projection,
            focal,
        } = view;
        let fleet = &sim.fleet;
        let sun_local = |body, eye: DVec3| {
            let root = fleet
                .ephemeris
                .bodies()
                .iter()
                .find(|b| {
                    b.parent_index.is_none()
                        && fleet.ephemeris.system_of(b.index) == fleet.ephemeris.system_of(body)
                })
                .expect("world system star");
            if root.index == sim.home {
                fleet
                    .frames()
                    .transform(fleet.origin_frame(), fleet.body_frames(body).1)
                    .apply_direction(DVec3::X)
            } else {
                let v = fleet
                    .frames()
                    .transform(fleet.body_frames(root.index).0, fleet.body_frames(body).1)
                    .apply_point(DVec3::ZERO)
                    - eye;
                assert!(
                    v.is_finite() && v.length_squared() > 0.0,
                    "world sun geometry"
                );
                v.normalize()
            }
        };
        let sun = sun_local(self.active, self.eye);
        if let Some(b) = self.bodies.get(&self.active) {
            let mut material = grounds.get_mut(&b.material).expect("world ground material");
            crate::scenery::update_ground(&mut material.ground, self.eye, sun, fleet.time());
        }
        use void_scenery::atmosphere_scene::{AtmosphereVolume, ordered_volumes};
        let volumes: Vec<_> = self
            .atmospheres
            .iter()
            .map(|(&body, a)| AtmosphereVolume {
                body,
                center: sample
                    .to_camera(fleet, fleet.body_frames(body).1, axes)
                    .apply_point(DVec3::ZERO),
                radius: a.top_radius,
            })
            .collect();
        let layers = ordered_volumes(&volumes)
            .into_iter()
            .map(|i| {
                let body = volumes[i].body;
                let into = sample.to_camera(fleet, fleet.body_frames(body).1, axes);
                let turn = into.rotation().conjugate();
                let eye = -(turn * volumes[i].center);
                let a = &self.atmospheres[&body];
                let mut air = a.air;
                air.update(
                    eye,
                    (turn.as_quat() * camera.rotation).normalize(),
                    projection,
                    focal,
                    sun_local(body, eye),
                );
                (air, a.textures.clone())
            })
            .collect();
        let mut params = void_scenery::earth_like_atmosphere(
            fleet.ephemeris.bodies()[self.active].radius_meters,
        );
        params.rayleigh_scattering = [0.0; 3];
        params.ozone_absorption = [0.0; 3];
        params.mie_scattering = 0.0;
        params.mie_extinction = 0.0;
        let mut resolve = AirSettings::new(&params);
        resolve.enabled = 0.0;
        resolve.clouds_enabled = 0.0;
        resolve.sun_disc_enabled = 0.0;
        resolve.exposure = sim.presentation.exposure;
        resolve.update(
            self.eye,
            (self.rotation.conjugate().as_quat() * camera.rotation).normalize(),
            projection,
            focal,
            sun,
        );
        (resolve, AirLayers(layers), self.rotation * sun)
    }
}

//! lab/scenery's look in Bevy: the lit ground and sea (`GroundMaterial`), the star field
//! (`StarMaterial`) and the atmosphere's tables as textures. Render space is the planet's body-fixed
//! axes with the camera at the origin.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use void_scenery::AtmosphereParams;

use crate::tiles::{ATTRIBUTE_CELL, ATTRIBUTE_HEIGHT};

const GROUND_SHADER: &str = "embedded://void_app/shaders/scenery/ground.wgsl";
const STARS_SHADER: &str = "embedded://void_app/shaders/scenery/stars.wgsl";
/// Imported by the others as `void::atmosphere`.
const ATMOSPHERE_SHADER: &str = "embedded://void_app/shaders/scenery/atmosphere.wgsl";

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        // Compiled into the binary, so the shaders load however the app is started.
        bevy::asset::embedded_asset!(app, "shaders/scenery/atmosphere.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/scenery/ground.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/scenery/impact.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/scenery/stars.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/scenery/air.wgsl");
        app.add_plugins((
            MaterialPlugin::<GroundMaterial>::default(),
            MaterialPlugin::<StarMaterial>::default(),
            crate::air::AirPlugin,
        ))
        .add_systems(Startup, load_shared_shaders);
    }
}

/// Keeps the imported shader modules loaded.
#[derive(Resource)]
struct SharedShaders(#[allow(dead_code)] Vec<Handle<Shader>>);

fn load_shared_shaders(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(SharedShaders(vec![
        assets.load(ATMOSPHERE_SHADER),
        assets.load("embedded://void_app/shaders/scenery/impact.wgsl"),
    ]));
}

/// An RGBA f32 table as a linearly filtered half-float texture, clamped at the edges, as the lab
/// uploads it (half floats filter on every device).
pub fn table_image(data: &[f32], width: usize, height: usize) -> Image {
    assert_eq!(
        data.len(),
        width * height * 4,
        "table_image: {width}×{height}"
    );
    assert!(
        data.iter().all(|v| v.is_finite() && v.abs() <= 65504.0),
        "scenery LUT cannot be represented as finite RGBA16Float"
    );
    let bytes = data
        .iter()
        .flat_map(|&v| half::f16::from_f32(v).to_le_bytes())
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// The ground shader's uniforms; see `ground.wgsl`.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GroundUniforms {
    pub planet_center: Vec3,
    pub sun_illuminance: f32,
    pub sun_direction: Vec3,
    pub atmosphere_enabled: f32,
    pub wave_origin: Vec3,
    pub time: f32,
    pub sea_level: f32,
    pub ocean_enabled: f32,
    pub rock_height: f32,
    pub snow_height: f32,
    pub bottom_radius: f32,
    pub top_radius: f32,
    pub horizon: f32,
    pub regolith: f32,
    pub impact_seed: u32,
    pub impact_density: f32,
    pub plains_fraction: f32,
    pub impact_basin_count: u32,
    pub impact_basins: [Vec4; 32],
    pub impact_fills: [Vec4; 8],
    pub impact_mature: Vec4,
    pub impact_plain_color: Vec4,
    pub impact_fresh: Vec4,
    pub impact_rays: [Vec4; 32],
    pub impact_ray_params: [Vec4; 32],
}

impl GroundUniforms {
    pub fn new(
        params: &AtmosphereParams,
        sea_level: f64,
        rock_height: f64,
        snow_height: f64,
    ) -> Self {
        Self {
            regolith: 0.0,
            impact_seed: 0,
            impact_density: 0.0,
            plains_fraction: 0.0,
            impact_basin_count: 0,
            impact_basins: [Vec4::ZERO; 32],
            impact_fills: [Vec4::ZERO; 8],
            impact_mature: Vec4::ZERO,
            impact_plain_color: Vec4::ZERO,
            impact_fresh: Vec4::ZERO,
            impact_rays: [Vec4::ZERO; 32],
            impact_ray_params: [Vec4::ZERO; 32],
            planet_center: Vec3::ZERO,
            sun_illuminance: 1.0,
            sun_direction: Vec3::X,
            atmosphere_enabled: 1.0,
            wave_origin: Vec3::ZERO,
            time: 0.0,
            sea_level: sea_level as f32,
            ocean_enabled: 1.0,
            rock_height: rock_height as f32,
            snow_height: snow_height as f32,
            bottom_radius: params.bottom_radius as f32,
            top_radius: params.top_radius as f32,
            horizon: (params.top_radius * params.top_radius
                - params.bottom_radius * params.bottom_radius)
                .sqrt() as f32,
        }
    }
}

/// Wave and detail patterns repeat every this many metres on each body-fixed axis.
pub const WAVE_PERIOD: f64 = 4096.0;

/// The per-frame part of the ground's uniforms: camera (body-fixed), sun, clock.
pub fn update_ground(
    uniforms: &mut GroundUniforms,
    camera: glam::DVec3,
    sun: glam::DVec3,
    seconds: f64,
) {
    uniforms.planet_center = (-camera).as_vec3();
    uniforms.sun_direction = sun.as_vec3();
    let wrap = |v: f64| v - (v / WAVE_PERIOD).floor() * WAVE_PERIOD;
    uniforms.wave_origin = Vec3::new(
        wrap(camera.x) as f32,
        wrap(camera.y) as f32,
        wrap(camera.z) as f32,
    );
    // Every wave's period divides no common time; keep the clock small for f32 by wrapping at a day.
    uniforms.time = (seconds % 86_400.0) as f32;
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GroundMaterial {
    #[uniform(0)]
    pub ground: GroundUniforms,
    #[texture(1)]
    #[sampler(2)]
    pub transmittance: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub irradiance: Handle<Image>,
}

impl Material for GroundMaterial {
    fn vertex_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_HEIGHT.at_shader_location(3),
            ATTRIBUTE_CELL.at_shader_location(4),
        ])?];
        Ok(())
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct StarMaterial {
    /// Brightness of a magnitude-0 star.
    #[uniform(0)]
    pub brightness: f32,
}

impl Material for StarMaterial {
    fn vertex_shader() -> ShaderRef {
        STARS_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        STARS_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
        ])?];
        // Drawn behind everything; they write no depth, as the lab's.
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// The star field as one point-list mesh; see `void_scenery::generate_stars`.
pub fn star_mesh(positions: Vec<[f32; 3]>, colors: &[[f32; 3]]) -> Mesh {
    let colors: Vec<[f32; 4]> = colors.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect();
    Mesh::new(
        PrimitiveTopology::PointList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
}

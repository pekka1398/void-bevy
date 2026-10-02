//! lab/scenery's transport and resolve passes as one Bevy post-process pass (`air.wgsl`): the air
//! and clouds between the camera and the scene, integrated together in depth order, plus the sun's
//! disc. It runs on the HDR scene before tone mapping, reading the main depth texture.
//!
//! Main world: `AirSettings` on the camera (its uniforms, set every frame) and the `AirTextures`
//! resource. Render world: the pipeline, and the pass in `Core3dSystems::PostProcess`.

use bevy::asset::RenderAssetUsages;
use bevy::camera::CameraProjection;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_3d, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ViewDepthTexture, ViewTarget};
use bevy::render::{RenderApp, RenderStartup};
use void_scenery::AtmosphereParams;
use void_scenery::clouds::{DETAIL_PERIOD, SHAPE_PERIOD};

pub(crate) const AIR_SHADER: &str = "embedded://void_app/shaders/scenery/air.wgsl";

pub struct AirPlugin;

impl Plugin for AirPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<AirSettings>::default(),
            UniformComponentPlugin::<AirSettings>::default(),
            ExtractResourcePlugin::<AirTextures>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_air_pipeline)
            .add_systems(
                Core3d,
                air_pass
                    .in_set(Core3dSystems::PostProcess)
                    .before(tonemapping),
            );
    }
}

/// The pass's uniforms; see `air.wgsl`. Put it on the camera to draw the air for that view.
#[derive(Component, Clone, Copy, Debug, ExtractComponent, ShaderType)]
pub struct AirSettings {
    pub view_from_clip: Mat4,
    pub world_from_view: Mat4,
    pub planet_center: Vec3,
    pub camera_altitude: f32,
    pub camera_up: Vec3,
    pub sun_illuminance: f32,
    pub sun_direction: Vec3,
    pub enabled: f32,
    pub rayleigh_scattering: Vec3,
    pub rayleigh_scale_height: f32,
    pub ozone_absorption: Vec3,
    pub ozone_center_height: f32,
    pub ozone_width: f32,
    pub mie_scattering: f32,
    pub mie_extinction: f32,
    pub mie_scale_height: f32,
    pub mie_anisotropy: f32,
    pub multiple_enabled: f32,
    pub sun_disc_enabled: f32,
    pub near: f32,
    pub bottom_radius: f32,
    pub top_radius: f32,
    pub horizon: f32,
    pub clouds_enabled: f32,
    pub shape_origin: Vec3,
    pub coverage: f32,
    pub detail_origin: Vec3,
    pub weather_only: f32,
    pub macro_origin: Vec3,
    pub sea_level: f32,
    pub focal_pixels: f32,
}

impl AirSettings {
    /// The constant part, from the atmosphere's parameters; everything per-frame starts neutral.
    pub fn new(p: &AtmosphereParams) -> Self {
        let v = |c: [f64; 3]| Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32);
        Self {
            view_from_clip: Mat4::IDENTITY,
            world_from_view: Mat4::IDENTITY,
            planet_center: Vec3::ZERO,
            camera_altitude: 1.0,
            camera_up: Vec3::Z,
            sun_illuminance: 1.0,
            sun_direction: Vec3::X,
            enabled: 1.0,
            rayleigh_scattering: v(p.rayleigh_scattering),
            rayleigh_scale_height: p.rayleigh_scale_height as f32,
            ozone_absorption: v(p.ozone_absorption),
            ozone_center_height: p.ozone_center_height as f32,
            ozone_width: p.ozone_width as f32,
            mie_scattering: p.mie_scattering as f32,
            mie_extinction: p.mie_extinction as f32,
            mie_scale_height: p.mie_scale_height as f32,
            mie_anisotropy: p.mie_anisotropy as f32,
            multiple_enabled: 1.0,
            sun_disc_enabled: 1.0,
            near: 0.1,
            bottom_radius: p.bottom_radius as f32,
            top_radius: p.top_radius as f32,
            horizon: (p.top_radius * p.top_radius - p.bottom_radius * p.bottom_radius).sqrt()
                as f32,
            clouds_enabled: 1.0,
            shape_origin: Vec3::ZERO,
            coverage: void_scenery::clouds::DEFAULT_CLOUD_COVERAGE as f32,
            detail_origin: Vec3::ZERO,
            weather_only: 0.0,
            macro_origin: Vec3::ZERO,
            sea_level: 0.0,
            focal_pixels: 1000.0,
        }
    }

    /// Per frame: the camera's body-fixed position, its rotation and projection, the sun.
    pub fn update(
        &mut self,
        camera: glam::DVec3,
        rotation: Quat,
        projection: &PerspectiveProjection,
        focal_pixels: f64,
        sun: glam::DVec3,
    ) {
        let r = camera.length();
        assert!(r > 0.0, "AirSettings::update: camera at the planet centre");
        self.planet_center = (-camera).as_vec3();
        self.camera_up = (camera / r).as_vec3();
        self.camera_altitude = (r - f64::from(self.bottom_radius)) as f32;
        self.sun_direction = sun.as_vec3();
        self.view_from_clip = projection.get_clip_from_view().inverse();
        self.world_from_view = Mat4::from_quat(rotation);
        self.near = projection.near;
        // Camera positions modulo each noise period, so the f32 noise coordinates stay small.
        let origin = |period: f64| {
            let m = |v: f64| v.rem_euclid(period) as f32;
            Vec3::new(m(camera.x), m(camera.y), m(camera.z))
        };
        self.shape_origin = origin(SHAPE_PERIOD);
        self.detail_origin = origin(DETAIL_PERIOD);
        self.macro_origin = origin(SHAPE_PERIOD * 16.0);
        self.focal_pixels = focal_pixels as f32;
    }
}

/// The tables and the cloud textures the pass reads.
#[derive(Resource, Clone, ExtractResource)]
pub struct AirTextures {
    pub transmittance: Handle<Image>,
    pub multiple: Handle<Image>,
    pub irradiance: Handle<Image>,
    pub weather: Handle<Image>,
    pub shape: Handle<Image>,
    pub detail: Handle<Image>,
}

/// The weather atlas: RGBA8, linear, longitude repeating.
pub fn weather_image(data: Vec<u8>, width: usize, height: usize) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// A repeating RGBA8 noise volume with its whole mip chain, each level the 2×2×2 box average of the
/// one above, as WebGL's generateMipmap builds it for the lab.
pub fn noise_volume_image(data: Vec<u8>, size: usize) -> Image {
    let mut levels = vec![data];
    let mut n = size;
    while n > 1 {
        let above = levels.last().unwrap();
        let m = n / 2;
        let mut next = vec![0u8; m * m * m * 4];
        for z in 0..m {
            for y in 0..m {
                for x in 0..m {
                    for c in 0..4 {
                        let mut sum = 0u32;
                        for (dx, dy, dz) in (0..8).map(|k| (k & 1, (k >> 1) & 1, k >> 2)) {
                            let (sx, sy, sz) = (2 * x + dx, 2 * y + dy, 2 * z + dz);
                            sum += u32::from(above[((sz * n + sy) * n + sx) * 4 + c]);
                        }
                        next[((z * m + y) * m + x) * 4 + c] = ((sum + 4) / 8) as u8;
                    }
                }
            }
        }
        levels.push(next);
        n = m;
    }
        let mip_level_count = levels.len() as u32;
    // `Image::new` takes the top level alone; the whole chain follows, level after level.
    let mut image = Image::new(
        Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: size as u32 },
        TextureDimension::D3,
        levels[0].clone(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(levels.concat());
    image.texture_descriptor.mip_level_count = mip_level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

#[derive(Resource)]
struct AirPipeline {
    layout: BindGroupLayoutDescriptor,
    table_sampler: Sampler,
    noise_sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

fn init_air_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let filterable = TextureSampleType::Float { filterable: true };
    let layout = BindGroupLayoutDescriptor::new(
        "air_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
                uniform_buffer::<AirSettings>(true),
                texture_2d(filterable),
                texture_2d(filterable),
                texture_2d(filterable),
                sampler(SamplerBindingType::Filtering),
                texture_2d(filterable),
                texture_3d(filterable),
                texture_3d(filterable),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    // The tables clamp at the edges; the weather atlas and noise volumes repeat, trilinear.
    let table_sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let noise_sampler = render_device.create_sampler(&SamplerDescriptor {
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Linear,
        ..default()
    });
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("air_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: asset_server.load(AIR_SHADER),
            targets: vec![Some(ColorTargetState {
                // The camera is always Hdr: the default HDR main texture.
                format: TextureFormat::Rgba16Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(AirPipeline {
        layout,
        table_sampler,
        noise_sampler,
        pipeline_id,
    });
}

#[allow(clippy::too_many_arguments)]
fn air_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &DynamicUniformIndex<AirSettings>,
    )>,
    air_pipeline: Option<Res<AirPipeline>>,
    textures: Option<Res<AirTextures>>,
    images: Res<RenderAssets<GpuImage>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<AirSettings>>,
    mut ctx: RenderContext,
) {
    let (Some(air_pipeline), Some(textures)) = (air_pipeline, textures) else {
        return;
    };
    let (view_target, depth, settings_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(air_pipeline.pipeline_id) else {
        return;
    };
    let Some(settings) = uniforms.uniforms().binding() else {
        return;
    };
    let image = |h: &Handle<Image>| images.get(h).map(|i| &i.texture_view);
    let (
        Some(transmittance),
        Some(multiple),
        Some(irradiance),
        Some(weather),
        Some(shape),
        Some(detail),
    ) = (
        image(&textures.transmittance),
        image(&textures.multiple),
        image(&textures.irradiance),
        image(&textures.weather),
        image(&textures.shape),
        image(&textures.detail),
    )
    else {
        return;
    };
    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "air_bind_group",
        &pipeline_cache.get_bind_group_layout(&air_pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            depth.view(),
            settings.clone(),
            transmittance,
            multiple,
            irradiance,
            &air_pipeline.table_sampler,
            weather,
            shape,
            detail,
            &air_pipeline.noise_sampler,
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("air_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
    pass.draw(0..3, 0..1);
}

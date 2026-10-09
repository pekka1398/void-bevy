//! Optional bit-preserving packing directly into Bevy's mesh slabs. Sampling and
//! seam interpolation remain in the f64 core. No geometry is read back in play.
use crate::tiles::{ATTRIBUTE_CELL, ATTRIBUTE_HEIGHT};
use bevy::{
    asset::{Asset, AssetId, Assets, RenderAssetUsages},
    camera::primitives::Aabb,
    core_pipeline::schedule::camera_driver,
    ecs::system::{
        SystemParamItem,
        lifetimeless::{SRes, SResMut},
    },
    mesh::{Indices, Mesh},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        mesh::{
            RenderMesh, RenderMeshBufferInfo,
            allocator::{ElementClass, MeshAllocationKey, MeshAllocator, MeshAllocatorSettings},
        },
        render_asset::{
            AssetExtractionError, PrepareAssetError, RenderAsset, RenderAssetPlugin, RenderAssets,
            prepare_assets,
        },
        render_resource::{
            binding_types::{storage_buffer, storage_buffer_read_only, uniform_buffer},
            *,
        },
        renderer::{
            RenderAdapter, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
            RenderQueue,
        },
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use void_lod::{TileMeshData, stitch_edges};

pub fn enabled() -> bool {
    let enabled =
        std::env::args().any(|arg| arg == "--lod-gpu-pack" || arg == "--lod-gpu-pack-verify");
    assert!(
        !(enabled && std::env::args().any(|arg| arg == "--lod-main-world-meshes")),
        "GPU LOD packing cannot retain ordinary CPU mesh attributes"
    );
    enabled
}
fn verifying() -> bool {
    std::env::args().any(|arg| arg == "--lod-gpu-pack-verify")
}

#[derive(Default)]
struct Telemetry {
    ready: AtomicBool,
    pending: AtomicU64,
    packed: AtomicU64,
    canceled: AtomicU64,
    input_bytes: AtomicU64,
    output_bytes: AtomicU64,
    verify_pending: AtomicU64,
    verified: AtomicU64,
    prepare_ns: AtomicU64,
    pack_ns: AtomicU64,
    bind_groups: AtomicU64,
    failure: Mutex<Option<String>>,
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct GpuPackingShared(Arc<Telemetry>);
impl GpuPackingShared {
    pub fn ready(&self) -> bool {
        self.check();
        self.0.ready.load(Ordering::Acquire)
    }
    pub fn pending(&self) -> u64 {
        self.0.pending.load(Ordering::Acquire) + self.0.verify_pending.load(Ordering::Acquire)
    }
    pub fn check(&self) {
        if let Some(message) = &*self.0.failure.lock().unwrap() {
            panic!("GPU LOD packing: {message}");
        }
    }
    pub fn report(&self) -> serde_json::Value {
        self.check();
        serde_json::json!({"pipeline_ready":self.ready(),"pending":self.0.pending.load(Ordering::Acquire),
            "packed_tiles":self.0.packed.load(Ordering::Relaxed),"canceled_tiles":self.0.canceled.load(Ordering::Relaxed),
            "source_bytes":self.0.input_bytes.load(Ordering::Relaxed),"output_bytes":self.0.output_bytes.load(Ordering::Relaxed),
            "verification_pending":self.0.verify_pending.load(Ordering::Acquire),"verified_tiles":self.0.verified.load(Ordering::Relaxed),
            "prepare_cpu_total_ms":self.0.prepare_ns.load(Ordering::Relaxed) as f64 / 1e6,
            "pack_encode_cpu_total_ms":self.0.pack_ns.load(Ordering::Relaxed) as f64 / 1e6,
            "bind_groups_created":self.0.bind_groups.load(Ordering::Relaxed),
            "verification_enabled":verifying(),"packing":"resident slabs; CPU f64 sampling and seams; no geometry readback except explicit verification"})
    }
}
struct TicketInner {
    shared: GpuPackingShared,
    finished: AtomicBool,
}
impl Drop for TicketInner {
    fn drop(&mut self) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            self.shared.0.canceled.fetch_add(1, Ordering::Relaxed);
            self.shared.0.pending.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
#[derive(Clone)]
struct Ticket(Arc<TicketInner>);
impl Ticket {
    fn new(shared: &GpuPackingShared) -> Self {
        shared.0.pending.fetch_add(1, Ordering::AcqRel);
        Self(Arc::new(TicketInner {
            shared: shared.clone(),
            finished: AtomicBool::new(false),
        }))
    }
    fn packed(&self) {
        assert!(
            !self.0.finished.swap(true, Ordering::AcqRel),
            "GPU tile completed twice"
        );
        self.0.shared.0.packed.fetch_add(1, Ordering::Relaxed);
        self.0.shared.0.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct UploadContext<'a> {
    pub assets: &'a mut Assets<GpuTileSource>,
    pub shared: &'a GpuPackingShared,
}
#[derive(Component)]
pub struct GpuTileHandle(#[allow(dead_code)] pub Handle<GpuTileSource>);

pub struct UploadData {
    bytes: Vec<u8>,
    expected: Option<Vec<u8>>,
    n: u32,
    index_bits: u32,
    cell_bits: u32,
    pub bounds: Aabb,
}
impl UploadData {
    pub fn bytes_len(&self) -> usize {
        self.bytes.len()
    }
    pub fn metadata_mesh(&self) -> Mesh {
        let mut mesh = empty_layout_mesh(RenderAssetUsages::MAIN_WORLD);
        mesh.final_aabb = Some(bevy::math::bounding::Aabb3d::new(
            self.bounds.center,
            self.bounds.half_extents,
        ));
        mesh
    }
    pub fn into_asset(self, mesh: Handle<Mesh>, shared: &GpuPackingShared) -> GpuTileSource {
        GpuTileSource {
            mesh,
            bounds: self.bounds,
            n: self.n,
            index_bits: self.index_bits,
            cell_bits: self.cell_bits,
            bytes: Some(self.bytes),
            expected: self.expected,
            ticket: Ticket::new(shared),
        }
    }
}
pub fn upload_data(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
    indices: &Indices,
    radius: f64,
) -> UploadData {
    let (mut positions, mut normals, mut heights) = stitch_edges(data, coarse, n);
    let count = n * n;
    positions.truncate(count);
    normals.truncate(count);
    heights.truncate(count);
    assert_eq!(positions.len(), count);
    assert_eq!(normals.len(), count);
    assert_eq!(heights.len(), count);
    let bounds =
        Aabb::enclosing(positions.iter().map(|p| Vec3::from_array(*p))).expect("GPU tile bounds");
    let cell = void_lod::cell_meters(radius, data.key.level, n) as f32;
    let index_bits = match indices {
        Indices::U16(_) => 16,
        Indices::U32(_) => 32,
    };
    assert_eq!(indices.len(), 6 * (n - 1) * (n - 1));
    let mut bytes = Vec::with_capacity(count * 40);
    bytes.extend_from_slice(bytemuck::cast_slice(&positions));
    bytes.extend_from_slice(bytemuck::cast_slice(&normals));
    bytes.extend_from_slice(bytemuck::cast_slice(&data.colors[..count]));
    bytes.extend_from_slice(bytemuck::cast_slice(&heights));
    let expected = verifying().then(|| {
        let mut expected = Vec::with_capacity(count * 48 + indices.len() * (index_bits / 8));
        for i in 0..count {
            expected.extend_from_slice(bytemuck::bytes_of(&positions[i]));
            expected.extend_from_slice(bytemuck::bytes_of(&normals[i]));
            expected.extend_from_slice(bytemuck::bytes_of(&data.colors[i]));
            expected.extend_from_slice(&1.0_f32.to_ne_bytes());
            expected.extend_from_slice(&heights[i].to_ne_bytes());
            expected.extend_from_slice(&cell.to_ne_bytes());
        }
        match indices {
            Indices::U16(values) => expected.extend_from_slice(bytemuck::cast_slice(values)),
            Indices::U32(values) => expected.extend_from_slice(bytemuck::cast_slice(values)),
        };
        expected
    });
    UploadData {
        bytes,
        expected,
        n: u32::try_from(n).expect("GPU tile resolution"),
        index_bits: index_bits as u32,
        cell_bits: cell.to_bits(),
        bounds,
    }
}
fn empty_layout_mesh(usage: RenderAssetUsages) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
        .with_inserted_attribute(ATTRIBUTE_HEIGHT, Vec::<f32>::new())
        .with_inserted_attribute(ATTRIBUTE_CELL, Vec::<f32>::new())
}
#[derive(Asset, TypePath, Clone)]
pub struct GpuTileSource {
    mesh: Handle<Mesh>,
    bounds: Aabb,
    n: u32,
    index_bits: u32,
    cell_bits: u32,
    bytes: Option<Vec<u8>>,
    expected: Option<Vec<u8>>,
    #[type_path(ignore)]
    ticket: Ticket,
}
struct PreparedTile {
    mesh: Handle<Mesh>,
    n: u32,
    index_bits: u32,
    cell_bits: u32,
    bytes: Option<Vec<u8>>,
    expected: Option<Vec<u8>>,
    ticket: Ticket,
}
#[derive(Resource, Clone, ExtractResource)]
struct Prototypes {
    short: Handle<Mesh>,
    long: Handle<Mesh>,
}
#[derive(Resource)]
struct PackPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

pub struct GpuLodPackingPlugin;
impl Plugin for GpuLodPackingPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.get_sub_app(RenderApp).is_some(),
            "GPU LOD packing requires native rendering"
        );
        bevy::asset::embedded_asset!(app, "shaders/lod_pack.wgsl");
        app.init_resource::<GpuPackingShared>()
            .init_asset::<GpuTileSource>()
            .add_plugins((
                RenderAssetPlugin::<PreparedTile, RenderMesh>::default(),
                ExtractResourcePlugin::<GpuPackingShared>::default(),
                ExtractResourcePlugin::<Prototypes>::default(),
            ))
            .add_systems(Startup, init_prototypes);
        let render = app.sub_app_mut(RenderApp);
        render
            .init_resource::<VerificationQueue>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(
                Render,
                ready
                    .after(prepare_assets::<RenderMesh>)
                    .in_set(RenderSystems::PrepareAssets),
            )
            .add_systems(
                RenderGraph,
                pack_tiles
                    .before(camera_driver)
                    .in_set(RenderGraphSystems::Render),
            )
            .add_systems(
                RenderGraph,
                kick_verifications.in_set(RenderGraphSystems::Finish),
            );
    }
    fn finish(&self, app: &mut App) {
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<MeshAllocatorSettings>()
            .extra_buffer_usages |= BufferUsages::STORAGE | BufferUsages::COPY_SRC;
    }
}
fn init_prototypes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let make = |indices| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; 3])
        .with_inserted_attribute(ATTRIBUTE_HEIGHT, vec![0.0; 3])
        .with_inserted_attribute(ATTRIBUTE_CELL, vec![1.0; 3])
        .with_inserted_indices(indices)
    };
    // ElementLayout constructors are private in pinned Bevy. These two invisible,
    // persistent metadata templates obtain the exact public allocator layouts.
    let short = meshes.add(make(Indices::U16(vec![0, 1, 2, 0, 2, 1])));
    let long = meshes.add(make(Indices::U32(vec![0, 1, 2, 0, 2, 1])));
    commands.insert_resource(Prototypes { short, long });
}
fn init_pipeline(
    mut commands: Commands,
    server: Res<AssetServer>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
) {
    assert!(
        adapter
            .get_downlevel_capabilities()
            .flags
            .contains(DownlevelFlags::BASE_VERTEX),
        "GPU LOD packing requires base-vertex support"
    );
    assert!(
        device.limits().max_storage_buffers_per_shader_stage >= 3,
        "GPU LOD packing requires three storage buffers per stage"
    );
    let layout = BindGroupLayoutDescriptor::new(
        "resident LOD pack",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                uniform_buffer::<PackParams>(true),
            ),
        ),
    );
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("resident LOD pack".into()),
        layout: vec![layout.clone()],
        shader: server.load("embedded://void_app/shaders/lod_pack.wgsl"),
        entry_point: Some("pack".into()),
        ..default()
    });
    commands.insert_resource(PackPipeline { layout, pipeline });
}
fn ready(
    cache: Res<PipelineCache>,
    pipeline: Res<PackPipeline>,
    prototypes: Res<Prototypes>,
    meshes: Res<RenderAssets<RenderMesh>>,
    shared: Res<GpuPackingShared>,
) {
    if let CachedPipelineState::Err(error) = cache.get_compute_pipeline_state(pipeline.pipeline)
        && !matches!(
            error,
            bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable
        )
    {
        panic!("GPU LOD compute pipeline: {error}");
    }
    let ready = cache.get_compute_pipeline(pipeline.pipeline).is_some()
        && meshes.get(prototypes.short.id()).is_some()
        && meshes.get(prototypes.long.id()).is_some();
    shared.0.ready.store(ready, Ordering::Release);
}
impl RenderAsset for PreparedTile {
    type SourceAsset = GpuTileSource;
    type Param = (
        SRes<RenderDevice>,
        SRes<RenderQueue>,
        SResMut<MeshAllocator>,
        SRes<MeshAllocatorSettings>,
        SResMut<RenderAssets<RenderMesh>>,
        SRes<Prototypes>,
    );
    fn asset_usage(_: &GpuTileSource) -> RenderAssetUsages {
        RenderAssetUsages::RENDER_WORLD
    }
    fn byte_len(source: &GpuTileSource) -> Option<usize> {
        source.bytes.as_ref().map(Vec::len)
    }
    fn take_gpu_data(
        source: &mut GpuTileSource,
        _: Option<&Self>,
    ) -> Result<GpuTileSource, AssetExtractionError> {
        let bytes = source
            .bytes
            .take()
            .ok_or(AssetExtractionError::AlreadyExtracted)?;
        let expected = source.expected.take();
        Ok(GpuTileSource {
            mesh: source.mesh.clone(),
            bounds: source.bounds,
            n: source.n,
            index_bits: source.index_bits,
            cell_bits: source.cell_bits,
            bytes: Some(bytes),
            expected,
            ticket: source.ticket.clone(),
        })
    }
    fn prepare_asset(
        source: GpuTileSource,
        _id: AssetId<GpuTileSource>,
        (device, queue, allocator, settings, meshes, prototypes): &mut SystemParamItem<Self::Param>,
        _: Option<&Self>,
    ) -> Result<Self, PrepareAssetError<GpuTileSource>> {
        let prototype = if source.index_bits == 16 {
            prototypes.short.id()
        } else {
            prototypes.long.id()
        };
        let Some(template) = meshes.get(prototype) else {
            return Err(PrepareAssetError::RetryNextUpdate(source));
        };
        let started = std::time::Instant::now();
        assert_eq!(template.layout.0.layout().array_stride, 48);
        let layout = template.layout.clone();
        let key_bits = template.key_bits.clone();
        let slabs = allocator
            .mesh_slabs(&prototype)
            .expect("GPU metadata prototype slabs");
        let vertex_layout = *allocator.slabs[&slabs.vertex_slab_id].element_layout();
        let index_layout =
            *allocator.slabs[&slabs.index_slab_id.expect("GPU prototype indexed")].element_layout();
        let count = source.n.checked_mul(source.n).expect("GPU vertex count");
        let index_count = 6 * (source.n - 1) * (source.n - 1);
        let id = source.mesh.id();
        let mut stage = allocator.stage_allocation();
        stage.allocate(
            &MeshAllocationKey::new(id, ElementClass::Vertex),
            u64::from(count) * 48,
            vertex_layout,
            settings,
        );
        stage.allocate(
            &MeshAllocationKey::new(id, ElementClass::Index),
            u64::from(index_count) * u64::from(source.index_bits / 8),
            index_layout,
            settings,
        );
        stage.commit(device, queue);
        // In pinned Bevy, allocation commits reserve ranges but leave them
        // pending until copy_element_data publishes them. For general slabs a
        // zero-byte copy publishes the range without staging an upload; the
        // compute pass writes every element before any camera uses it.
        for class in [ElementClass::Vertex, ElementClass::Index] {
            let key = MeshAllocationKey::new(id, class);
            let slab_id = allocator.key_to_slab[&key];
            assert!(
                matches!(
                    allocator.slabs[&slab_id],
                    bevy::render::slab_allocator::Slab::General(_)
                ),
                "GPU LOD packing currently requires general mesh slabs"
            );
            allocator.copy_element_data(&key, 0, |_| {}, device, queue);
        }
        assert!(
            meshes
                .insert(
                    id,
                    RenderMesh {
                        vertex_count: count,
                        aabb_center: source.bounds.center.into(),
                        buffer_info: RenderMeshBufferInfo::Indexed {
                            count: index_count,
                            index_format: if source.index_bits == 16 {
                                IndexFormat::Uint16
                            } else {
                                IndexFormat::Uint32
                            }
                        },
                        key_bits,
                        layout
                    }
                )
                .is_none(),
            "GPU tile already registered"
        );
        source.ticket.0.shared.0.prepare_ns.fetch_add(
            started
                .elapsed()
                .as_nanos()
                .try_into()
                .expect("prepare duration"),
            Ordering::Relaxed,
        );
        Ok(PreparedTile {
            mesh: source.mesh,
            n: source.n,
            index_bits: source.index_bits,
            cell_bits: source.cell_bits,
            bytes: source.bytes,
            expected: source.expected,
            ticket: source.ticket,
        })
    }
}
#[derive(Clone, Copy, ShaderType)]
struct PackParams {
    n: u32,
    vertices: u32,
    input_start: u32,
    vertex_start: u32,
    index_start: u32,
    index_bits: u32,
    cell_bits: u32,
    padding: u32,
}
#[derive(Default, Resource)]
struct VerificationQueue(Vec<(Buffer, Vec<u8>, GpuPackingShared)>);

// Use full small slabs or an aligned window for large slabs. No out-of-range
// storage bindings, even when Bevy grows a slab beyond a device binding limit.
fn window<'a>(
    buffer: &'a Buffer,
    start: u64,
    len: u64,
    device: &RenderDevice,
) -> (BufferBinding<'a>, u32) {
    let max = device.limits().max_storage_buffer_binding_size;
    let (offset, size) = if buffer.size() <= max {
        (0, buffer.size())
    } else {
        let alignment = u64::from(device.limits().min_storage_buffer_offset_alignment);
        let offset = start / alignment * alignment;
        (offset, start + len - offset)
    };
    assert!(
        size <= max && offset + size <= buffer.size(),
        "GPU tile storage window exceeds device limits"
    );
    (
        BufferBinding {
            buffer,
            offset,
            size: BufferSize::new(size),
        },
        u32::try_from((start - offset) / 4).expect("GPU word offset"),
    )
}
#[allow(clippy::too_many_arguments)]
fn pack_tiles(
    mut prepared: ResMut<RenderAssets<PreparedTile>>,
    allocator: Res<MeshAllocator>,
    pipeline: Res<PackPipeline>,
    cache: Res<PipelineCache>,
    queue: Res<RenderQueue>,
    shared: Res<GpuPackingShared>,
    mut checks: ResMut<VerificationQueue>,
    mut ctx: RenderContext,
) {
    let Some(compute) = cache.get_compute_pipeline(pipeline.pipeline) else {
        return;
    };
    let ids: Vec<_> = prepared
        .iter()
        .filter_map(|(id, tile)| tile.bytes.is_some().then_some(id))
        .collect();
    if ids.is_empty() {
        return;
    }
    let started = std::time::Instant::now();
    let device = ctx.render_device().clone();
    let max_bytes = device.limits().max_storage_buffer_binding_size as usize;
    let mut chunks: Vec<Vec<AssetId<GpuTileSource>>> = Vec::new();
    let mut chunk = Vec::new();
    let mut size = 0;
    for id in ids {
        let bytes = prepared.get(id).unwrap().bytes.as_ref().unwrap().len();
        assert!(bytes <= max_bytes, "GPU tile input exceeds storage limit");
        if size + bytes > max_bytes {
            chunks.push(std::mem::take(&mut chunk));
            size = 0;
        }
        size += bytes;
        chunk.push(id);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    for chunk in chunks {
        let mut input = Vec::new();
        let mut uniforms = DynamicUniformBuffer::<PackParams>::new_with_alignment(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        let mut work = Vec::new();
        for id in &chunk {
            let tile = prepared.get_mut(*id).unwrap();
            let bytes = tile.bytes.take().expect("GPU tile payload once");
            let mesh_id = tile.mesh.id();
            let v = allocator
                .mesh_vertex_slice(&mesh_id)
                .expect("GPU tile vertex allocation");
            let i = allocator
                .mesh_index_slice(&mesh_id)
                .expect("GPU tile index allocation");
            let vertex_bytes = u64::from(tile.n) * u64::from(tile.n) * 48;
            let index_bytes =
                u64::from(6 * (tile.n - 1) * (tile.n - 1)) * u64::from(tile.index_bits / 8);
            let (v_binding, v_word) = window(
                v.buffer,
                u64::from(v.range.start) * 48,
                vertex_bytes,
                &device,
            );
            let (i_binding, i_word) = window(
                i.buffer,
                u64::from(i.range.start) * u64::from(tile.index_bits / 8),
                index_bytes,
                &device,
            );
            let params = PackParams {
                n: tile.n,
                vertices: tile.n * tile.n,
                input_start: u32::try_from(input.len() / 4).unwrap(),
                vertex_start: v_word,
                index_start: i_word,
                index_bits: tile.index_bits,
                cell_bits: tile.cell_bits,
                padding: 0,
            };
            let uniform_offset = uniforms.push(&params);
            input.extend_from_slice(&bytes);
            work.push((
                *id,
                uniform_offset,
                v_binding,
                i_binding,
                vertex_bytes,
                index_bytes,
                v.buffer.id(),
                i.buffer.id(),
            ));
        }
        let input_buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("LOD SoA upload batch"),
            contents: &input,
            usage: BufferUsages::STORAGE,
        });
        uniforms.write_buffer(&device, &queue);
        let mut groups = Vec::new();
        let mut group_indices = Vec::new();
        let mut group_cache = std::collections::HashMap::new();
        for (_, _, v, i, _, _, v_id, i_id) in &work {
            let key = (*v_id, v.offset, v.size, *i_id, i.offset, i.size);
            if let Some(&index) = group_cache.get(&key) {
                group_indices.push(index);
                continue;
            }
            let index = groups.len();
            groups.push(device.create_bind_group(
                "resident LOD pack",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    input_buffer.as_entire_buffer_binding(),
                    BindingResource::Buffer(v.clone()),
                    BindingResource::Buffer(i.clone()),
                    uniforms.binding().unwrap(),
                )),
            ));
            group_cache.insert(key, index);
            group_indices.push(index);
        }
        shared
            .0
            .bind_groups
            .fetch_add(groups.len() as u64, Ordering::Relaxed);
        let diagnostics = ctx.diagnostic_recorder();
        let diagnostics = diagnostics.as_deref();
        {
            let mut pass = ctx
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("resident LOD pack"),
                    ..default()
                });
            let span = diagnostics.pass_span(&mut pass, "lod_pack");
            pass.set_pipeline(compute);
            for ((id, offset, _, _, _, _, _, _), &index) in work.iter().zip(&group_indices) {
                let tile = prepared.get(*id).unwrap();
                pass.set_bind_group(0, &groups[index], &[*offset]);
                pass.dispatch_workgroups((tile.n * tile.n).div_ceil(64), 1, 1);
            }
            span.end(&mut pass);
        }
        shared
            .0
            .input_bytes
            .fetch_add(input.len() as u64, Ordering::Relaxed);
        for (id, _, _, _, vertex_bytes, index_bytes, _, _) in work {
            let tile = prepared.get_mut(id).unwrap();
            shared
                .0
                .output_bytes
                .fetch_add(vertex_bytes + index_bytes, Ordering::Relaxed);
            if let Some(expected) = tile.expected.take() {
                assert_eq!(expected.len() as u64, vertex_bytes + index_bytes);
                let buffer = device.create_buffer(&BufferDescriptor {
                    label: Some("LOD verification readback"),
                    size: vertex_bytes + index_bytes,
                    usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let v = allocator.mesh_vertex_slice(&tile.mesh.id()).unwrap();
                let i = allocator.mesh_index_slice(&tile.mesh.id()).unwrap();
                ctx.command_encoder().copy_buffer_to_buffer(
                    v.buffer,
                    u64::from(v.range.start) * 48,
                    &buffer,
                    0,
                    vertex_bytes,
                );
                ctx.command_encoder().copy_buffer_to_buffer(
                    i.buffer,
                    u64::from(i.range.start) * u64::from(tile.index_bits / 8),
                    &buffer,
                    vertex_bytes,
                    index_bytes,
                );
                shared.0.verify_pending.fetch_add(1, Ordering::AcqRel);
                checks.0.push((buffer, expected, shared.clone()));
            }
            tile.ticket.packed();
        }
    }
    shared.0.pack_ns.fetch_add(
        started
            .elapsed()
            .as_nanos()
            .try_into()
            .expect("packing duration"),
        Ordering::Relaxed,
    );
}
fn kick_verifications(mut queue: ResMut<VerificationQueue>) {
    // RenderGraphSystems::Finish is after the frame command buffers are submitted.
    for (buffer, expected, shared) in queue.0.drain(..) {
        let mapped = buffer.clone();
        buffer.slice(..).map_async(MapMode::Read, move |result| {
            if let Err(error) = result {
                *shared.0.failure.lock().unwrap() =
                    Some(format!("GPU verification map failed: {error}"));
            } else {
                let actual = mapped.slice(..).get_mapped_range().to_vec();
                mapped.unmap();
                if actual != expected {
                    let byte = actual.iter().zip(&expected).position(|(a, b)| a != b);
                    *shared.0.failure.lock().unwrap() =
                        Some(format!("resident payload differs at byte {byte:?}"));
                } else {
                    shared.0.verified.fetch_add(1, Ordering::Relaxed);
                }
            }
            shared.0.verify_pending.fetch_sub(1, Ordering::AcqRel);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloned_tickets_cancel_or_complete_exactly_once() {
        let shared = GpuPackingShared::default();
        let ticket = Ticket::new(&shared);
        let clone = ticket.clone();
        drop(ticket);
        assert_eq!(shared.pending(), 1);
        clone.packed();
        drop(clone);
        assert_eq!(shared.pending(), 0);
        assert_eq!(shared.0.packed.load(Ordering::Relaxed), 1);
        assert_eq!(shared.0.canceled.load(Ordering::Relaxed), 0);
        let ticket = Ticket::new(&shared);
        let clone = ticket.clone();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let other = barrier.clone();
            scope.spawn(move || {
                other.wait();
                drop(ticket);
            });
            scope.spawn(move || {
                barrier.wait();
                drop(clone);
            });
        });
        assert_eq!(shared.pending(), 0);
        assert_eq!(shared.0.canceled.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn upload_keeps_core_vertices_bounds_and_exact_cell_bits() {
        let n = 33;
        let radius = 6_371_000.0;
        let data = void_lod::build_tile_mesh(
            void_lod::TileKey {
                face: 0,
                level: 14,
                x: 8192,
                y: 8192,
            },
            &|d: glam::DVec3, _: f64| void_lod::SurfaceSample {
                height_meters: 5000.0 + d.z * 30.0,
                color: [0.1, 0.2, 0.3],
            },
            void_lod::TileMeshOptions {
                radius_meters: radius,
                resolution: n,
            },
        );
        let (indices, grid) = void_lod::build_tile_indices(n);
        let upload = upload_data(
            &data,
            [None; 4],
            n,
            &Indices::U16(
                indices[..grid]
                    .iter()
                    .map(|&i| u16::try_from(i).unwrap())
                    .collect(),
            ),
            radius,
        );
        assert_eq!(upload.bytes.len(), n * n * 40);
        assert_eq!(
            upload.cell_bits,
            (void_lod::cell_meters(radius, 14, n) as f32).to_bits()
        );
        assert_eq!(
            &upload.bytes[..n * n * 12],
            bytemuck::cast_slice::<_, u8>(&data.positions[..n * n])
        );
        assert_eq!(
            upload.metadata_mesh().asset_usage,
            RenderAssetUsages::MAIN_WORLD
        );
        use bevy::camera::primitives::MeshAabb;
        assert_eq!(
            upload.metadata_mesh().compute_aabb().unwrap(),
            upload.bounds
        );
    }
}

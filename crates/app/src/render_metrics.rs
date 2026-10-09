//! Render diagnostics tagged on the GPU with their source frame. The asynchronous readback is
//! correlated before collecting values, so settle/drain frames never masquerade as run samples.
use bevy::{
    diagnostic::DiagnosticsStore,
    prelude::*,
    render::{
        RenderApp,
        diagnostic::{RecordDiagnostics, RenderDiagnosticsPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{BufferInitDescriptor, BufferUsages, PipelineCache},
        renderer::{
            RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
        },
    },
};
use std::{collections::BTreeSet, path::PathBuf, time::Instant};
use void_diagnostics::render::{RenderCapture, RenderValue};

#[derive(Clone, Default, Resource, ExtractResource)]
pub struct RenderFrameTag {
    pub id: u32,
    pub measure: bool,
    pub warmup: bool,
}
#[derive(Resource)]
pub struct RenderMetrics {
    gpu_support: std::sync::Arc<std::sync::OnceLock<&'static str>>,
    allocator_gauges: std::collections::BTreeMap<String, (u64, f64, f64, f64)>,
    counters: DrawCounters,
    output: PathBuf,
    pub capture: RenderCapture,
    warmup_capture: RenderCapture,
    pub adapter: serde_json::Value,
    pub last_pending_pipelines: Option<u32>,
    pub last_paths: BTreeSet<String>,
    pub errors: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    pub delivered: BTreeSet<u32>,
    empty_pass_frames: BTreeSet<u32>,
    pub limit: Option<usize>,
    pub started: Instant,
    pub benchmark: Option<serde_json::Value>,
}
impl RenderMetrics {
    pub fn new(path: PathBuf, limit: Option<usize>) -> Self {
        Self {
            gpu_support: Default::default(),
            allocator_gauges: Default::default(),
            output: path,
            counters: DrawCounters::default(),
            capture: RenderCapture::default(),
            warmup_capture: RenderCapture::default(),
            adapter: serde_json::Value::Null,
            last_pending_pipelines: None,
            last_paths: BTreeSet::new(),
            errors: Default::default(),
            delivered: BTreeSet::new(),
            empty_pass_frames: BTreeSet::new(),
            limit,
            started: Instant::now(),
            benchmark: None,
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let allocator: std::collections::BTreeMap<_,_> = self.allocator_gauges.iter().map(|(name,(samples,total,max,last))|
            (name,serde_json::json!({"samples":samples,"mean":total/(*samples as f64),"max":max,"last":last}))).collect();
        serde_json::json!({"adapter":self.adapter, "gpu_preprocessing_max_supported":self.gpu_support.get(),
            "mesh_allocator":{"gauges":allocator,"measurement":"main-world diagnostic gauges during run updates; not GPU timestamp-correlated; bytes are allocated slab capacity, not live geometry bytes"}, "benchmark":self.benchmark,
            "capture":self.capture.report(),"warmup_capture":self.warmup_capture.report(), "delivered_frame_ids_including_settle_and_drain":self.delivered,
            "frames_without_pass_diagnostics":self.empty_pass_frames, "render_errors":*self.errors.lock().unwrap(), "draw_calls":{"tracked_capture_enabled":cfg!(feature="render-metrics"),
                "coverage":"Bevy TrackedRenderPass + VOID air + pinned Bevy tonemapping/upscaling fullscreen passes",
                "known_draw_records":"direct/fixed-count submissions; GPU-counted batches are read back separately and capped by each actual command max_count",
                "instrumentation_overhead":"detailed_trace and COPY_SRC/readback of used indirect count slots; compare only captures with the same instrumentation"}})
    }
    pub fn write(&self) {
        let parent = self
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(parent).expect("render metrics: create output directory");
        std::fs::write(
            &self.output,
            serde_json::to_vec_pretty(&self.report()).unwrap(),
        )
        .expect("render metrics: write report");
    }
}
impl Drop for RenderMetrics {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.write();
        }
    }
}
#[derive(Default, Clone)]
struct DrawFrame {
    counts: void_diagnostics::render::DrawSubmissions,
    requests: Vec<String>,
}
fn gpu_count_offset(message: &str) -> (u64, u32) {
    let (left, max) = message
        .rsplit_once(")x, max ")
        .expect("draw capture: GPU-count message");
    let offset = left
        .split_whitespace()
        .last()
        .unwrap()
        .parse::<u64>()
        .expect("draw capture: GPU count offset");
    let max = max
        .strip_suffix('x')
        .expect("draw capture: GPU max suffix")
        .parse::<u32>()
        .expect("draw capture: GPU maximum");
    (offset, max)
}
#[derive(Resource, Clone, Default)]
struct DrawCounters(
    std::sync::Arc<std::sync::Mutex<(u32, std::collections::BTreeMap<u32, DrawFrame>)>>,
);
#[derive(Resource, Clone)]
struct GpuSupportProbe(std::sync::Arc<std::sync::OnceLock<&'static str>>);
pub struct RenderMetricsPlugin;
impl Plugin for RenderMetricsPlugin {
    fn finish(&self, app: &mut App) {
        let features = app
            .sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .features();
        assert!(
            features.intersects(
                bevy::render::settings::WgpuFeatures::TIMESTAMP_QUERY
                    | bevy::render::settings::WgpuFeatures::PIPELINE_STATISTICS_QUERY
            ),
            "Source-frame render diagnostics require a GPU query set: pinned Bevy 0.19.1 reads numeric tag buffers before mapping completes when both query types are unavailable; use --profile for CPU-only profiling"
        );
    }
    fn build(&self, app: &mut App) {
        let counters = app.world().resource::<RenderMetrics>().counters.clone();
        let support = GpuSupportProbe(app.world().resource::<RenderMetrics>().gpu_support.clone());
        app.sub_app_mut(RenderApp)
            .insert_resource(counters)
            .insert_resource(support);
        if cfg!(feature = "render-metrics") {
            app.sub_app_mut(RenderApp).world_mut().resource_mut::<bevy::render::batching::gpu_preprocessing::IndirectParametersBuffersSettings>()
                .allow_copies_from_indirect_parameter_buffers=true;
        }
        app.init_resource::<RenderFrameTag>()
            .add_plugins((
                RenderDiagnosticsPlugin,
                bevy::render::diagnostic::MeshAllocatorDiagnosticPlugin,
                ExtractResourcePlugin::<RenderFrameTag>::default(),
            ))
            .add_systems(Startup, adapter_info)
            .add_systems(PostUpdate, collect)
            .add_systems(Last, next_frame);
        app.sub_app_mut(RenderApp).add_systems(
            RenderGraph,
            tag_frame
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
        app.sub_app_mut(RenderApp).add_systems(
            RenderGraph,
            tag_gpu_draw_counts
                .after(RenderGraphSystems::Render)
                .before(bevy::render::diagnostic::resolve_encoder),
        );
    }
}
fn adapter_info(
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapterInfo>,
    mut metrics: ResMut<RenderMetrics>,
) {
    let features = device.features();
    metrics.adapter = serde_json::json!({"name":adapter.name, "backend":format!("{:?}",adapter.backend),
        "device_type":format!("{:?}",adapter.device_type), "driver":adapter.driver, "driver_info":adapter.driver_info,
        "timestamp_queries":features.contains(bevy::render::settings::WgpuFeatures::TIMESTAMP_QUERY),
        "pipeline_statistics_queries":features.contains(bevy::render::settings::WgpuFeatures::PIPELINE_STATISTICS_QUERY)});
}
fn next_frame(mut tag: ResMut<RenderFrameTag>) {
    tag.id = tag
        .id
        .checked_add(1)
        .expect("render metrics: frame counter exhausted");
}
fn tag_frame(
    tag: Res<RenderFrameTag>,
    cache: Res<PipelineCache>,
    counters: Res<DrawCounters>,
    support: Res<bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport>,
    probe: Res<GpuSupportProbe>,
    mut ctx: RenderContext,
) {
    probe.0.get_or_init(|| {
        use bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode;
        match support.max_supported_mode {
            GpuPreprocessingMode::None => "none",
            GpuPreprocessingMode::PreprocessingOnly => "preprocessing_only",
            GpuPreprocessingMode::Culling => "culling",
        }
    });
    {
        let mut c = counters.0.lock().unwrap();
        c.0 = tag.id;
        assert!(
            c.1.insert(tag.id, Default::default()).is_none(),
            "draw capture: duplicate source frame"
        );
    }
    let pending = u32::try_from(cache.waiting_pipelines().count())
        .expect("render metrics: too many pipelines");
    let mut bytes = Vec::new();
    for value in [
        tag.id,
        u32::from(tag.measure),
        pending,
        u32::from(tag.warmup),
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    // A separate immutable buffer per frame also avoids queue-write aliasing across in-flight frames.
    let buffer = ctx
        .render_device()
        .create_buffer_with_data(&BufferInitDescriptor {
            label: Some("void diagnostic frame tag"),
            contents: &bytes,
            usage: BufferUsages::COPY_SRC,
        });
    let recorder = ctx
        .diagnostic_recorder()
        .expect("render metrics: diagnostic recorder missing");
    for (i, name) in [
        "void_frame_id",
        "void_measure",
        "void_pending_pipelines",
        "void_warmup",
    ]
    .into_iter()
    .enumerate()
    {
        let offset = (i * 4) as u64;
        recorder.record_u32(
            ctx.command_encoder(),
            &buffer.slice(offset..offset + 4),
            name,
        );
    }
}
fn tag_gpu_draw_counts(
    counters: Res<DrawCounters>,
    buffers: Res<bevy::render::batching::gpu_preprocessing::IndirectParametersBuffers>,
    mut ctx: RenderContext,
) {
    if !cfg!(feature = "render-metrics") {
        return;
    }
    let requests = {
        let c = counters.0.lock().unwrap();
        c.1[&c.0].requests.clone()
    };
    let recorder = ctx
        .diagnostic_recorder()
        .expect("draw capture: recorder missing");
    for (i, message) in requests.iter().enumerate() {
        let (offset, _) = gpu_count_offset(message);
        let found: Vec<_> = buffers
            .buffers
            .values()
            .flat_map(|phase| {
                [
                    phase.indexed.batch_sets_buffer(),
                    phase.non_indexed.batch_sets_buffer(),
                ]
                .into_iter()
                .flatten()
            })
            .filter(|buffer| message.contains(&format!("({buffer:?} ")))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "draw capture: GPU count buffer is not a unique mesh batch-set buffer: {message}"
        );
        recorder.record_u32(
            ctx.command_encoder(),
            &found[0].slice(offset..offset + 4),
            format!("void_draw_count_{i}"),
        );
    }
}
pub(crate) fn collect(
    store: Res<DiagnosticsStore>,
    tag: Res<RenderFrameTag>,
    mut metrics: ResMut<RenderMetrics>,
) {
    if tag.measure {
        use bevy::render::diagnostic::MeshAllocatorDiagnosticPlugin as Allocator;
        for path in [
            Allocator::slabs_diagnostic_path(),
            Allocator::slabs_size_diagnostic_path(),
            Allocator::allocations_diagnostic_path(),
        ] {
            if let Some(value) = store
                .get(path)
                .and_then(|diagnostic| diagnostic.measurement())
                .map(|measurement| measurement.value)
            {
                assert!(
                    value.is_finite() && value >= 0.0,
                    "invalid allocator diagnostic"
                );
                let gauge = metrics
                    .allocator_gauges
                    .entry(path.as_str().to_owned())
                    .or_insert((0, 0.0, 0.0, 0.0));
                gauge.0 += 1;
                gauge.1 += value;
                gauge.2 = gauge.2.max(value);
                gauge.3 = value;
            }
        }
    }
    let get = |name: &str| {
        store
            .iter()
            .find(|d| d.path().as_str() == name)
            .and_then(|d| d.measurement())
    };
    let Some(id) = get("render/void_frame_id") else {
        return;
    };
    assert!(
        id.value >= 0.0 && id.value <= u32::MAX as f64 && id.value.fract() == 0.0,
        "render metrics: invalid GPU frame tag"
    );
    let frame = id.value as u32;
    if !metrics.delivered.insert(frame) {
        return;
    }
    let measure = get("render/void_measure").expect("render metrics: incomplete tag");
    let warmup = get("render/void_warmup").expect("render metrics: missing warmup source tag");
    assert_eq!(
        id.time, warmup.time,
        "render metrics: mismatched warmup source tag"
    );
    let pending = get("render/void_pending_pipelines").expect("render metrics: incomplete tag");
    assert_eq!(
        id.time, measure.time,
        "render metrics: mismatched GPU frame tag"
    );
    assert_eq!(
        id.time, pending.time,
        "render metrics: mismatched pipeline tag"
    );
    metrics.last_pending_pipelines = Some(pending.value as u32);
    metrics.last_paths = store
        .iter()
        .filter_map(|d| {
            d.measurement()
                .filter(|m| m.time == id.time)
                .map(|_| d.path().as_str().to_string())
        })
        .collect();
    let draw_frame = metrics
        .counters
        .0
        .lock()
        .unwrap()
        .1
        .remove(&frame)
        .expect("draw capture: missing source frame");
    if (measure.value == 0.0 && warmup.value == 0.0)
        || (measure.value == 1.0
            && metrics
                .limit
                .is_some_and(|limit| metrics.capture.frames() >= limit))
    {
        return;
    }
    assert!(
        measure.value == 1.0 || (measure.value == 0.0 && warmup.value == 1.0),
        "render metrics: invalid capture phase"
    );
    let mut values: Vec<RenderValue> = store
        .iter()
        .filter_map(|d| {
            let m = d.measurement()?;
            let path = d.path().as_str();
            if path.starts_with("render/void_draw_count_") {
                return None;
            }
            if m.time != id.time
                || !path.starts_with("render/")
                || [
                    "render/void_frame_id",
                    "render/void_measure",
                    "render/void_pending_pipelines",
                    "render/void_warmup",
                ]
                .contains(&path)
            {
                return None;
            }
            Some(RenderValue {
                path: path.into(),
                unit: if d.suffix.is_empty() {
                    "count".into()
                } else {
                    d.suffix.to_string()
                },
                value: m.value,
            })
        })
        .collect();
    if values.is_empty() {
        metrics.empty_pass_frames.insert(frame);
    }
    if cfg!(feature = "render-metrics") {
        let counts = draw_frame.counts;
        let gpu_records: u64 = draw_frame
            .requests
            .iter()
            .enumerate()
            .map(|(i, message)| {
                let name = format!("render/void_draw_count_{i}");
                let query = get(&name).expect("draw capture: GPU count readback missing");
                assert_eq!(
                    query.time, id.time,
                    "draw capture: count is from another frame"
                );
                assert!(
                    query.value >= 0.0
                        && query.value.fract() == 0.0
                        && query.value <= u32::MAX as f64,
                    "draw capture: invalid GPU count"
                );
                u64::from((query.value as u32).min(gpu_count_offset(message).1))
            })
            .sum();
        assert_eq!(
            draw_frame.requests.len() as u64,
            counts.gpu_counted_commands,
            "draw capture: missing GPU-count command"
        );
        // These pinned Bevy pass spans enclose exactly one raw fullscreen draw, after all readiness guards.
        let fullscreen = [
            "render/tonemapping/elapsed_cpu",
            "render/upscaling/elapsed_cpu",
        ]
        .iter()
        .filter(|name| get(name).is_some_and(|m| m.time == id.time))
        .count() as u64;
        for (name, value) in [
            ("tracked_draw_commands", counts.commands),
            ("known_draw_records", counts.known_records),
            ("gpu_counted_draw_commands", counts.gpu_counted_commands),
            ("gpu_counted_draw_records", gpu_records),
            ("raw_fullscreen_draw_records", fullscreen),
            (
                "covered_draw_records",
                counts.known_records + gpu_records + fullscreen,
            ),
        ] {
            values.push(RenderValue {
                path: format!("render/submissions/{name}"),
                unit: "count".into(),
                value: value as f64,
            });
        }
    }
    if !values.is_empty() {
        if warmup.value == 1.0 {
            metrics.warmup_capture.sample(frame, values);
        } else {
            metrics.capture.sample(frame, values);
        }
    }
}

struct ErrorLayer(std::sync::Arc<std::sync::Mutex<Vec<String>>>, DrawCounters);
impl<S: bevy::log::tracing::Subscriber> bevy::log::tracing_subscriber::Layer<S> for ErrorLayer {
    fn on_event(
        &self,
        event: &bevy::log::tracing::Event<'_>,
        _ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        let draw = event.metadata().target() == "bevy_render::render_phase::draw_state"
            || event.metadata().target() == "void_draw_submission";
        if !draw && *event.metadata().level() != bevy::log::tracing::Level::ERROR {
            return;
        }
        struct Message(String);
        impl bevy::log::tracing::field::Visit for Message {
            fn record_debug(
                &mut self,
                field: &bevy::log::tracing::field::Field,
                value: &dyn std::fmt::Debug,
            ) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if draw {
            let mut counters = self.1.0.lock().unwrap();
            let frame = counters.0;
            counters
                .1
                .get_mut(&frame)
                .expect("draw capture: event outside source frame")
                .counts
                .observe(&message.0);
            if message.0.starts_with("multi draw indirect count:")
                || message.0.starts_with("multi draw indexed indirect count:")
            {
                counters.1.get_mut(&frame).unwrap().requests.push(message.0);
            }
        } else {
            self.0
                .lock()
                .unwrap()
                .push(format!("{}: {}", event.metadata().target(), message.0));
        }
    }
}

pub(crate) fn error_layer(app: &mut App) -> Option<bevy::log::BoxedLayer> {
    app.world().get_resource::<RenderMetrics>().map(|m| {
        Box::new(ErrorLayer(m.errors.clone(), m.counters.clone())) as bevy::log::BoxedLayer
    })
}

pub(crate) fn quiet_draw_formatter(_app: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    use bevy::log::tracing_subscriber::Layer;
    Some(Box::new(
        bevy::log::tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_filter(bevy::log::tracing_subscriber::filter::filter_fn(|meta| {
                meta.target() != "bevy_render::render_phase::draw_state"
                    && meta.target() != "void_draw_submission"
            })),
    ))
}

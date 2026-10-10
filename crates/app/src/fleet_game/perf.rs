//! Frame timing: the DEV panel's PERFORMANCE readout and `--bench`, the fixed measurement
//! scenarios (guides/profiling.md). Both read the same per-frame samples: `begin_frame` and
//! `end_frame` bracket the main world's frame, `simulate` reports its part in between.
use super::*;
use bevy::diagnostic::DiagnosticsStore;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::time::Instant;
use void_fleet_flight::placement::{Placement, PlacementAttitude, PlacementVelocity, SiteKind};

/// What `simulate` did this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Simulated {
    /// Wall time inside `simulate`: the flight, warp and coast forecast.
    pub sim_seconds: f64,
    /// Simulated seconds the flight advanced this frame.
    pub advanced_seconds: f64,
    /// Time rate in effect after any warp limit, 0 while paused.
    pub set_rate: f64,
}
/// One frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSample {
    /// Wall time from this frame's `First` to the next frame's `First`: the frame's whole
    /// duration, including the wait for rendering and the screen.
    frame_seconds: f64,
    /// Wall time of the main world's schedules, `First` to `Last`. Rendering of the previous
    /// frame (and waiting for the screen) runs beside it on the render thread.
    main_seconds: f64,
    pub simulated: Simulated,
}

/// Statistics over a run of frames.
pub(crate) struct Summary {
    frames: usize,
    seconds: f64,
    frame_avg_ms: f64,
    frame_p95_ms: f64,
    frame_worst_ms: f64,
    main_avg_ms: f64,
    sim_avg_ms: f64,
    actual_rate: f64,
    set_rates: (f64, f64),
}
impl Summary {
    /// None until the samples cover some wall time.
    fn of(samples: &[FrameSample]) -> Option<Self> {
        let seconds: f64 = samples.iter().map(|s| s.frame_seconds).sum();
        if seconds <= 0.0 {
            return None;
        }
        let mut frames: Vec<f64> = samples.iter().map(|s| s.frame_seconds * 1e3).collect();
        frames.sort_by(f64::total_cmp);
        let n = samples.len();
        Some(Self {
            frames: n,
            seconds,
            frame_avg_ms: seconds * 1e3 / n as f64,
            frame_p95_ms: frames[((n as f64 * 0.95).ceil() as usize).max(1) - 1],
            frame_worst_ms: frames[n - 1],
            main_avg_ms: samples.iter().map(|s| s.main_seconds).sum::<f64>() * 1e3 / n as f64,
            sim_avg_ms: samples.iter().map(|s| s.simulated.sim_seconds).sum::<f64>() * 1e3
                / n as f64,
            actual_rate: samples
                .iter()
                .map(|s| s.simulated.advanced_seconds)
                .sum::<f64>()
                / seconds,
            set_rates: samples
                .iter()
                .fold((f64::INFINITY, 0.0_f64), |(lo, hi), s| {
                    (lo.min(s.simulated.set_rate), hi.max(s.simulated.set_rate))
                }),
        })
    }
    fn set_rate_text(&self) -> String {
        match self.set_rates {
            (_, 0.0) => "paused".to_owned(),
            (lo, hi) if lo == hi => format!("{hi}×"),
            (lo, hi) => format!("{lo}–{hi}×"),
        }
    }
}

/// The last `WINDOW_SECONDS` of frames, for the DEV panel. A frame's sample is complete only
/// when the next frame begins, so the newest one is always the previous frame.
#[derive(Resource, Default)]
pub(crate) struct FrameStats {
    recent: VecDeque<FrameSample>,
    /// When this frame's `First` began.
    began: Option<Instant>,
    simulated: Option<Simulated>,
    /// The previous frame, waiting for its end: when it began, its main-world seconds and
    /// simulation.
    ended: Option<(Instant, f64, Simulated)>,
}
const WINDOW_SECONDS: f64 = 2.0;
impl FrameStats {
    /// `simulate`'s part of this frame; exactly once per frame.
    pub fn simulated(&mut self, s: Simulated) {
        assert!(
            [s.sim_seconds, s.advanced_seconds, s.set_rate]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0),
            "frame stats: invalid simulation sample {s:?}"
        );
        assert!(
            self.simulated.replace(s).is_none(),
            "frame stats: simulate reported twice in one frame"
        );
    }
    fn record(&mut self, s: FrameSample) {
        assert!(
            s.frame_seconds.is_finite() && s.frame_seconds >= 0.0 && s.main_seconds.is_finite(),
            "frame stats: invalid frame sample {s:?}"
        );
        void_diagnostics::plot!("frame ms", s.frame_seconds * 1e3);
        void_diagnostics::plot!("main world ms", s.main_seconds * 1e3);
        void_diagnostics::plot!("simulate ms", s.simulated.sim_seconds * 1e3);
        void_diagnostics::plot!("warp set", s.simulated.set_rate);
        void_diagnostics::plot!(
            "warp actual",
            if s.frame_seconds > 0.0 {
                s.simulated.advanced_seconds / s.frame_seconds
            } else {
                0.0
            }
        );
        self.recent.push_back(s);
        while self.recent.len() > 1
            && self.recent.iter().map(|s| s.frame_seconds).sum::<f64>() > WINDOW_SECONDS
        {
            self.recent.pop_front();
        }
    }
    /// The newest complete frame: the previous one.
    pub fn latest(&self) -> Option<FrameSample> {
        self.recent.back().copied()
    }
}
/// Brackets each frame for `FrameStats`; every app that runs `simulate` needs it.
pub(super) fn add_frame_timing(app: &mut App) {
    app.add_systems(First, begin_frame)
        .add_systems(Last, end_frame);
}
/// Ends the previous frame, so its sample covers everything up to this frame: rendering and
/// the wait for the screen included.
fn begin_frame(mut stats: ResMut<FrameStats>) {
    let now = Instant::now();
    if let Some((began, main_seconds, simulated)) = stats.ended.take() {
        stats.record(FrameSample {
            frame_seconds: now.duration_since(began).as_secs_f64(),
            main_seconds,
            simulated,
        });
    }
    stats.began = Some(now);
}
/// Skipped, like `simulate`, in a frame without the window (while the app closes).
fn end_frame(_window: Single<&Window>, mut stats: ResMut<FrameStats>) {
    let began = stats.began.take().expect("frame stats: Last without First");
    let simulated = stats
        .simulated
        .take()
        .expect("frame stats: simulate did not run this frame");
    stats.ended = Some((began, began.elapsed().as_secs_f64(), simulated));
}

/// The top-level render pass a diagnostic times on the GPU. Nested spans are left out so the
/// passes add up.
fn gpu_pass(d: &bevy::diagnostic::Diagnostic) -> Option<&str> {
    d.path()
        .as_str()
        .strip_prefix("render/")?
        .strip_suffix("/elapsed_gpu")
        .filter(|pass| !pass.contains('/'))
}
/// GPU milliseconds of each top-level render pass, per GPU frame, for the frames whose results
/// arrived after `after`, oldest first. Bevy stamps every measurement of one frame with the
/// same time when the results arrive. A pass that runs several times a frame (the air, once per
/// layer) has one measurement per run; they are added up.
fn gpu_frames(
    store: &DiagnosticsStore,
    after: Option<Instant>,
) -> BTreeMap<Instant, BTreeMap<String, f64>> {
    let mut frames: BTreeMap<Instant, BTreeMap<String, f64>> = BTreeMap::new();
    for d in store.iter() {
        let Some(pass) = gpu_pass(d) else { continue };
        for m in d
            .measurements()
            .filter(|m| after.is_none_or(|a| m.time > a))
        {
            assert!(
                m.value.is_finite() && m.value >= 0.0,
                "gpu timing: invalid {pass} measurement {} ms",
                m.value
            );
            *frames
                .entry(m.time)
                .or_default()
                .entry(pass.to_owned())
                .or_default() += m.value;
        }
    }
    frames
}
/// Why the GPU cannot time every top-level render pass, or None when it can. Bevy's recorder
/// writes no timestamp at all without `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS`,
/// and none for spans opened on a render pass (most passes) without
/// `TIMESTAMP_QUERY_INSIDE_PASSES`. A total missing those passes would look complete, so GPU
/// time counts as not measured unless all three are there.
fn gpu_timing_unavailable(device: &bevy::render::renderer::RenderDevice) -> Option<String> {
    use bevy::render::render_resource::WgpuFeatures as F;
    let missing: Vec<_> = [
        (F::TIMESTAMP_QUERY, "TIMESTAMP_QUERY"),
        (
            F::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            "TIMESTAMP_QUERY_INSIDE_ENCODERS",
        ),
        (
            F::TIMESTAMP_QUERY_INSIDE_PASSES,
            "TIMESTAMP_QUERY_INSIDE_PASSES",
        ),
    ]
    .into_iter()
    .filter(|(feature, _)| !device.features().contains(*feature))
    .map(|(_, name)| name)
    .collect();
    (!missing.is_empty()).then(|| format!("the adapter lacks {}", missing.join(", ")))
}
/// Whether this run collects render-pass GPU times: Tracy builds and `--bench`.
fn gpu_timing_on(bench: Option<&Bench>) -> bool {
    cfg!(feature = "profiling") || bench.is_some()
}

/// Line count of the DEV PERFORMANCE readout; the panel reserves exactly this much room.
pub(super) const READOUT_LINES: usize = 5;

/// The DEV PERFORMANCE readout, always `READOUT_LINES` lines.
pub(super) fn readout(
    stats: Res<FrameStats>,
    store: Res<DiagnosticsStore>,
    device: Res<bevy::render::renderer::RenderDevice>,
    bench: Option<Res<Bench>>,
    mut texts: Query<(&ui::Readout, &mut Text)>,
) {
    let window: Vec<_> = stats.recent.iter().copied().collect();
    let text = match Summary::of(&window) {
        None => "PERFORMANCE · no data yet\n\n\n\n".to_owned(),
        Some(s) => {
            let gpu = gpu_frames(&store, None)
                .pop_last()
                .map(|(_, passes)| passes)
                .unwrap_or_default();
            let gpu = if !gpu_timing_on(bench.as_deref()) {
                "gpu    not measured (--features profiling, or --bench)".to_owned()
            } else if let Some(reason) = gpu_timing_unavailable(&device) {
                format!("gpu    not measured: {reason}")
            } else if gpu.is_empty() {
                "gpu    no data yet".to_owned()
            } else {
                let mut passes: Vec<_> = gpu.iter().collect();
                passes.sort_by(|a, b| b.1.total_cmp(a.1));
                format!(
                    "gpu    {:.2} ms · {}",
                    gpu.values().sum::<f64>(),
                    passes
                        .iter()
                        .take(3)
                        .map(|(name, ms)| format!("{name} {ms:.2}"))
                        .collect::<Vec<_>>()
                        .join(" · ")
                )
            };
            format!(
                "PERFORMANCE · last {:.1} s · {} frames\nframe  {:.1} avg · {:.1} worst · main {:.1} ms\nsim    {:.2} ms/frame · {:.0}% of frame\nwarp   actual {:.0}× · set {}\n{gpu}",
                s.seconds,
                s.frames,
                s.frame_avg_ms,
                s.frame_worst_ms,
                s.main_avg_ms,
                s.sim_avg_ms,
                s.sim_avg_ms / s.frame_avg_ms * 100.0,
                s.actual_rate,
                s.set_rate_text(),
            )
        }
    };
    assert_eq!(
        text.split('\n').count(),
        READOUT_LINES,
        "performance readout line count: {text}"
    );
    for (kind, mut t) in &mut texts {
        if *kind == ui::Readout::Performance {
            t.0.clone_from(&text);
        }
    }
}

/// One fixed situation in the main world: only the ship's starting state and the time rate differ.
struct Scenario {
    name: &'static str,
    description: &'static str,
    /// Index into `RATES`.
    rate: usize,
    /// Held paused through the settle and released as measuring begins, so an event right
    /// after the start (a touchdown) falls inside the measurement.
    settle_paused: bool,
    setup: fn(&mut Pilot),
}
const SCENARIOS: [Scenario; 8] = [
    Scenario {
        name: "ground",
        description: "on the launch pad, 1×",
        rate: 0,
        settle_paused: false,
        setup: |_| {},
    },
    Scenario {
        name: "orbit-10k",
        description: "O: 400 km orbit of the observed body, 10,000×",
        rate: 7,
        settle_paused: false,
        setup: launch_orbit,
    },
    Scenario {
        name: "orbit-100k",
        description: "O: 400 km orbit of the observed body, 100,000×",
        rate: 8,
        settle_paused: false,
        setup: launch_orbit,
    },
    Scenario {
        name: "far-100k",
        description: "placed 1,000,000 km above the home body, circular, 100,000×",
        rate: 8,
        settle_paused: false,
        setup: place_far,
    },
    Scenario {
        name: "map",
        description: "O orbit, camera zoomed out to the map (3 radii), 1×",
        rate: 0,
        settle_paused: false,
        setup: map_view,
    },
    Scenario {
        name: "atmosphere",
        description: "placed 2 km over a daylit sea at 150 m/s, camera near its horizon: sea and sky, 1×",
        rate: 0,
        settle_paused: false,
        setup: place_low,
    },
    Scenario {
        name: "reentry",
        description: "placed 40 km over daylit land, 1,800 m/s, 12° down, retrograde, 4×",
        rate: 2,
        settle_paused: false,
        setup: place_reentry,
    },
    Scenario {
        name: "landing",
        description: "placed 30 m over daylit land, 3 m/s down, upright, 1×; held paused through the settle, so the measurement covers the descent, touchdown (about 2 s in) and rest on the ground",
        rate: 0,
        settle_paused: true,
        setup: place_landing,
    },
];
/// Real seconds each scenario runs before measuring (tiles load, warp settles), then measures.
const SETTLE_SECONDS: f64 = 8.0;
const MEASURE_SECONDS: f64 = 10.0;

fn launch_orbit(pilot: &mut Pilot) {
    super::launch_orbit(pilot);
}
fn home_body(pilot: &Pilot) -> String {
    let sim = pilot.flight.session.sim();
    sim.fleet.ephemeris.bodies()[sim.observation_body()]
        .id
        .clone()
}
fn place(pilot: &mut Pilot, placement: Placement) {
    if let Some(reason) = place::apply(pilot, Action::Place { placement }) {
        panic!("bench: placement refused: {reason}");
    }
}
fn site(pilot: &Pilot, body: &str, kind: SiteKind) -> (f64, f64) {
    pilot
        .flight
        .session
        .sim()
        .daylight_site(body, kind)
        .unwrap_or_else(|e| panic!("bench: no daylight {kind:?} site on {body}: {e}"))
}
fn place_far(pilot: &mut Pilot) {
    let body = home_body(pilot);
    let (latitude_degrees, longitude_degrees) = site(pilot, &body, SiteKind::Land);
    let mut placement = Placement {
        body,
        latitude_degrees,
        longitude_degrees,
        altitude_meters: 1e9,
        velocity: PlacementVelocity::Orbital {
            speed: 0.0,
            heading_degrees: 90.0,
            flight_path_degrees: 0.0,
        },
        attitude: PlacementAttitude::Prograde,
    };
    let speed = pilot
        .flight
        .session
        .sim()
        .circular_speed(&placement)
        .unwrap_or_else(|e| panic!("bench: circular speed at 1,000,000 km: {e}"));
    placement.velocity = PlacementVelocity::Orbital {
        speed,
        heading_degrees: 90.0,
        flight_path_degrees: 0.0,
    };
    place(pilot, placement);
}
fn map_view(pilot: &mut Pilot) {
    super::launch_orbit(pilot);
    let sim = pilot.flight.session.sim();
    let radius = sim.fleet.ephemeris.bodies()[sim.observation_body()].radius_meters;
    let distance = sim.presentation.distance;
    // Zoom maps a wheel pixel to a factor exp(0.002) of camera distance.
    pilot.flight.session.execute(Action::View {
        command: ViewCommand::Zoom {
            pixels: -(3.0 * radius / distance).ln() / 0.002,
        },
    });
}
fn surface_placement(
    pilot: &Pilot,
    kind: SiteKind,
    altitude_meters: f64,
    speed: f64,
    flight_path_degrees: f64,
    attitude: PlacementAttitude,
) -> Placement {
    let body = home_body(pilot);
    let (latitude_degrees, longitude_degrees) = site(pilot, &body, kind);
    Placement {
        body,
        latitude_degrees,
        longitude_degrees,
        altitude_meters,
        velocity: PlacementVelocity::Surface {
            speed,
            heading_degrees: 90.0,
            flight_path_degrees,
        },
        attitude,
    }
}
fn place_low(pilot: &mut Pilot) {
    let p = surface_placement(
        pilot,
        SiteKind::Ocean,
        2000.0,
        150.0,
        0.0,
        PlacementAttitude::Prograde,
    );
    place(pilot, p);
    // The camera starts above the ship looking down at the sea; lower it to 8° above the
    // ship's horizon so the sky and clouds are in view too. A long drag first rests it at the top.
    let session = &mut pilot.flight.session;
    for y in [
        1e4,
        -(82f64.to_radians() - void_view::MIN_ANGLE_FROM_UP) / void_view::RADIANS_PER_PIXEL,
    ] {
        session.execute(Action::View {
            command: ViewCommand::Drag { x: 0.0, y },
        });
    }
}
fn place_reentry(pilot: &mut Pilot) {
    let p = surface_placement(
        pilot,
        SiteKind::Land,
        40_000.0,
        1800.0,
        -12.0,
        PlacementAttitude::Retrograde,
    );
    place(pilot, p);
}
fn place_landing(pilot: &mut Pilot) {
    let p = surface_placement(
        pilot,
        SiteKind::Land,
        30.0,
        3.0,
        -90.0,
        PlacementAttitude::Upright,
    );
    place(pilot, p);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Settle,
    Measure,
}
/// GPU times of the render passes over the GPU frames whose results arrived while measuring.
struct GpuTotals {
    /// Arrival time of the newest GPU frame taken.
    seen: Instant,
    frames: usize,
    /// Per pass: GPU ms summed over all frames, and the number of frames it ran in.
    passes: BTreeMap<String, (f64, usize)>,
}
impl GpuTotals {
    fn new(since: Instant) -> Self {
        Self {
            seen: since,
            frames: 0,
            passes: BTreeMap::new(),
        }
    }
    fn take(&mut self, store: &DiagnosticsStore) {
        let frames = gpu_frames(store, Some(self.seen));
        if let Some(newest) = frames.keys().next_back() {
            self.seen = *newest;
        }
        for passes in frames.values() {
            self.frames += 1;
            for (pass, ms) in passes {
                let entry = self.passes.entry(pass.clone()).or_default();
                entry.0 += ms;
                entry.1 += 1;
            }
        }
    }
}

/// `--bench <report>`: the scenarios one after another, then the report and exit.
#[derive(Resource)]
pub(crate) struct Bench {
    report: PathBuf,
    next: usize,
    /// The running scenario, its phase and when that phase began.
    running: Option<(usize, Phase, Instant)>,
    samples: Vec<FrameSample>,
    gpu: GpuTotals,
    results: Vec<String>,
    table: Vec<String>,
}
pub(super) fn add_bench(app: &mut App, report: PathBuf) {
    // With `profiling`, Bevy's `trace_tracy` already adds the render diagnostics (GPU timestamps).
    #[cfg(not(feature = "profiling"))]
    app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    app.insert_resource(Bench {
        report,
        next: 0,
        running: None,
        samples: Vec::new(),
        gpu: GpuTotals::new(Instant::now()),
        results: Vec::new(),
        table: Vec::new(),
    })
    .add_systems(Update, bench.after(super::controls).before(super::simulate));
}
fn start(pilot: &mut Pilot, scenario: &Scenario) {
    super::reset_world(pilot);
    (scenario.setup)(pilot);
    pilot.flight.paused = scenario.settle_paused;
    pilot.flight.rate = 0;
    pilot.notice.0.clear();
}
#[allow(clippy::too_many_arguments)]
fn bench(
    mut bench: ResMut<Bench>,
    mut pilot: Pilot,
    stats: Res<FrameStats>,
    store: Res<DiagnosticsStore>,
    adapter: Res<bevy::render::renderer::RenderAdapterInfo>,
    device: Res<bevy::render::renderer::RenderDevice>,
    window: Single<&Window>,
    mut exit: MessageWriter<AppExit>,
) {
    let bench = &mut *bench;
    let now = Instant::now();
    let Some((index, phase, since)) = bench.running else {
        if bench.next == SCENARIOS.len() {
            return;
        }
        let scenario = &SCENARIOS[bench.next];
        eprintln!("bench: {} — {}", scenario.name, scenario.description);
        start(&mut pilot, scenario);
        bench.running = Some((bench.next, Phase::Settle, now));
        bench.next += 1;
        return;
    };
    let scenario = &SCENARIOS[index];
    let elapsed = now.duration_since(since).as_secs_f64();
    match phase {
        // Like a pilot: run at 1× first so the vessel left on the pad by R and O comes to rest
        // (on-rails warp is refused while it moves), then ask for the rate, again after any
        // refusal. The warp limit may still lower it each frame.
        Phase::Settle if elapsed < SETTLE_SECONDS / 2.0 => pilot.flight.rate = 0,
        Phase::Settle if elapsed < SETTLE_SECONDS => pilot.flight.rate = scenario.rate,
        Phase::Settle => {
            // In a Tracy capture, this plot marks which scenario each measured frame belongs to.
            void_diagnostics::plot!("bench scenario", (index + 1) as f64);
            pilot.flight.paused = false;
            bench.samples.clear();
            bench.gpu = GpuTotals::new(now);
            bench.running = Some((index, Phase::Measure, now));
        }
        Phase::Measure => {
            // The newest complete frame is the previous one, so the samples start with the frame
            // that switched to measuring.
            bench
                .samples
                .push(stats.latest().expect("bench: simulate recorded no frame"));
            bench.gpu.take(&store);
            if elapsed >= MEASURE_SECONDS {
                void_diagnostics::plot!("bench scenario", 0.0);
                finish_scenario(bench, &pilot, scenario, gpu_timing_unavailable(&device));
                bench.running = None;
                if bench.next == SCENARIOS.len() {
                    write_report(bench, &adapter, &window);
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}
/// Resident and peak resident memory of this process, MiB, from /proc/self/status.
fn memory_mib() -> (f64, f64) {
    let status =
        std::fs::read_to_string("/proc/self/status").expect("bench: read /proc/self/status");
    let field = |name: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .unwrap_or_else(|| panic!("bench: no {name} in /proc/self/status"))
            .trim()
            .strip_suffix(" kB")
            .expect("bench: memory in kB")
            .trim()
            .parse::<f64>()
            .expect("bench: memory value")
            / 1024.0
    };
    (field("VmRSS:"), field("VmHWM:"))
}
fn finish_scenario(
    bench: &mut Bench,
    pilot: &Pilot,
    scenario: &Scenario,
    gpu_unavailable: Option<String>,
) {
    let s = Summary::of(&bench.samples).expect("bench: no frames measured");
    let (rss, peak) = memory_mib();
    let sim = pilot.flight.session.sim();
    let ship = sim.fleet.snapshot(&sim.selected);
    let nav = sim.navigation_body(&sim.selected);
    let altitude = sim.altitude(&sim.selected, nav, true);
    let gpu_frames = bench.gpu.frames;
    // Per pass: GPU ms averaged over all GPU-timed frames (a pass missing from a frame counts as
    // 0 ms) and the frames it ran in; with the total. None when the adapter cannot time every pass.
    let gpu = gpu_unavailable.is_none().then(|| {
        assert!(
            gpu_frames > 0,
            "bench: {}: the adapter supports GPU timing but no GPU times arrived in {MEASURE_SECONDS} s",
            scenario.name
        );
        let mut passes: Vec<_> = bench
            .gpu
            .passes
            .iter()
            .map(|(pass, (sum, n))| (pass.clone(), sum / gpu_frames as f64, *n))
            .collect();
        passes.sort_by(|a, b| b.1.total_cmp(&a.1));
        let total: f64 = passes.iter().map(|g| g.1).sum();
        assert!(total.is_finite(), "bench: GPU total {total}");
        (total, passes)
    });
    let gpu_column = gpu
        .as_ref()
        .map_or("—".to_owned(), |(ms, _)| format!("{ms:.2}"));
    bench.table.push(format!(
        "{:<11} {:>6} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>5.0}% {:>8} {:>9}× {:>11} {:>10.0} {:>7.0}",
        scenario.name,
        s.frames,
        s.frame_avg_ms,
        s.frame_p95_ms,
        s.frame_worst_ms,
        s.main_avg_ms,
        s.sim_avg_ms,
        s.sim_avg_ms / s.frame_avg_ms * 100.0,
        gpu_column,
        RATES[scenario.rate],
        s.set_rate_text(),
        s.actual_rate,
        rss,
    ));
    let mut text = format!(
        "== {} · {} ==\nframes {} in {:.1} s · frame {:.2} ms avg · {:.2} ms p95 · {:.2} ms worst\nmain world {:.2} ms/frame (First to Last; rendering runs beside it)\nsimulate {:.2} ms/frame ({:.0}% of frame)\nwarp requested {}× · in effect {} · actual {:.1}×\nmemory RSS {:.0} MiB · process peak {:.0} MiB\nend state: {:?} · {:.0} m above {} ground · {} vessels · notice: {}\n",
        scenario.name,
        scenario.description,
        s.frames,
        s.seconds,
        s.frame_avg_ms,
        s.frame_p95_ms,
        s.frame_worst_ms,
        s.main_avg_ms,
        s.sim_avg_ms,
        s.sim_avg_ms / s.frame_avg_ms * 100.0,
        RATES[scenario.rate],
        s.set_rate_text(),
        s.actual_rate,
        rss,
        peak,
        ship.mode,
        altitude,
        sim.fleet.ephemeris.bodies()[nav].name,
        sim.fleet.vessel_ids().len(),
        if pilot.notice.0.is_empty() {
            "—"
        } else {
            &pilot.notice.0
        },
    );
    match gpu {
        None => writeln!(
            text,
            "gpu not measured: {}",
            gpu_unavailable.expect("GPU totals are missing only when timing is unavailable")
        )
        .expect("format"),
        Some((total, passes)) => {
            writeln!(
                text,
                "gpu {total:.2} ms/frame over {gpu_frames} GPU-timed frames, top-level passes (each averaged over all {gpu_frames}):"
            )
            .expect("format");
            for (pass, ms, n) in &passes {
                writeln!(text, "  {ms:>7.3} ms  {pass}  (ran in {n} frames)").expect("format");
            }
        }
    }
    eprintln!("{text}");
    bench.results.push(text);
}
fn write_report(
    bench: &Bench,
    adapter: &bevy::render::renderer::RenderAdapterInfo,
    window: &Window,
) {
    let mut report = format!(
        "VOID bench report\nbuild: {} · Tracy {} · adapter {} ({:?}) · window {}×{}\neach scenario: {SETTLE_SECONDS} s settle, then {MEASURE_SECONDS} s measured (wall time)\n\n",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        if cfg!(feature = "profiling") {
            "on"
        } else {
            "off"
        },
        adapter.name,
        adapter.backend,
        window.physical_width(),
        window.physical_height(),
    );
    report.push_str(
        "scenario    frames   avg ms   p95 ms worst ms  main ms   sim ms   sim%   gpu ms  requested   in effect   actual ×  RSS MiB\n",
    );
    for row in &bench.table {
        report.push_str(row);
        report.push('\n');
    }
    report.push('\n');
    for r in &bench.results {
        report.push_str(r);
        report.push('\n');
    }
    if let Some(parent) = bench.report.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("bench: create {}: {e}", parent.display()));
    }
    std::fs::write(&bench.report, report)
        .unwrap_or_else(|e| panic!("bench: write {}: {e}", bench.report.display()));
    eprintln!("bench: report written to {}", bench.report.display());
}

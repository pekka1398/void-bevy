//! Frame timing: the DEV panel's PERFORMANCE readout and `--bench`, the fixed measurement
//! scenarios (guides/profiling.md). Both read the same per-frame samples `simulate` records.
use super::*;
use bevy::diagnostic::DiagnosticsStore;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::time::Instant;
use void_fleet_flight::placement::{Placement, PlacementAttitude, PlacementVelocity, SiteKind};

/// One frame as `simulate` saw it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSample {
    /// Wall time since the previous frame began (Bevy's real delta).
    pub frame_seconds: f64,
    /// Wall time inside `simulate`: the flight, warp and coast forecast.
    pub sim_seconds: f64,
    /// Simulated seconds the flight advanced this frame.
    pub advanced_seconds: f64,
    /// Time rate in effect after any warp limit, 0 while paused.
    pub set_rate: f64,
}

/// Statistics over a run of frames.
pub(crate) struct Summary {
    frames: usize,
    seconds: f64,
    frame_avg_ms: f64,
    frame_p95_ms: f64,
    frame_worst_ms: f64,
    sim_avg_ms: f64,
    actual_rate: f64,
    set_rates: (f64, f64),
}
impl Summary {
    fn of(samples: &[FrameSample]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let seconds: f64 = samples.iter().map(|s| s.frame_seconds).sum();
        let mut frames: Vec<f64> = samples.iter().map(|s| s.frame_seconds * 1e3).collect();
        frames.sort_by(f64::total_cmp);
        let n = samples.len();
        Some(Self {
            frames: n,
            seconds,
            frame_avg_ms: seconds * 1e3 / n as f64,
            frame_p95_ms: frames[((n as f64 * 0.95).ceil() as usize).max(1) - 1],
            frame_worst_ms: frames[n - 1],
            sim_avg_ms: samples.iter().map(|s| s.sim_seconds).sum::<f64>() * 1e3 / n as f64,
            actual_rate: samples.iter().map(|s| s.advanced_seconds).sum::<f64>() / seconds,
            set_rates: samples
                .iter()
                .fold((f64::INFINITY, 0.0_f64), |(lo, hi), s| {
                    (lo.min(s.set_rate), hi.max(s.set_rate))
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

/// The last `WINDOW_SECONDS` of frames, for the DEV panel.
#[derive(Resource, Default)]
pub(crate) struct FrameStats {
    recent: VecDeque<FrameSample>,
}
const WINDOW_SECONDS: f64 = 2.0;
impl FrameStats {
    pub fn record(&mut self, s: FrameSample) {
        assert!(
            [
                s.frame_seconds,
                s.sim_seconds,
                s.advanced_seconds,
                s.set_rate
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0),
            "frame stats: invalid sample {s:?}"
        );
        void_diagnostics::plot!("frame ms", s.frame_seconds * 1e3);
        void_diagnostics::plot!("simulate ms", s.sim_seconds * 1e3);
        void_diagnostics::plot!("warp set", s.set_rate);
        void_diagnostics::plot!(
            "warp actual",
            if s.frame_seconds > 0.0 {
                s.advanced_seconds / s.frame_seconds
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
    fn latest(&self) -> Option<FrameSample> {
        self.recent.back().copied()
    }
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
/// The latest GPU milliseconds of each top-level render pass.
fn gpu_passes(store: &DiagnosticsStore) -> BTreeMap<String, f64> {
    store
        .iter()
        .filter_map(|d| Some((gpu_pass(d)?.to_owned(), d.value()?)))
        .collect()
}

/// Line count of the DEV PERFORMANCE readout; the panel reserves exactly this much room.
pub(super) const READOUT_LINES: usize = 5;

/// The DEV PERFORMANCE readout, always `READOUT_LINES` lines.
pub(super) fn readout(
    stats: Res<FrameStats>,
    store: Res<DiagnosticsStore>,
    mut texts: Query<(&ui::Readout, &mut Text)>,
) {
    let window: Vec<_> = stats.recent.iter().copied().collect();
    let text = match Summary::of(&window) {
        None => "PERFORMANCE · no frames yet\n\n\n\n".to_owned(),
        Some(s) => {
            let gpu = gpu_passes(&store);
            let gpu = if gpu.is_empty() {
                "gpu    not measured (--features profiling, or --bench)".to_owned()
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
                "PERFORMANCE · last {:.1} s · {} frames\nframe  {:.1} ms avg · {:.1} ms worst\nsim    {:.2} ms/frame · {:.0}% of frame\nwarp   actual {:.0}× · set {}\n{gpu}",
                s.seconds,
                s.frames,
                s.frame_avg_ms,
                s.frame_worst_ms,
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
    setup: fn(&mut Pilot),
}
const SCENARIOS: [Scenario; 8] = [
    Scenario {
        name: "ground",
        description: "on the launch pad, 1×",
        rate: 0,
        setup: |_| {},
    },
    Scenario {
        name: "orbit-10k",
        description: "O: 400 km orbit of the observed body, 10,000×",
        rate: 7,
        setup: launch_orbit,
    },
    Scenario {
        name: "orbit-100k",
        description: "O: 400 km orbit of the observed body, 100,000×",
        rate: 8,
        setup: launch_orbit,
    },
    Scenario {
        name: "far-100k",
        description: "placed 1,000,000 km above the home body, circular, 100,000×",
        rate: 8,
        setup: place_far,
    },
    Scenario {
        name: "map",
        description: "O orbit, camera zoomed out to the map (3 radii), 1×",
        rate: 0,
        setup: map_view,
    },
    Scenario {
        name: "atmosphere",
        description: "placed 2 km over a daylit sea at 150 m/s, clouds and sea in view, 1×",
        rate: 0,
        setup: place_low,
    },
    Scenario {
        name: "reentry",
        description: "placed 40 km over daylit land, 1,800 m/s, 12° down, retrograde, 4×",
        rate: 2,
        setup: place_reentry,
    },
    Scenario {
        name: "landing",
        description: "placed 30 m over daylit land, 3 m/s down, upright, to touchdown, 1×",
        rate: 0,
        setup: place_landing,
    },
];
/// Real seconds each scenario runs before measuring (tiles load, warp settles), then measures.
const SETTLE_SECONDS: f64 = 4.0;
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
    place::apply(pilot, Action::Place { placement });
    assert!(
        pilot.flight.paused,
        "bench: placement refused: {}",
        pilot.notice.0
    );
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
/// `--bench <report>`: the scenarios one after another, then the report and exit.
#[derive(Resource)]
pub(crate) struct Bench {
    report: PathBuf,
    next: usize,
    /// The running scenario, its phase and when that phase began.
    running: Option<(usize, Phase, Instant)>,
    samples: Vec<FrameSample>,
    /// Per pass: sum of GPU ms, frames counted, time of the last value taken.
    gpu: BTreeMap<String, (f64, usize, Instant)>,
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
        gpu: BTreeMap::new(),
        results: Vec::new(),
        table: Vec::new(),
    })
    .add_systems(Update, bench.after(super::controls).before(super::simulate));
}
fn start(pilot: &mut Pilot, scenario: &Scenario) {
    super::reset_world(pilot);
    (scenario.setup)(pilot);
    pilot.flight.paused = false;
    pilot.flight.rate = scenario.rate;
    pilot.notice.0.clear();
}
#[allow(clippy::too_many_arguments)]
fn bench(
    mut bench: ResMut<Bench>,
    mut pilot: Pilot,
    stats: Res<FrameStats>,
    store: Res<DiagnosticsStore>,
    adapter: Res<bevy::render::renderer::RenderAdapterInfo>,
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
        // Like a pilot pressing "." again after a refusal: the pad vessel left behind by O may
        // still be settling. The warp limit can still lower the rate each frame.
        Phase::Settle if elapsed < SETTLE_SECONDS => pilot.flight.rate = scenario.rate,
        Phase::Settle => {
            // In a Tracy capture, this plot marks which scenario each measured frame belongs to.
            void_diagnostics::plot!("bench scenario", (index + 1) as f64);
            bench.samples.clear();
            bench.gpu.clear();
            bench.running = Some((index, Phase::Measure, now));
        }
        Phase::Measure => {
            bench
                .samples
                .push(stats.latest().expect("bench: simulate recorded no frame"));
            for d in store.iter() {
                let (Some(pass), Some(m)) = (gpu_pass(d), d.measurement()) else {
                    continue;
                };
                let entry = bench.gpu.entry(pass.to_owned()).or_insert((0.0, 0, since));
                if m.time > entry.2 {
                    *entry = (entry.0 + m.value, entry.1 + 1, m.time);
                }
            }
            if elapsed >= MEASURE_SECONDS {
                void_diagnostics::plot!("bench scenario", 0.0);
                finish_scenario(bench, &pilot, scenario);
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
fn finish_scenario(bench: &mut Bench, pilot: &Pilot, scenario: &Scenario) {
    let s = Summary::of(&bench.samples).expect("bench: no frames measured");
    let (rss, peak) = memory_mib();
    let sim = pilot.flight.session.sim();
    let ship = sim.fleet.snapshot(&sim.selected);
    let nav = sim.navigation_body(&sim.selected);
    let altitude = sim.altitude(&sim.selected, nav, true);
    let mut gpu: Vec<_> = bench
        .gpu
        .iter()
        .map(|(pass, (sum, n, _))| (pass.clone(), sum / *n as f64, *n))
        .collect();
    gpu.sort_by(|a, b| b.1.total_cmp(&a.1));
    let gpu_total: f64 = gpu.iter().map(|g| g.1).sum();
    bench.table.push(format!(
        "{:<11} {:>6} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>5.0}% {:>8.2} {:>9}× {:>11} {:>10.0} {:>7.0}",
        scenario.name,
        s.frames,
        s.frame_avg_ms,
        s.frame_p95_ms,
        s.frame_worst_ms,
        s.sim_avg_ms,
        s.sim_avg_ms / s.frame_avg_ms * 100.0,
        gpu_total,
        RATES[scenario.rate],
        s.set_rate_text(),
        s.actual_rate,
        rss,
    ));
    let mut text = format!(
        "== {} · {} ==\nframes {} in {:.1} s · frame {:.2} ms avg · {:.2} ms p95 · {:.2} ms worst\nsimulate {:.2} ms/frame ({:.0}% of frame)\nwarp requested {}× · in effect {} · actual {:.1}×\nmemory RSS {:.0} MiB · process peak {:.0} MiB\nend state: {:?} · {:.0} m above {} ground · {} vessels · notice: {}\ngpu {:.2} ms/frame over top-level passes:\n",
        scenario.name,
        scenario.description,
        s.frames,
        s.seconds,
        s.frame_avg_ms,
        s.frame_p95_ms,
        s.frame_worst_ms,
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
        gpu_total,
    );
    for (pass, ms, n) in &gpu {
        writeln!(text, "  {ms:>7.3} ms  {pass}  ({n} frames)").expect("format");
    }
    if gpu.is_empty() {
        text.push_str("  none recorded (the adapter has no timestamp queries)\n");
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
        "scenario    frames   avg ms   p95 ms worst ms   sim ms   sim%   gpu ms  requested   in effect   actual ×  RSS MiB\n",
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

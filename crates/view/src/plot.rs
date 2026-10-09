//! Orbit-lab plotting semantics shared by the main map. Every sample is transformed at its own
//! time, then placed using the frame now. These are presentation coordinates, never physics.
use crate::frame_to_ecliptic;
use glam::DVec3;
use void_orbit::{
    EphemerisSource, FrameEvaluator, FrameSpec, OrbitNode, Trajectory, find_apsides, find_nodes,
    to_frame,
};

#[derive(Clone, Copy, PartialEq)]
struct SourceSignature {
    spec: FrameSpec,
    plotting_system: usize,
    physics_system: usize,
    offset: void_frames::SplitPosition,
}
impl SourceSignature {
    fn new(eph: &dyn EphemerisSource, spec: FrameSpec, system: usize) -> Self {
        Self {
            spec,
            plotting_system: system,
            physics_system: eph.origin_system().0,
            offset: eph.physics_offset(),
        }
    }
}

/// Visual tolerance in the selected moving frame, in meters. A camera may convert
/// a pixel target at its focus depth to this value; this is not a screen-space bound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotQuality {
    pub max_error_meters: f64,
    pub max_points: usize,
}
impl Default for PlotQuality {
    fn default() -> Self {
        Self {
            max_error_meters: 100.0,
            max_points: 8192,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PlotSamplingStatus {
    /// The whole path is retained, but some tested chords exceed the tolerance.
    pub point_limit_reached: bool,
    /// Requested tolerance could not be resolved within depth/f64 time limits.
    pub resolution_limit_reached: bool,
    /// Maximum error among tested chords, including ones subsequently subdivided.
    /// Quarter-point probes are an estimator, not a global error bound.
    pub max_observed_error_meters: f64,
}

// Largest measured chord error first; time breaks ties deterministically.
struct PlotInterval {
    left: (f64, DVec3),
    right: (f64, DVec3),
    middle: (f64, DVec3),
    error: f64,
    depth: u32,
}
impl PartialEq for PlotInterval {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for PlotInterval {}
impl PartialOrd for PlotInterval {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PlotInterval {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.error
            .total_cmp(&other.error)
            .then_with(|| other.left.0.total_cmp(&self.left.0))
    }
}

fn adaptive_samples(
    trajectory: &Trajectory,
    quality: PlotQuality,
    start: f64,
    mut sample: impl FnMut(f64) -> DVec3,
) -> (Vec<(f64, DVec3)>, PlotSamplingStatus) {
    assert!(quality.max_error_meters.is_finite() && quality.max_error_meters > 0.0);
    assert!(quality.max_points >= 2);
    let end = trajectory.last_time();
    let mut checked = |t: f64| {
        let p = sample(t);
        assert!(p.is_finite(), "non-finite plot sample");
        p
    };
    let mut status = PlotSamplingStatus::default();
    let mut lo = 0;
    let mut hi = trajectory.count();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if trajectory.time(mid) <= start {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // Establish coverage of the WHOLE time range before refining any section.
    // Bound seed work independently of source length: small sources retain every
    // knot; dense sources use knot-index strata plus a uniform temporal grid.
    // Narrow features between those strata/probes can be missed: this is a visual
    // error estimator, never a bound on physics or all possible curve extrema.
    let seed_budget = (quality.max_points / 2).clamp(2, 1026);
    let interior_budget = seed_budget - 2;
    let knot_budget = interior_budget.div_ceil(2);
    let grid_budget = interior_budget - knot_budget;
    let remaining_knots = trajectory.count().saturating_sub(lo + 1);
    let mut times = vec![start, end];
    if remaining_knots <= knot_budget {
        times.extend((lo..lo + remaining_knots).map(|i| trajectory.time(i)));
    } else {
        for i in 0..knot_budget {
            // Midpoint of each equal index stratum, with no traversal of skipped knots.
            let index = lo + ((2 * i + 1) * remaining_knots / (2 * knot_budget));
            times.push(trajectory.time(index));
        }
    }
    for i in 1..=grid_budget {
        times.push(start + (end - start) * i as f64 / (grid_budget + 1) as f64);
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut points: Vec<_> = times.into_iter().map(|t| (t, checked(t))).collect();
    let mut pending = std::collections::BinaryHeap::new();
    let mut inspect = |left: (f64, DVec3), right: (f64, DVec3), depth: u32| {
        let mut error = 0.0_f64;
        let mut middle = left;
        for fraction in [0.25, 0.5, 0.75] {
            let time = left.0 + (right.0 - left.0) * fraction;
            let p = checked(time);
            error = error.max((p - left.1.lerp(right.1, fraction)).length());
            if fraction == 0.5 {
                middle = (time, p);
            }
        }
        PlotInterval {
            left,
            right,
            middle,
            error,
            depth,
        }
    };
    for pair in points.windows(2) {
        pending.push(inspect(pair[0], pair[1], 0));
    }
    while let Some(interval) = pending.pop() {
        status.max_observed_error_meters = status.max_observed_error_meters.max(interval.error);
        if interval.error <= quality.max_error_meters {
            break;
        }
        if points.len() >= quality.max_points {
            status.point_limit_reached = true;
            break;
        }
        if interval.depth >= 32
            || interval.middle.0 <= interval.left.0
            || interval.middle.0 >= interval.right.0
        {
            status.resolution_limit_reached = true;
            continue;
        }
        points.push(interval.middle);
        pending.push(inspect(interval.left, interval.middle, interval.depth + 1));
        pending.push(inspect(interval.middle, interval.right, interval.depth + 1));
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    // At most 7 * max_points position evaluations, O(max_points) auxiliary memory.
    (points, status)
}

#[derive(Clone, Copy, PartialEq)]
struct EventSignature {
    source: SourceSignature,
    generation: u64,
    first: f64,
    end: f64,
    reference: usize,
    now: f64,
}

#[derive(Default)]
pub struct PlotPath {
    signature: Option<(SourceSignature, u64, f64, f64, PlotQuality)>,
    cache: Vec<(f64, DVec3)>,
    pub sampling_status: PlotSamplingStatus,
    last_time: Option<f64>,
    event_signature: Option<EventSignature>,
    event_nodes: Vec<OrbitNode>,
    event_apsides: Vec<(String, DVec3)>,
    pub points: Vec<DVec3>,
    pub apsides: Vec<(String, DVec3)>,
    pub nodes: Vec<(OrbitNode, DVec3)>,
}
impl PlotPath {
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        eph: &dyn EphemerisSource,
        trajectory: &Trajectory,
        spec: FrameSpec,
        generation: u64,
        now: f64,
        origin: DVec3,
        reference: usize,
    ) {
        self.update_with_quality(
            eph,
            trajectory,
            spec,
            generation,
            now,
            origin,
            reference,
            PlotQuality::default(),
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn update_with_quality(
        &mut self,
        eph: &dyn EphemerisSource,
        trajectory: &Trajectory,
        spec: FrameSpec,
        generation: u64,
        now: f64,
        origin: DVec3,
        reference: usize,
        quality: PlotQuality,
    ) {
        self.points.clear();
        self.apsides.clear();
        self.nodes.clear();
        if trajectory.count() < 2 || trajectory.last_time() <= now {
            self.sampling_status = PlotSamplingStatus::default();
            return;
        }
        let start = now.max(trajectory.first_time());
        let end = trajectory.last_time();
        let signature = (
            SourceSignature::new(eph, spec, eph.system_of(reference).0),
            generation,
            trajectory.first_time(),
            end,
            quality,
        );
        let mut evaluator = FrameEvaluator::new_in_system(eph, spec, eph.system_of(reference));
        if self.signature != Some(signature) || self.last_time.is_some_and(|t| now < t) {
            (self.cache, self.sampling_status) =
                adaptive_samples(trajectory, quality, start, |t| {
                    to_frame(&evaluator.evaluate(eph, t), trajectory.sample(t).0)
                });
            self.signature = Some(signature);
        }
        self.last_time = Some(now);
        let here = evaluator.evaluate(eph, now);
        let focus = to_frame(&here, origin);
        let place = |p: DVec3| frame_to_ecliptic(&here.axes, p - focus);
        self.points.push(place(to_frame(
            &evaluator.evaluate(eph, start),
            trajectory.sample(start).0,
        )));
        self.points.extend(
            self.cache
                .iter()
                .filter(|(t, _)| *t > start && *t < end)
                .map(|(_, p)| place(*p)),
        );
        self.points.push(place(to_frame(
            &evaluator.evaluate(eph, end),
            trajectory.sample(end).0,
        )));
        let event_signature = EventSignature {
            source: signature.0,
            generation,
            first: trajectory.first_time(),
            end,
            reference,
            now,
        };
        if self.event_signature != Some(event_signature) {
            self.event_nodes = find_nodes(trajectory, eph, spec, eph.system_of(reference), now, 16);
            self.event_apsides = find_apsides(trajectory, eph, reference, now, 2)
                .into_iter()
                .map(|a| {
                    let label = format!(
                        "{:?} {:.1} km",
                        a.kind,
                        (a.distance_meters - eph.bodies()[reference].radius_meters) / 1000.0
                    );
                    (
                        label,
                        to_frame(&evaluator.evaluate(eph, a.time), a.position),
                    )
                })
                .collect();
            self.event_signature = Some(event_signature);
        }
        // Paused redraws and camera pans only place the same exact-time events;
        // running simulation retains the existing exact-now event search semantics.
        self.nodes
            .extend(self.event_nodes.iter().map(|n| (*n, place(n.in_frame))));
        self.apsides.extend(
            self.event_apsides
                .iter()
                .map(|(label, p)| (label.clone(), place(*p))),
        );
    }
}
/// Frame-space body samples refreshed on simulation time, independent of rendering cadence.
#[derive(Default)]
pub struct BodyPlots {
    signature: Option<(SourceSignature, u64, usize, u64, u64)>,
    samples: Vec<Vec<DVec3>>,
}
impl BodyPlots {
    pub fn update(
        &mut self,
        eph: &dyn EphemerisSource,
        spec: FrameSpec,
        now: f64,
        origin: DVec3,
    ) -> Vec<Vec<DVec3>> {
        self.update_in_system(eph, spec, eph.origin_system(), now, origin)
    }
    pub fn update_in_system(
        &mut self,
        eph: &dyn EphemerisSource,
        spec: FrameSpec,
        system: void_frames::SystemId,
        now: f64,
        origin: DVec3,
    ) -> Vec<Vec<DVec3>> {
        let signature = (
            SourceSignature::new(eph, spec, system.0),
            (now / 2.0).floor().to_bits(),
            eph.bodies().len(),
            eph.start_time().to_bits(),
            eph.end_time().to_bits(),
        );
        let mut evaluator = FrameEvaluator::new_in_system(eph, spec, system);
        if self.signature != Some(signature) {
            self.signature = Some(signature);
            let start = (now - 86400.0).max(eph.start_time());
            let end = (now + 86400.0).min(eph.end_time());
            self.samples = vec![Vec::new(); eph.bodies().len()];
            if end > start {
                for i in 0..=256 {
                    let t = start + (end - start) * i as f64 / 256.0;
                    let frame = evaluator.evaluate(eph, t);
                    for (body, samples) in self.samples.iter_mut().enumerate() {
                        if !spec.centred_on(body) {
                            samples.push(to_frame(&frame, evaluator.position(body)));
                        }
                    }
                }
            }
        }
        let here = evaluator.evaluate(eph, now);
        let focus = to_frame(&here, origin);
        self.samples
            .iter()
            .map(|samples| {
                samples
                    .iter()
                    .map(|&p| frame_to_ecliptic(&here.axes, p - focus))
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod sampling_tests {
    use super::*;

    fn short_curve() -> Trajectory {
        let mut path = Trajectory::new();
        for (t, y) in [(0.0, 0.0), (1.0, 100.0), (2.0, 0.0), (10000.0, 0.0)] {
            path.append(t, &[t, y, 0.0, 1.0, 0.0, 0.0]);
        }
        path
    }
    fn chord_at(points: &[(f64, DVec3)], time: f64) -> DVec3 {
        let i = points
            .partition_point(|(t, _)| *t <= time)
            .saturating_sub(1)
            .min(points.len() - 2);
        let (a, b) = (points[i], points[i + 1]);
        a.1.lerp(b.1, (time - a.0) / (b.0 - a.0))
    }
    #[test]
    fn localized_curve_witness_and_adaptive_error() {
        let path = short_curve();
        let old: Vec<_> = (0..=512)
            .map(|i| {
                let t = path.last_time() * i as f64 / 512.0;
                (t, path.sample(t).0)
            })
            .collect();
        assert_eq!((chord_at(&old, 1.0) - path.sample(1.0).0).length(), 100.0);
        let quality = PlotQuality {
            max_error_meters: 0.1,
            max_points: 8192,
        };
        let (points, status) = adaptive_samples(&path, quality, 0.0, |t| path.sample(t).0);
        assert!(!status.point_limit_reached);
        let mut maximum_error = 0.0_f64;
        for i in 0..=2000 {
            let time = i as f64 / 1000.0;
            let error = (chord_at(&points, time) - path.sample(time).0).length();
            maximum_error = maximum_error.max(error);
            assert!(error <= 0.11);
        }
        eprintln!(
            "short-curve witness: old_points={} old_error=100m adaptive_points={} dense_max_error={maximum_error}m",
            old.len(),
            points.len()
        );
        assert_eq!(points.first().unwrap().0, 0.0);
        assert_eq!(points.last().unwrap().0, 10000.0);
        assert!(points.len() <= quality.max_points);
        assert!(points.windows(2).all(|p| p[0].0 < p[1].0));
    }
    #[test]
    fn limited_path_retains_endpoint_and_reports_limit() {
        let path = short_curve();
        let (points, status) = adaptive_samples(
            &path,
            PlotQuality {
                max_error_meters: 0.01,
                max_points: 8,
            },
            0.0,
            |t| path.sample(t).0,
        );
        assert!(status.point_limit_reached);
        assert_eq!(points.len(), 8);
        assert_eq!(points.last().unwrap().0, path.last_time());
        assert!(points.iter().all(|(_, p)| p.is_finite()));
    }
    #[test]
    fn translation_rotation_and_future_start_preserve_sampling() {
        let path = short_curve();
        let quality = PlotQuality {
            max_error_meters: 0.1,
            max_points: 8192,
        };
        let (base, _) = adaptive_samples(&path, quality, 0.5, |t| path.sample(t).0);
        let rotation = glam::DQuat::from_rotation_x(0.5);
        let offset = DVec3::splat(1e9);
        let (shifted, _) = adaptive_samples(&path, quality, 0.5, |t| {
            rotation * path.sample(t).0 + offset
        });
        assert_eq!(base.len(), shifted.len());
        assert_eq!(base[0].0, 0.5);
        for (a, b) in base.iter().zip(&shifted) {
            assert_eq!(a.0, b.0);
            assert!((rotation * a.1 - (b.1 - offset)).length() < 1e-6);
        }
    }
    #[test]
    fn dense_source_preserves_late_curve_and_bounds_evaluations() {
        let mut path = Trajectory::new();
        let end = 131072.0;
        for i in 0..=131072 {
            let t = i as f64;
            // An expensive early feature must not consume the late curve's coverage.
            let (y, vy) = if t < 4096.0 {
                ((t * 0.1).sin() * 100.0, (t * 0.1).cos() * 10.0)
            } else if t > 120000.0 {
                let phase = (t - 120000.0) / (end - 120000.0) * std::f64::consts::PI;
                (
                    phase.sin() * 1000.0,
                    phase.cos() * 1000.0 * std::f64::consts::PI / (end - 120000.0),
                )
            } else {
                (0.0, 0.0)
            };
            path.append(t, &[t, y, 0.0, 1.0, vy, 0.0]);
        }
        let quality = PlotQuality {
            max_error_meters: 0.001,
            max_points: 256,
        };
        let mut evaluations = 0;
        let (points, status) = adaptive_samples(&path, quality, 0.0, |t| {
            evaluations += 1;
            path.sample(t).0
        });
        assert!(status.point_limit_reached);
        assert_eq!(points.len(), quality.max_points);
        assert!(evaluations <= 7 * quality.max_points);
        assert!(points.iter().filter(|(t, _)| *t > 120000.0).count() >= 8);
        let late_peak = (120000.0 + end) / 2.0;
        assert!((chord_at(&points, late_peak) - path.sample(late_peak).0).length() < 10.0);
        assert_eq!(points.first().unwrap().0, 0.0);
        assert_eq!(points.last().unwrap().0, end);
        eprintln!(
            "dense-source witness: source_knots={} plot_points={} evaluations={} late_peak_error={}m",
            path.count(),
            points.len(),
            evaluations,
            (chord_at(&points, late_peak) - path.sample(late_peak).0).length()
        );

        let mut evaluations = 0;
        let (straight, status) = adaptive_samples(&path, quality, 0.0, |t| {
            evaluations += 1;
            DVec3::X * t
        });
        assert!(!status.point_limit_reached);
        assert!(straight.len() <= quality.max_points / 2);
        assert!(evaluations <= 7 * quality.max_points);
        assert_eq!(straight.last().unwrap().0, end);
    }

    #[test]
    #[should_panic(expected = "non-finite plot sample")]
    fn nonfinite_samples_fail_explicitly() {
        adaptive_samples(&short_curve(), PlotQuality::default(), 0.0, |_| DVec3::NAN);
    }
}

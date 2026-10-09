//! Plotting semantics shared by the main map. Every sample is transformed at its own
//! time, then placed using the frame now. These are presentation coordinates, never physics.
use crate::{PathCache, frame_to_ecliptic};
use glam::DVec3;
use void_orbit::{EphemerisSource, FrameEvaluator, FrameSpec, Trajectory, find_apsides, to_frame};

#[derive(Default)]
pub struct PlotPath {
    signature: Option<(FrameSpec, u64, f64, f64)>,
    cache: Option<PathCache>,
    last_time: Option<f64>,
    pub points: Vec<DVec3>,
    pub apsides: Vec<(String, DVec3)>,
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
        self.points.clear();
        self.apsides.clear();
        if trajectory.count() < 2 || trajectory.last_time() <= now {
            return;
        }
        let start = now.max(trajectory.first_time());
        let end = trajectory.last_time();
        let dt = ((end - trajectory.first_time()) / 512.0).max(0.01);
        let signature = (spec, generation, trajectory.first_time(), end);
        if self.signature != Some(signature) || self.last_time.is_some_and(|t| now < t) {
            self.cache = Some(PathCache::new(dt));
            self.signature = Some(signature);
        }
        self.last_time = Some(now);
        let mut evaluator = FrameEvaluator::new(eph, spec);
        let here = evaluator.evaluate(eph, now);
        let focus = to_frame(&here, origin);
        let place = |p: DVec3| frame_to_ecliptic(&here.axes, p - focus);
        let cache = self.cache.as_mut().expect("plot cache");
        cache.update(start, end, |t| {
            to_frame(&evaluator.evaluate(eph, t), trajectory.sample(t).0)
        });
        self.points.push(place(to_frame(
            &evaluator.evaluate(eph, start),
            trajectory.sample(start).0,
        )));
        self.points.extend(cache.samples().map(|(_, p)| place(p)));
        self.points.push(place(to_frame(
            &evaluator.evaluate(eph, end),
            trajectory.sample(end).0,
        )));
        self.apsides = find_apsides(trajectory, eph, reference, now, 2)
            .into_iter()
            .map(|a| {
                let label = format!(
                    "{:?} {:.1} km",
                    a.kind,
                    (a.distance_meters - eph.bodies()[reference].radius_meters) / 1000.0
                );
                (
                    label,
                    place(to_frame(&evaluator.evaluate(eph, a.time), a.position)),
                )
            })
            .collect();
    }
}
/// Frame-space body samples refreshed on simulation time, independent of rendering cadence.
#[derive(Default)]
pub struct BodyPlots {
    signature: Option<(FrameSpec, u64, usize)>,
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
        let signature = (spec, (now / 2.0).floor().to_bits(), eph.bodies().len());
        let mut evaluator = FrameEvaluator::new(eph, spec);
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

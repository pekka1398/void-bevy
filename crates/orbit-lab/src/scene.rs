//! CPU side of TS SceneView: time-local plotting, incremental path caches, fade colours,
//! trajectory events, and target/plan end points. No alternate orbital integrator.
use crate::{Focus, OrbitLab};
use glam::DVec3;
use void_orbit::*;
use void_view::PathCache;

pub const HISTORY: &str = "#ff5a5a";
pub const PREDICTION: &str = "#4fc8ff";
pub const PLAN: &str = "#ffc857";
pub const BURN: &str = "#ff6a2a";
pub const VESSEL: &str = "#7dffb0";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Body,
    History,
    Prediction,
    Plan,
    Target,
}
pub struct PlotPath {
    pub kind: PathKind,
    pub color: String,
    pub points: Vec<(f64, DVec3)>,
    pub fade_start: f64,
    pub fade_span: f64,
    pub burns: Vec<(f64, f64)>,
}
pub struct PlotBody {
    pub index: usize,
    pub position: DVec3,
    pub axes: [DVec3; 3],
}
#[derive(Clone, Debug)]
pub struct PlotMarker {
    pub position: DVec3,
    pub text: String,
    pub color: String,
    pub focus: Option<Focus>,
    pub priority: f64,
    pub ring: bool,
}
pub struct PlotScene {
    pub bodies: Vec<PlotBody>,
    pub paths: Vec<PlotPath>,
    pub markers: Vec<PlotMarker>,
    pub vessel: DVec3,
    pub thrust: Option<DVec3>,
    pub target_gap: Option<f64>,
}

#[derive(Default)]
pub struct SceneView {
    signature: Option<(FrameSpec, f64, f64, f64)>,
    bodies: Vec<Option<PathCache>>,
    vessel: Option<PathCache>,
    prediction: Option<PathCache>,
    plan: Option<PathCache>,
    target: Option<PathCache>,
    prediction_generation: u64,
    plan_generation: u64,
    target_generation: u64,
    target_index: Option<usize>,
}
impl SceneView {
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }
    pub fn update(&mut self, lab: &mut OrbitLab) -> PlotScene {
        let signature = (
            lab.frame,
            lab.trail_span,
            lab.vessel_span,
            lab.sim.prediction_horizon_seconds(),
        );
        if self.signature != Some(signature) {
            self.invalidate();
            self.signature = Some(signature);
        }
        let sim = &mut lab.sim;
        let now = sim.time;
        let eph = &sim.ephemeris;
        let mut evaluator = FrameEvaluator::new(eph, lab.frame);
        let period = evaluator.rotation_period_seconds(eph, now);
        let here = evaluator.evaluate(eph, now);
        let vessel = to_frame(&here, sim.vessel_position_at(now));
        let focus = match lab.focus {
            Focus::Vessel => vessel,
            Focus::Body(i) => to_frame(&here, evaluator.position(i)),
        };
        let body_positions: Vec<_> = sim
            .system
            .bodies
            .iter()
            .map(|b| to_frame(&here, evaluator.position(b.index)))
            .collect();
        let natural = vessel_period(sim);
        let interval = |span: f64| (natural / 128.0).min(period / 128.0).max(span / 6000.0);
        let mut paths = Vec::new();
        let mut bodies = Vec::new();
        let mut markers = Vec::new();
        if self.bodies.len() != sim.system.bodies.len() {
            self.bodies = (0..sim.system.bodies.len()).map(|_| None).collect();
        }
        for body in &sim.system.bodies {
            let index = body.index;
            let position = body_positions[index];
            let axes =
                body_orientation(&body.rotation, now).map(|axis| direction_to_frame(&here, axis));
            bodies.push(PlotBody {
                index,
                position: position - focus,
                axes,
            });
            markers.push(PlotMarker {
                position: position - focus,
                text: body.name.clone(),
                color: body.color.clone(),
                focus: Some(Focus::Body(index)),
                priority: if lab.focus == Focus::Body(index) {
                    1e60
                } else {
                    body.mass_kg
                },
                ring: false,
            });
            let span = lab
                .trail_span
                .min(body.orbit_period_seconds.unwrap_or(lab.trail_span))
                .min(now - eph.start_time());
            let dt = (body.orbit_period_seconds.unwrap_or(lab.trail_span) / 128.0)
                .min(period / 128.0)
                .max(lab.trail_span / 6000.0);
            let cache = self.bodies[index].get_or_insert_with(|| PathCache::new(dt));
            cache.update(now - span, now, |t| {
                let f = evaluator.evaluate(eph, t);
                to_frame(&f, evaluator.position(index))
            });
            if !lab.frame.centred_on(index) {
                paths.push(path(
                    cache,
                    focus,
                    None,
                    Some((now, position)),
                    PathKind::Body,
                    &body.color,
                    now,
                    0.0,
                    &[],
                ));
            }
        }
        let cache = self
            .vessel
            .get_or_insert_with(|| PathCache::new(interval(lab.vessel_span)));
        let from = now - lab.vessel_span.min(now - sim.history.first_time());
        cache.update(from, now, |t| {
            to_frame(&evaluator.evaluate(eph, t), sim.vessel_position_at(t))
        });
        paths.push(path(
            cache,
            focus,
            None,
            Some((now, vessel)),
            PathKind::History,
            HISTORY,
            now,
            lab.vessel_span,
            &[],
        ));
        if sim.impact.is_none() && sim.prediction.count() > 1 {
            if self.prediction.is_none() || self.prediction_generation != sim.prediction_generation
            {
                self.prediction_generation = sim.prediction_generation;
                self.prediction = Some(PathCache::new(interval(sim.prediction_horizon_seconds())));
            }
            let end = sim.prediction.last_time();
            let cache = self.prediction.as_mut().unwrap();
            cache.update(now, end, |t| {
                to_frame(&evaluator.evaluate(eph, t), sim.prediction.sample(t).0)
            });
            let tail = to_frame(
                &evaluator.evaluate(eph, end),
                sim.prediction.position(sim.prediction.count() - 1),
            );
            paths.push(path(
                cache,
                focus,
                Some((now, vessel)),
                Some((end, tail)),
                PathKind::Prediction,
                PREDICTION,
                now,
                sim.prediction_horizon_seconds(),
                &[],
            ));
        } else {
            self.prediction = None;
        }
        let mut target_gap = None;
        let trajectory = &sim.plan.trajectory;
        if sim.impact.is_none()
            && sim.plan.count() > 0
            && trajectory.count() > 1
            && trajectory.last_time() > now
        {
            if self.plan.is_none() || self.plan_generation != sim.plan.generation {
                self.plan_generation = sim.plan.generation;
                self.plan = Some(PathCache::new(interval(sim.plan.end_time() - now)));
            }
            let end = trajectory.last_time();
            let f = evaluator.evaluate(eph, end);
            let tail = to_frame(&f, trajectory.position(trajectory.count() - 1));
            let cache = self.plan.as_mut().unwrap();
            cache.update(now.max(trajectory.first_time()), end, |t| {
                to_frame(&evaluator.evaluate(eph, t), trajectory.sample(t).0)
            });
            let burns: Vec<_> = sim
                .plan
                .burns()
                .iter()
                .map(|b| (b.start_time, b.end_time))
                .collect();
            paths.push(path(
                cache,
                focus,
                Some((now, vessel)),
                Some((end, tail)),
                PathKind::Plan,
                PLAN,
                now,
                sim.plan.end_time() - now,
                &burns,
            ));
            markers.push(PlotMarker {
                position: tail - focus,
                text: format!("plan end +{}", duration(end - now)),
                color: PLAN.into(),
                focus: None,
                priority: 3e49,
                ring: true,
            });
            if let Some(index) = lab.target {
                let target = eph.body_position(index, end);
                let at = to_frame(&f, target);
                target_gap = Some(
                    (trajectory.position(trajectory.count() - 1) - target).length()
                        - sim.system.bodies[index].radius_meters,
                );
                markers.push(PlotMarker {
                    position: at - focus,
                    text: format!("{} +{}", sim.system.bodies[index].name, duration(end - now)),
                    color: PLAN.into(),
                    focus: None,
                    priority: 3e49,
                    ring: true,
                });
                if !lab.frame.centred_on(index) {
                    if self.target.is_none()
                        || self.target_generation != self.plan_generation
                        || self.target_index != lab.target
                    {
                        self.target_generation = self.plan_generation;
                        self.target_index = lab.target;
                        self.target = Some(PathCache::new(cache.interval_seconds));
                    }
                    let cache = self.target.as_mut().unwrap();
                    cache.update(now, end, |t| {
                        to_frame(&evaluator.evaluate(eph, t), eph.body_position(index, t))
                    });
                    paths.push(path(
                        cache,
                        focus,
                        Some((now, body_positions[index])),
                        Some((end, at)),
                        PathKind::Target,
                        PLAN,
                        now,
                        sim.plan.end_time() - now,
                        &[],
                    ));
                } else {
                    self.target = None;
                }
            } else {
                self.target = None;
            }
        } else {
            self.plan = None;
            self.target = None;
        }
        let reference = match lab.frame {
            FrameSpec::BodyInertial { body } | FrameSpec::BodySurface { body } => body,
            _ => sim.navigation_reference(),
        };
        if sim.impact.is_none() {
            for apsis in find_apsides(&sim.prediction, eph, reference, now, 6) {
                markers.push(apsis_marker(
                    apsis,
                    eph,
                    &mut evaluator,
                    focus,
                    sim.system.bodies[reference].radius_meters,
                    false,
                ));
            }
            if let Some(impact) = sim.prediction_impact() {
                markers.push(event(
                    to_frame(
                        &evaluator.evaluate(eph, impact.time),
                        sim.prediction.position(sim.prediction.count() - 1),
                    ) - focus,
                    format!("Impact {}", sim.system.bodies[impact.body].name),
                    HISTORY,
                    1e49,
                ));
            }
            if sim.plan.count() > 0 {
                for (i, burn) in sim.plan.burns().iter().enumerate() {
                    if burn.start_time >= now
                        && trajectory.count() > 1
                        && burn.start_time <= trajectory.last_time()
                        && burn.start_time >= trajectory.first_time()
                    {
                        markers.push(event(
                            to_frame(
                                &evaluator.evaluate(eph, burn.start_time),
                                trajectory.sample(burn.start_time).0,
                            ) - focus,
                            format!("Burn {} / {:.1} m/s", i + 1, burn.delta_v),
                            BURN,
                            2e49,
                        ));
                    }
                }
                if let Some(last) = sim.plan.burns().last() {
                    let reference = match lab.frame {
                        FrameSpec::BodyInertial { body } | FrameSpec::BodySurface { body } => body,
                        _ => sim.plan.maneuver(sim.plan.burns().len() - 1).reference_body,
                    };
                    let from = now.max(last.end_time);
                    if trajectory.count() > 1 && trajectory.last_time() > from {
                        for apsis in find_apsides(trajectory, eph, reference, from, 4) {
                            markers.push(apsis_marker(
                                apsis,
                                eph,
                                &mut evaluator,
                                focus,
                                sim.system.bodies[reference].radius_meters,
                                true,
                            ));
                        }
                    }
                }
                if let Some(impact) = sim.plan.impact() {
                    markers.push(event(
                        to_frame(
                            &evaluator.evaluate(eph, impact.time),
                            trajectory.position(trajectory.count() - 1),
                        ) - focus,
                        format!("plan impact {}", sim.system.bodies[impact.body].name),
                        HISTORY,
                        1e49,
                    ));
                }
            }
        }
        markers.push(PlotMarker {
            position: vessel - focus,
            text: if sim.impact.is_some() {
                "Vessel / impact"
            } else {
                "Vessel"
            }
            .into(),
            color: VESSEL.into(),
            focus: Some(Focus::Vessel),
            priority: if lab.focus == Focus::Vessel {
                1e60
            } else {
                1e50
            },
            ring: false,
        });
        markers.sort_by(|a, b| b.priority.total_cmp(&a.priority));
        let thrust = if sim.impact.is_none() {
            Some(direction_to_frame(&here, sim.thrust_direction()))
        } else {
            None
        };
        PlotScene {
            bodies,
            paths,
            markers,
            vessel: vessel - focus,
            thrust,
            target_gap,
        }
    }
}
fn vessel_period(sim: &Simulation) -> f64 {
    let vessel = sim.vessel();
    let n = sim.system.bodies.len();
    let mut p = vec![DVec3::ZERO; n];
    let mut v = p.clone();
    sim.ephemeris.states_at(sim.time, &mut p, Some(&mut v));
    let index = sim.dominance.dominant(&p, vessel.position);
    let r = vessel.position - p[index];
    let v = vessel.velocity - v[index];
    let period = osculating_orbit(r, v, sim.system.bodies[index].gm).period_seconds;
    if period.is_finite() {
        period
    } else {
        let dt = r.length() / v.length();
        assert!(dt > 0.0 && dt.is_finite(), "undefined vessel path interval");
        dt
    }
}
#[allow(clippy::too_many_arguments)]
fn path(
    cache: &PathCache,
    origin: DVec3,
    head: Option<(f64, DVec3)>,
    tail: Option<(f64, DVec3)>,
    kind: PathKind,
    color: &str,
    start: f64,
    span: f64,
    burns: &[(f64, f64)],
) -> PlotPath {
    let mut points = Vec::new();
    points.extend(head);
    points.extend(cache.samples());
    points.extend(tail);
    for (_, p) in &mut points {
        *p -= origin;
    }
    if matches!(
        kind,
        PathKind::Prediction | PathKind::Plan | PathKind::Target
    ) {
        points.reverse();
    }
    PlotPath {
        kind,
        color: color.into(),
        points,
        fade_start: start,
        fade_span: span,
        burns: burns.into(),
    }
}
impl PlotPath {
    pub fn shade(&self, time: f64) -> (&str, f64) {
        if self.kind == PathKind::Body {
            return (&self.color, 0.55);
        }
        let fraction = if self.kind == PathKind::History {
            (self.fade_start - time) / self.fade_span
        } else {
            (time - self.fade_start) / self.fade_span
        };
        let shade = 0.12 + 0.88 * (1.0 - fraction.clamp(0.0, 1.0)).powf(1.5);
        let color = if self.burns.iter().any(|&(a, b)| time >= a && time <= b) {
            BURN
        } else {
            &self.color
        };
        (color, shade)
    }
}
fn event(position: DVec3, text: String, color: &str, priority: f64) -> PlotMarker {
    PlotMarker {
        position,
        text,
        color: color.into(),
        focus: None,
        priority,
        ring: false,
    }
}
fn apsis_marker(
    apsis: Apsis,
    eph: &Ephemeris,
    evaluator: &mut FrameEvaluator,
    origin: DVec3,
    radius: f64,
    plan: bool,
) -> PlotMarker {
    let short = if apsis.kind == ApsisKind::Periapsis {
        "Pe"
    } else {
        "Ap"
    };
    event(
        to_frame(&evaluator.evaluate(eph, apsis.time), apsis.position) - origin,
        format!(
            "{}{short} {}",
            if plan { "plan " } else { "" },
            distance(apsis.distance_meters - radius)
        ),
        if plan { PLAN } else { PREDICTION },
        1e49,
    )
}
pub fn distance(m: f64) -> String {
    if m.abs() >= 1e9 {
        format!("{:.3} Gm", m / 1e9)
    } else if m.abs() >= 1e6 {
        format!("{:.3} Mm", m / 1e6)
    } else if m.abs() >= 1e3 {
        format!("{:.2} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}
pub fn duration(s: f64) -> String {
    if !s.is_finite() {
        return "unbound".into();
    }
    if s.abs() >= 86400.0 {
        format!("{:.2} d", s / 86400.0)
    } else if s.abs() >= 3600.0 {
        format!("{:.2} h", s / 3600.0)
    } else {
        format!("{s:.1} s")
    }
}

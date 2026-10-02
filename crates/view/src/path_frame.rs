//! What the map draws relative to, as `lab/view/src/PathFrame.ts`, and the orbit lab's path sample
//! cache (`lab/orbit/src/app/PathCache.ts`).
//!
//! The map has one plotting frame, as Principia's, centred on one reference body:
//! - inertial: the body's non-rotating equatorial frame (the orbit lab's body-inertial);
//! - surface: turning with the body (body-surface), so a point on the ground stays still, a
//!   stationary orbit is a point and a suborbital hop is an arc over the ground.
//!
//! A path sample goes into the frame at its own time, so cached samples never change; the
//! drawing turns frame coordinates back into ecliptic axes with the frame's orientation now.

use glam::DVec3;
use void_orbit::{CelestialBody, Ephemeris, body_orientation, osculating_orbit};

use crate::conic::ellipse_points_in_time;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFrameKind {
    Inertial,
    Surface,
}

/// The map's one plotting frame: every path and orbit is drawn in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlottingFrame {
    pub kind: PathFrameKind,
    /// Index of the body the frame is centred on.
    pub reference: usize,
}

/// A surface-frame orbit gets this many samples per turn of the frame, within the bounds below.
const SAMPLES_PER_FRAME_TURN: f64 = 64.0;
const MIN_ORBIT_SAMPLES: usize = 256;
/// A planet's year in a fast-turning frame is hundreds of turns or more; past this many samples
/// each turn is drawn coarser (Mars in Aurelia's surface frame: about 24 per turn).
pub const MAX_ORBIT_SAMPLES: usize = 16_384;

/// Axes (x, y, z) as ecliptic vectors.
pub type Basis = [DVec3; 3];

/// The frame's axes in the ecliptic at time t.
pub fn frame_axes(kind: PathFrameKind, body: &CelestialBody, t: f64) -> Basis {
    match kind {
        PathFrameKind::Surface => body_orientation(&body.rotation, t),
        PathFrameKind::Inertial => body.rotation.equatorial_basis(),
    }
}

/// Frame coordinates to ecliptic axes (relative to the reference body, not the barycentre).
pub fn frame_to_ecliptic(axes: &Basis, v: DVec3) -> DVec3 {
    DVec3::new(
        axes[0].x * v.x + axes[1].x * v.y + axes[2].x * v.z,
        axes[0].y * v.x + axes[1].y * v.y + axes[2].y * v.z,
        axes[0].z * v.x + axes[1].z * v.y + axes[2].z * v.z,
    )
}

fn dot(a: DVec3, b: DVec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

#[derive(Clone, Debug)]
pub struct PathFrame {
    pub kind: PathFrameKind,
    pub reference: usize,
    body: CelestialBody,
}

impl PathFrame {
    pub fn new(ephemeris: &Ephemeris, kind: PathFrameKind, reference: usize) -> Self {
        Self {
            kind,
            reference,
            body: ephemeris.bodies()[reference].clone(),
        }
    }

    /// A barycentric point at time t, in frame coordinates. Needs the ephemeris at t.
    pub fn at(&self, ephemeris: &Ephemeris, t: f64, barycentric: DVec3) -> DVec3 {
        let origin = ephemeris.body_position(self.reference, t);
        let axes = self.axes_at(t);
        let d = barycentric - origin;
        DVec3::new(dot(d, axes[0]), dot(d, axes[1]), dot(d, axes[2]))
    }

    /// The frame's axes in the ecliptic at time t, without the ephemeris.
    pub fn axes_at(&self, t: f64) -> Basis {
        frame_axes(self.kind, &self.body, t)
    }
}

/// An orbiting body's path in `frame_body`'s surface frame over the same span the inertial map
/// draws: one period of its osculating ellipse about its parent from now. Each point is where the
/// body is at its own time, turned into the frame at that time, so a moon traces petals and a
/// stationary orbit is a point. The parent is held where it is now (`parent_offset` = parent −
/// frame centre, now). Returns frame coordinates relative to the frame's centre; the last point
/// closes the period at its own time, which in a turning frame is not the first point.
pub fn orbit_in_surface_frame(
    frame_body: &CelestialBody,
    now: f64,
    parent_offset: DVec3,
    relative_position: DVec3,
    relative_velocity: DVec3,
    gm: f64,
) -> Vec<DVec3> {
    let period = osculating_orbit(relative_position, relative_velocity, gm).period_seconds;
    let turn = frame_body.rotation.period_seconds;
    let count = ((SAMPLES_PER_FRAME_TURN * period / turn).ceil() as usize)
        .clamp(MIN_ORBIT_SAMPLES, MAX_ORBIT_SAMPLES);
    let (points, period_seconds) =
        ellipse_points_in_time(relative_position, relative_velocity, gm, count);
    // body_orientation at each time, written out: the body-fixed axes are the equatorial ones
    // turned about the pole, so a point's body-fixed coordinates are its equatorial ones turned
    // back.
    let [node, quadrature, pole] = frame_body.rotation.equatorial_basis();
    let spin = 2.0 * std::f64::consts::PI / frame_body.rotation.period_seconds;
    (0..=count)
        .map(|i| {
            let v = parent_offset + points[i % count];
            let along = v.x * node.x + v.y * node.y + v.z * node.z;
            let across = v.x * quadrature.x + v.y * quadrature.y + v.z * quadrature.z;
            let angle = frame_body.rotation.angle_at_epoch_radians
                + spin * (now + period_seconds * i as f64 / count as f64);
            let (c, s) = (angle.cos(), angle.sin());
            DVec3::new(
                c * along + s * across,
                -s * along + c * across,
                v.x * pole.x + v.y * pole.y + v.z * pole.z,
            )
        })
        .collect()
}

/// Samples of a path already transformed into the plotting frame. A frame's transform depends
/// only on the sample's own time, so past samples never change and only new ones are computed
/// each frame. The owner discards the cache when the frame or sampling interval changes.
#[derive(Clone, Debug)]
pub struct PathCache {
    pub interval_seconds: f64,
    times: std::collections::VecDeque<f64>,
    points: std::collections::VecDeque<DVec3>,
}

impl PathCache {
    pub fn new(interval_seconds: f64) -> Self {
        assert!(
            interval_seconds > 0.0 && interval_seconds.is_finite(),
            "path cache interval {interval_seconds}"
        );
        Self {
            interval_seconds,
            times: Default::default(),
            points: Default::default(),
        }
    }

    pub fn count(&self) -> usize {
        self.times.len()
    }

    /// Keep samples on the grid k × interval inside [from, to). Both ends only move forward for
    /// one cache; the owner starts a new cache otherwise.
    pub fn update(&mut self, from: f64, to: f64, mut sample: impl FnMut(f64) -> DVec3) {
        while self.times.front().is_some_and(|&t| t < from) {
            self.times.pop_front();
            self.points.pop_front();
        }
        let dt = self.interval_seconds;
        let mut next = match self.times.back() {
            Some(&last) => last + dt,
            None => (from / dt).ceil() * dt,
        };
        if next < from {
            next = (from / dt).ceil() * dt;
        }
        while next < to {
            self.points.push_back(sample(next));
            self.times.push_back(next);
            next += dt;
        }
    }

    /// The sample times and points, oldest first.
    pub fn samples(&self) -> impl Iterator<Item = (f64, DVec3)> + '_ {
        self.times.iter().copied().zip(self.points.iter().copied())
    }

    /// Head, the cached samples, then tail, relative to `origin` (all in frame coordinates).
    pub fn write_relative(
        &self,
        out: &mut Vec<DVec3>,
        origin: DVec3,
        head: Option<DVec3>,
        tail: Option<DVec3>,
    ) {
        out.clear();
        out.extend(head.map(|h| h - origin));
        out.extend(self.points.iter().map(|&p| p - origin));
        out.extend(tail.map(|t| t - origin));
    }
}

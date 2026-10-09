//! The map's state, as `lab/view/src/MapLayer.ts` without the drawing: bodies' osculating orbits
//! about their parents, the vessel's path with its apsides, and the labels with their priorities.
//! Orbits, paths and apsides are drawn in one plotting frame. The caller draws lines and labels at
//! the map weight's opacity.

use glam::DVec3;
use void_orbit::{
    ApsisKind, CelestialBody, EphemerisSource, Trajectory, find_apsides, osculating_orbit,
};

use crate::conic::ellipse_points;
use crate::path_frame::{
    Basis, PathCache, PathFrame, PathFrameKind, PlottingFrame, frame_axes, frame_to_ecliptic,
    orbit_in_surface_frame,
};

/// Points on an inertial orbit loop.
pub const ORBIT_POINTS: usize = 256;
/// Bodies' orbits are recomputed this often (wall time), the stalest one per frame: a planet's
/// year in a surface frame is thousands of points, too many to redo every orbit at once.
pub const ORBIT_REFRESH_MS: f64 = 500.0;
pub const APSIS_REFRESH_MS: f64 = 250.0;

/// One frame of map state, all barycentric at `time`.
#[derive(Clone, Copy, Debug)]
pub struct MapFrame<'a> {
    pub time: f64,
    pub positions: &'a [DVec3],
    pub velocities: &'a [DVec3],
    /// Render origin (the focus).
    pub origin: DVec3,
    pub vessel: DVec3,
    pub vessel_velocity: DVec3,
    pub plotting: PlottingFrame,
    /// Wall clock, ms, for the refresh cadences.
    pub wall_ms: f64,
}

/// A body's orbit as last shaped. Inertial: a closed loop in ecliptic axes relative to the
/// parent. Surface: an open line in frame coordinates relative to the plotting frame's centre.
#[derive(Clone, Debug, Default)]
pub struct OrbitShape {
    pub points: Vec<DVec3>,
    pub closed: bool,
    /// Increments on every reshape, for the caller to rebuild its mesh.
    pub version: u64,
}

/// Where an orbit line is drawn this frame: its points are turned by `axes` (frame coordinates
/// to ecliptic; None for inertial loops, already ecliptic) and moved by `anchor` (relative to the
/// origin).
#[derive(Clone, Copy, Debug)]
pub struct OrbitPlacement {
    pub anchor: DVec3,
    pub axes: Option<Basis>,
}

pub struct MapOrbits {
    plotting: Option<PlottingFrame>,
    shaped_at: Vec<f64>,
    /// The plotting frame's centre stands still there, so its orbit is not drawn (Principia's
    /// `FixesBody`).
    fixed: Vec<bool>,
    pub shapes: Vec<OrbitShape>,
}

impl MapOrbits {
    pub fn new(bodies: &[CelestialBody]) -> Self {
        Self {
            plotting: None,
            shaped_at: vec![f64::NEG_INFINITY; bodies.len()],
            fixed: vec![false; bodies.len()],
            shapes: vec![OrbitShape::default(); bodies.len()],
        }
    }

    /// Reshape what is due. A new plotting frame reshapes every orbit at once; otherwise the
    /// stalest one, if it is due. Inertial: each body's osculating ellipse about its parent.
    /// Surface: the same period of the same ellipse, each point turned into the frame at its own
    /// time.
    pub fn update(&mut self, bodies: &[CelestialBody], frame: &MapFrame) {
        let plotting = frame.plotting;
        let now = frame.wall_ms;
        let changed = self.plotting != Some(plotting);
        if changed {
            self.plotting = Some(plotting);
        }
        let mut stalest: Option<usize> = None;
        for body in bodies.iter().filter(|b| b.parent_index.is_some()) {
            if stalest.is_none_or(|s| self.shaped_at[body.index] < self.shaped_at[s]) {
                stalest = Some(body.index);
            }
        }
        if stalest.is_some_and(|s| now - self.shaped_at[s] < ORBIT_REFRESH_MS) {
            stalest = None;
        }
        let centre = &bodies[plotting.reference];
        let surface = plotting.kind == PathFrameKind::Surface;
        for body in bodies {
            let Some(parent) = body.parent_index else {
                continue;
            };
            let i = body.index;
            self.fixed[i] = surface && i == plotting.reference;
            if self.fixed[i] {
                self.shaped_at[i] = now;
                continue;
            }
            if !(changed || stalest == Some(i)) {
                continue;
            }
            self.shaped_at[i] = now;
            let relative_position = frame.positions[i] - frame.positions[parent];
            let relative_velocity = frame.velocities[i] - frame.velocities[parent];
            let gm = bodies[parent].gm + body.gm;
            let shape = &mut self.shapes[i];
            if surface {
                shape.points = orbit_in_surface_frame(
                    centre,
                    frame.time,
                    frame.positions[parent] - frame.positions[plotting.reference],
                    relative_position,
                    relative_velocity,
                    gm,
                );
                shape.closed = false;
            } else {
                shape.points =
                    ellipse_points(relative_position, relative_velocity, gm, ORBIT_POINTS);
                shape.closed = true;
            }
            shape.version += 1;
        }
    }

    /// Where body i's orbit is drawn now; None for the root and the plotting frame's centre.
    pub fn placement(
        &self,
        bodies: &[CelestialBody],
        i: usize,
        frame: &MapFrame,
    ) -> Option<OrbitPlacement> {
        let parent = bodies[i].parent_index?;
        if self.fixed[i] || self.shapes[i].points.is_empty() {
            return None;
        }
        let plotting = self.plotting?;
        Some(match plotting.kind {
            PathFrameKind::Surface => OrbitPlacement {
                anchor: frame.positions[plotting.reference] - frame.origin,
                axes: Some(frame_axes(
                    PathFrameKind::Surface,
                    &bodies[plotting.reference],
                    frame.time,
                )),
            },
            // Points are relative to the parent; f64 subtraction keeps the offset exact.
            PathFrameKind::Inertial => OrbitPlacement {
                anchor: frame.positions[parent] - frame.origin,
                axes: None,
            },
        })
    }
}

/// An apsis label, kept in the path frame so it turns with the frame between refreshes.
#[derive(Clone, Debug)]
pub struct ApsisLabel {
    pub label: String,
    pub in_frame: DVec3,
}

/// A trajectory drawn in the plotting frame: the vessel's coast prediction or its plan.
pub struct MapPath {
    cache: Option<PathCache>,
    frame: Option<PathFrame>,
    generation: u64,
    last_apsides_ms: f64,
    pub apsides: Vec<ApsisLabel>,
    /// The vessel now, then the cached samples, in ecliptic axes relative to the origin.
    pub points: Vec<DVec3>,
    pub visible: bool,
    scratch: Vec<DVec3>,
}

impl Default for MapPath {
    fn default() -> Self {
        Self::new()
    }
}

impl MapPath {
    pub fn new() -> Self {
        Self {
            cache: None,
            frame: None,
            generation: u64::MAX,
            last_apsides_ms: f64::NEG_INFINITY,
            apsides: Vec::new(),
            points: Vec::new(),
            visible: false,
            scratch: Vec::new(),
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.apsides.clear();
    }

    /// Follow `trajectory` (restarted whenever `generation` changes). With `apsides`, the next two
    /// apsides about the plotting frame's body are found again every `APSIS_REFRESH_MS`.
    pub fn update(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        trajectory: &Trajectory,
        generation: u64,
        frame: &MapFrame,
        apsides: bool,
    ) {
        if trajectory.count() < 2 || trajectory.last_time() <= frame.time {
            self.hide();
            return;
        }
        let reference = frame.plotting.reference;
        let stale = match &self.frame {
            Some(f) => f.reference != reference || f.kind != frame.plotting.kind,
            None => true,
        };
        if self.cache.is_none() || stale || self.generation != generation {
            self.generation = generation;
            self.frame = Some(PathFrame::new(ephemeris, frame.plotting.kind, reference));
            self.cache = Some(new_cache(ephemeris.bodies(), trajectory, reference, frame));
            self.last_apsides_ms = f64::NEG_INFINITY;
        }
        let path_frame = self.frame.as_ref().unwrap();
        let cache = self.cache.as_mut().unwrap();
        cache.update(
            frame.time.max(trajectory.first_time()),
            trajectory.last_time(),
            |t| path_frame.at(ephemeris, t, trajectory.sample(t).0),
        );
        // Subtracting in frame coordinates (f64) keeps the precision; the frame's axes now then
        // turn the line into the scene.
        cache.write_relative(
            &mut self.scratch,
            path_frame.at(ephemeris, frame.time, frame.origin),
            Some(path_frame.at(ephemeris, frame.time, frame.vessel)),
            None,
        );
        let axes = path_frame.axes_at(frame.time);
        self.points.clear();
        self.points
            .extend(self.scratch.iter().map(|&p| frame_to_ecliptic(&axes, p)));
        self.visible = true;
        if apsides && frame.wall_ms - self.last_apsides_ms >= APSIS_REFRESH_MS {
            self.last_apsides_ms = frame.wall_ms;
            let body = &ephemeris.bodies()[reference];
            self.apsides = find_apsides(trajectory, ephemeris, reference, frame.time, 2)
                .into_iter()
                .map(|apsis| ApsisLabel {
                    label: format!(
                        "{} {:.1} km",
                        if apsis.kind == ApsisKind::Periapsis {
                            "Pe"
                        } else {
                            "Ap"
                        },
                        (apsis.distance_meters - body.radius_meters) / 1000.0
                    ),
                    in_frame: path_frame.at(ephemeris, apsis.time, apsis.position),
                })
                .collect();
        }
    }

    /// Apsis label positions now, relative to the origin.
    pub fn apsis_positions(&self, frame: &MapFrame) -> Vec<(String, DVec3)> {
        let (Some(path_frame), true) = (&self.frame, self.visible) else {
            return Vec::new();
        };
        let axes = path_frame.axes_at(frame.time);
        let centre = frame.positions[path_frame.reference] - frame.origin;
        self.apsides
            .iter()
            .map(|a| {
                (
                    a.label.clone(),
                    centre + frame_to_ecliptic(&axes, a.in_frame),
                )
            })
            .collect()
    }
}

/// About 256 samples per orbit of the current osculating orbit, and at most 6000 over the path.
fn new_cache(
    bodies: &[CelestialBody],
    trajectory: &Trajectory,
    reference: usize,
    frame: &MapFrame,
) -> PathCache {
    let osc = osculating_orbit(
        frame.vessel - frame.positions[reference],
        frame.vessel_velocity - frame.velocities[reference],
        bodies[reference].gm,
    );
    let span = trajectory.last_time() - trajectory.first_time();
    let natural = if osc.period_seconds.is_finite() {
        osc.period_seconds
    } else {
        span
    };
    PathCache::new((natural / 256.0).max(span / 6000.0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Star,
    Body(usize),
    Vessel,
    Apsis,
    Node(usize),
}

/// A label to place: position relative to the origin, and its priority (higher wins a crowded
/// spot): the focus first, then the vessel, the apsides, then bodies by mass.
#[derive(Clone, Debug)]
pub struct MapLabel {
    pub kind: LabelKind,
    pub text: String,
    pub color: String,
    pub relative: DVec3,
    pub priority: f64,
}

/// Every label this frame, highest priority first. `focus` is None for the vessel.
pub fn map_labels(
    bodies: &[CelestialBody],
    frame: &MapFrame,
    focus: Option<usize>,
    apsides: &[(String, DVec3)],
) -> Vec<MapLabel> {
    let mut labels: Vec<MapLabel> = bodies
        .iter()
        .map(|b| MapLabel {
            kind: if b.parent_index.is_none() && b.index == 0 {
                LabelKind::Star
            } else {
                LabelKind::Body(b.index)
            },
            text: b.name.clone(),
            color: b.color.clone(),
            relative: frame.positions[b.index] - frame.origin,
            priority: if focus == Some(b.index) { 1e60 } else { 0.0 } + b.mass_kg,
        })
        .collect();
    labels.push(MapLabel {
        kind: LabelKind::Vessel,
        text: "Vessel".into(),
        color: "#7dffb0".into(),
        relative: frame.vessel - frame.origin,
        priority: if focus.is_none() { 1e60 } else { 1e50 },
    });
    for (i, (text, relative)) in apsides.iter().enumerate() {
        labels.push(MapLabel {
            kind: LabelKind::Apsis,
            text: text.clone(),
            color: "#4fc8ff".into(),
            relative: *relative,
            priority: 1e49 - i as f64,
        });
    }
    labels.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    labels
}

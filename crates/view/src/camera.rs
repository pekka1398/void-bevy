//! The camera and the per-frame view decisions.
//!
//! Single view: zooming out from the vessel fades the map (orbits, labels) in, then turns the
//! camera's up from the local vertical to the body's north and lets go of the ground's spin.
//! Split view (KSP's two views): M switches between flight and map, each with its own zoom range.

use glam::{DQuat, DVec3};

use crate::path_frame::PathFrameKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Single,
    Split,
}

/// Map elements fade in between these zoom scales, in radii of the reference body (log-distance
/// smoothstep): 4–40 km on Aurelia, starting where the rocket shrinks to about a pixel.
pub const MAP_FADE_RADII: [f64; 2] = [0.00063, 0.0063];
/// Then, once the map is fully in, the camera's up turns from the local vertical to the body's
/// north between these zoom scales, and the camera stops following the ground's spin. One
/// decade: 400–4,000 km on Aurelia.
pub const UP_TURN_RADII: [f64; 2] = [0.063, 0.63];
/// The camera co-rotates with the surface below the first focus altitude and is inertial above
/// the second, in radii of the reference body.
pub const SURFACE_LOCK_RADII: [f64; 2] = [0.004, 0.012];
/// Split mode: KSP `FlightCamera.maxDistance`, metres.
pub const FLIGHT_MAX_DISTANCE: f64 = 150_000.0;
/// Split mode: KSP `PlanetariumCamera.minDistance` (3) times `ScaledSpace.scaleFactor` (6000), m.
pub const MAP_MIN_DISTANCE: f64 = 18_000.0;
/// The view direction keeps at least this angle from straight up or down, radians.
pub const MIN_ANGLE_FROM_UP: f64 = 0.02;
pub const RADIANS_PER_PIXEL: f64 = 0.005;
/// Nearest camera distance from the vessel, metres (the vessel is about 6 m long).
pub const VESSEL_MIN_DISTANCE: f64 = 8.0;
/// Nearest camera distance from a focused body's centre, in its radii.
pub const BODY_MIN_RADII: f64 = 1.02;
/// Farthest camera distance, metres: beyond the outermost planet.
pub const MAX_DISTANCE: f64 = 2e13;

fn normalize(v: DVec3) -> DVec3 {
    let l = v.length();
    assert!(
        l > 0.0 && l.is_finite(),
        "view: vector {v} has no direction"
    );
    DVec3::new(v.x / l, v.y / l, v.z / l)
}

pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    assert!(edge1 > edge0, "smoothstep: edges {edge0}, {edge1}");
    assert!(x.is_finite(), "smoothstep: x={x}");
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn zoom_weight(radii: [f64; 2], zoom_scale: f64, reference_radius: f64) -> f64 {
    assert!(
        reference_radius > 0.0,
        "zoom weight: radius {reference_radius}"
    );
    if zoom_scale <= 0.0 {
        return 0.0;
    }
    smoothstep(
        f64::ln(radii[0] * reference_radius),
        f64::ln(radii[1] * reference_radius),
        f64::ln(zoom_scale),
    )
}

/// How much of the map is shown in single mode, 0..1. `zoom_scale` is the camera's distance from
/// what it looks at: from the vessel, or from the surface of a focused body.
pub fn map_weight_for(zoom_scale: f64, reference_radius: f64) -> f64 {
    zoom_weight(MAP_FADE_RADII, zoom_scale, reference_radius)
}

/// How far the camera's up has turned from the local vertical to north in single mode, 0..1.
pub fn up_weight_for(zoom_scale: f64, reference_radius: f64) -> f64 {
    zoom_weight(UP_TURN_RADII, zoom_scale, reference_radius)
}

/// Fraction of the reference body's spin the camera direction follows, 0..1; it ends as the up
/// turns to north.
pub fn corotation_weight(up_weight: f64, focus_altitude: f64, reference_radius: f64) -> f64 {
    assert!(
        (0.0..=1.0).contains(&up_weight),
        "corotation weight: up weight {up_weight}"
    );
    let high = smoothstep(
        SURFACE_LOCK_RADII[0] * reference_radius,
        SURFACE_LOCK_RADII[1] * reference_radius,
        focus_altitude,
    );
    (1.0 - up_weight) * (1.0 - high)
}

/// A unit vector perpendicular to unit a.
pub fn perpendicular(a: DVec3) -> DVec3 {
    normalize(a.cross(if a.x.abs() < 0.9 { DVec3::X } else { DVec3::Y }))
}

/// Turn unit a toward unit b by fraction s of the angle between them. Opposite vectors have no
/// unique great circle; they turn about `perpendicular(a)`.
pub fn slerp_unit(a: DVec3, b: DVec3, s: f64) -> DVec3 {
    assert!((0.0..=1.0).contains(&s), "slerp unit: s={s}");
    let angle = f64::acos(a.dot(b).clamp(-1.0, 1.0));
    if angle == 0.0 {
        return a;
    }
    let axis = a.cross(b);
    DQuat::from_axis_angle(
        if axis.length() > 1e-12 {
            normalize(axis)
        } else {
            perpendicular(a)
        },
        angle * s,
    ) * a
}

/// Camera orbiting its focus. The direction (focus to camera, unit) is kept in the inertial
/// ecliptic frame; co-rotation with a surface is applied by turning it with the body. Dragging
/// turns it about the current up (azimuth) and toward or away from it (elevation), so the same
/// state serves the flight view (up = local vertical) and the map (up = the body's north).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub direction: DVec3,
    pub distance: f64,
}

impl OrbitCamera {
    pub fn new(direction: DVec3, distance: f64) -> Self {
        assert!(
            (direction.length() - 1.0).abs() <= 1e-9,
            "orbit camera: direction not unit {direction}"
        );
        assert!(distance > 0.0, "orbit camera: distance {distance}");
        Self {
            direction,
            distance,
        }
    }

    /// Pointer drag in pixels; dragging down raises the camera.
    pub fn drag(&mut self, dx_pixels: f64, dy_pixels: f64, up: DVec3) {
        self.clamp_to_up(up);
        self.direction =
            normalize(DQuat::from_axis_angle(up, -dx_pixels * RADIANS_PER_PIXEL) * self.direction);
        // Elevation is set as an angle from up, clamped before turning, so a long drag stops at
        // the pole instead of wrapping over it.
        let from_up = f64::acos(up.dot(self.direction).clamp(-1.0, 1.0));
        let target = (from_up - dy_pixels * RADIANS_PER_PIXEL)
            .clamp(MIN_ANGLE_FROM_UP, std::f64::consts::PI - MIN_ANGLE_FROM_UP);
        // Rotating up about (up × direction) turns it toward the direction.
        self.direction =
            normalize(DQuat::from_axis_angle(normalize(up.cross(self.direction)), target) * up);
    }

    pub fn zoom(&mut self, factor: f64, min_distance: f64, max_distance: f64) {
        assert!(factor > 0.0, "orbit camera zoom: factor {factor}");
        self.distance = self.clamp_distance(self.distance * factor, min_distance, max_distance);
    }

    pub fn clamp_distance(&self, value: f64, min_distance: f64, max_distance: f64) -> f64 {
        assert!(
            min_distance > 0.0 && max_distance >= min_distance,
            "orbit camera: distance range {min_distance}..{max_distance}"
        );
        value.clamp(min_distance, max_distance)
    }

    /// Turn with a spinning body: angle about its unit spin axis.
    pub fn corotate(&mut self, axis: DVec3, angle: f64) {
        self.direction = normalize(DQuat::from_axis_angle(axis, angle) * self.direction);
    }

    /// Keep the direction at least `MIN_ANGLE_FROM_UP` away from up and down, keeping its
    /// azimuth.
    pub fn clamp_to_up(&mut self, up: DVec3) {
        let angle = f64::acos(up.dot(self.direction).clamp(-1.0, 1.0));
        let clamped = angle.clamp(MIN_ANGLE_FROM_UP, std::f64::consts::PI - MIN_ANGLE_FROM_UP);
        if clamped == angle {
            return;
        }
        let side = up.cross(self.direction);
        // Rotating up about (up × direction) turns it toward the direction.
        let axis = if side.length() > 1e-12 {
            normalize(side)
        } else {
            perpendicular(up)
        };
        self.direction = normalize(DQuat::from_axis_angle(axis, clamped) * up);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusKind {
    Vessel,
    Body,
}

/// What the camera looks at, in the inertial ecliptic frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusGeometry {
    pub kind: FocusKind,
    /// Unit vector from the reference body's centre to the vessel; None for a body focus.
    pub radial: Option<DVec3>,
    /// The reference body's spin axis (unit).
    pub north: DVec3,
    pub reference_radius: f64,
    /// Vessel altitude above the reference body's radius; 0 for a body focus, which counts as on
    /// its surface.
    pub altitude: f64,
    /// The focused body's radius; 0 for the vessel.
    pub focus_radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    /// 0 = flight view, 1 = map: orbit lines and labels are drawn at this opacity.
    pub map_weight: f64,
    /// 0 = up is the local vertical, 1 = the body's north. Single mode: rises only after the map
    /// is fully in.
    pub up_weight: f64,
    /// Fraction of the reference body's spin the camera follows.
    pub corotation: f64,
    /// Camera up (unit): local vertical in flight, the body's north on the map.
    pub up: DVec3,
    pub min_distance: f64,
    pub max_distance: f64,
}

/// Whose spin the camera follows, and how much. With a surface path frame the map camera is
/// referenced to that frame, as Principia's: it turns with the paths' reference body as the map
/// comes in, so that body and the paths drawn on it stand still while the stars and the Sun's
/// light turn. Otherwise it is the ground lock (`corotation`) about the focus's reference body.
/// Returns (body, weight).
pub fn camera_spin(
    state: &ViewState,
    path_frame: PathFrameKind,
    focus_reference: usize,
    path_reference: usize,
) -> (usize, f64) {
    if path_frame == PathFrameKind::Inertial {
        return (focus_reference, state.corotation);
    }
    // The ground lock about another body (a focused moon close up) gives way to the path frame.
    let weight = if focus_reference == path_reference {
        state.corotation.max(state.map_weight)
    } else {
        state.map_weight
    };
    (path_reference, weight)
}

/// The per-frame view decisions for a camera at `distance` from the focus. Single: everything
/// follows the zoom. Split: flight (`map_on` false) or map (`map_on` true).
pub fn view_state(mode: ViewMode, map_on: bool, focus: &FocusGeometry, distance: f64) -> ViewState {
    let consistent = match focus.kind {
        FocusKind::Vessel => focus.radial.is_some() && focus.focus_radius == 0.0,
        FocusKind::Body => focus.radial.is_none() && focus.focus_radius > 0.0,
    };
    assert!(consistent, "view state: inconsistent focus {focus:?}");
    let nearest = match focus.kind {
        FocusKind::Vessel => VESSEL_MIN_DISTANCE,
        FocusKind::Body => focus.focus_radius * BODY_MIN_RADII,
    };
    let (min_distance, max_distance, map_weight, up_weight);
    match (mode, map_on) {
        (ViewMode::Single, true) => panic!("view state: single mode has no map switch"),
        (ViewMode::Single, false) => {
            min_distance = nearest;
            max_distance = MAX_DISTANCE;
            let clamped = distance.clamp(min_distance, max_distance);
            map_weight = map_weight_for(clamped - focus.focus_radius, focus.reference_radius);
            up_weight = up_weight_for(clamped - focus.focus_radius, focus.reference_radius);
        }
        (ViewMode::Split, true) => {
            min_distance = nearest.max(focus.focus_radius + MAP_MIN_DISTANCE);
            max_distance = MAX_DISTANCE;
            map_weight = 1.0;
            up_weight = 1.0;
        }
        (ViewMode::Split, false) => {
            assert!(
                focus.kind == FocusKind::Vessel,
                "view state: the split flight view looks only at the vessel"
            );
            min_distance = nearest;
            max_distance = FLIGHT_MAX_DISTANCE;
            map_weight = 0.0;
            up_weight = 0.0;
        }
    }
    let up = match focus.radial {
        Some(radial) => slerp_unit(radial, focus.north, up_weight),
        None => focus.north,
    };
    ViewState {
        map_weight,
        up_weight,
        corotation: corotation_weight(up_weight, focus.altitude, focus.reference_radius),
        up,
        min_distance,
        max_distance,
    }
}

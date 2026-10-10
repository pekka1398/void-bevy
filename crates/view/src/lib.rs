//! One view from the vessel out to the whole system. The camera's
//! per-frame decisions (map fade, up turn, co-rotation; single or split view), bodies' orbits as
//! osculating ellipses, the plotting frame (inertial or turning with a body) and the map's lines
//! and labels. Drawing is the caller's.

mod camera;
mod conic;
mod map;
mod path_frame;

pub use camera::{
    BODY_MIN_RADII, FLIGHT_MAX_DISTANCE, FocusGeometry, FocusKind, MAP_FADE_RADII,
    MAP_MIN_DISTANCE, MAX_DISTANCE, MIN_ANGLE_FROM_UP, OrbitCamera, SURFACE_LOCK_RADII,
    UP_TURN_RADII, VESSEL_MIN_DISTANCE, ViewMode, ViewState, camera_spin, corotation_weight,
    map_weight_for, perpendicular, rotate, slerp_unit, smoothstep, up_weight_for, view_state,
};
pub use conic::{ellipse_points, ellipse_points_in_time};
pub use map::{
    APSIS_REFRESH_MS, ApsisLabel, LabelKind, MapFrame, MapLabel, MapOrbits, MapPath, ORBIT_POINTS,
    ORBIT_REFRESH_MS, OrbitPlacement, OrbitShape, map_labels,
};
pub use path_frame::{
    Basis, MAX_ORBIT_SAMPLES, PathCache, PathFrame, PathFrameKind, PlottingFrame, frame_axes,
    frame_to_ecliptic, orbit_in_surface_frame,
};

pub mod plot;

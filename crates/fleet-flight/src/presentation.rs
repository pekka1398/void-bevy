//! Camera and observation controls share the command journal with physics. Rendering only reads
//! these decisions: camera spin must never depend on how often a renderer happens to run.
use crate::FleetFlight;
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};
use void_frames::{FrameId, Motion, Transform};
use void_vessels::Fleet;
use void_view::{FocusGeometry, FocusKind, OrbitCamera, PathFrameKind, ViewMode, ViewState};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub main_camera: bool,
    pub direction: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub focus_body: Option<usize>,
    pub surface_path: bool,
    pub plotting_frame: void_orbit::FrameSpec,
    pub speed_surface: bool,
    pub altitude_agl: bool,
    pub colliders: bool,
    pub bounds: bool,
    pub wire: bool,
    pub terrain: bool,
    pub last_time: f64,
    pub paused: bool,
    pub rate: usize,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Toggle {
    Colliders,
    Bounds,
    Wire,
    Terrain,
    SpeedSurface,
    AltitudeAgl,
    PathFrame,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ViewCommand {
    Configure { main_camera: bool },
    PlotFrame { frame: void_orbit::FrameSpec },
    Focus { body: Option<usize> },
    Drag { x: f64, y: f64 },
    Zoom { pixels: f64 },
    Toggle { setting: Toggle },
}
pub struct CameraSample {
    /// Inertial (origin frame) eye and focus, for readouts; drawing goes through `to_camera`.
    pub eye: DVec3,
    pub focus: DVec3,
    pub view: ViewState,
    /// The frame the camera hangs on (the selected vessel's parts frame, or a focused body's
    /// inertial frame) and the focus point in it.
    pub focus_frame: FrameId,
    pub focus_local: DVec3,
    /// Eye minus focus, in origin-frame axes.
    pub offset: DVec3,
}
impl CameraSample {
    /// The camera frame relative to its focus frame: origin at the eye, with axes `axes` given in
    /// origin-frame coordinates.
    pub fn camera(&self, fleet: &Fleet, axes: DQuat) -> Motion {
        let turn = fleet
            .frames()
            .transform(self.focus_frame, fleet.origin_frame())
            .rotation()
            .inverse();
        Motion::fixed(
            self.focus_local + turn * self.offset,
            (turn * axes).normalize(),
        )
    }
    /// From any frame of the fleet's tree into the camera frame, through the frames' common
    /// ancestor: a part 40 m from the eye keeps its digits however far the system is.
    pub fn to_camera(&self, fleet: &Fleet, from: FrameId, axes: DQuat) -> Transform {
        fleet
            .frames()
            .transform(from, self.focus_frame)
            .into_child(&self.camera(fleet, axes))
    }
}
impl Presentation {
    pub fn new(position: DVec3, centre: DVec3, time: f64) -> Self {
        let radial = (position - centre).normalize();
        let side = if radial.x.hypot(radial.y) > 1e-9 {
            DVec3::new(-radial.y, radial.x, 0.0).normalize()
        } else {
            DVec3::X
        };
        Self {
            main_camera: true,
            direction: (side + 0.3 * radial).normalize(),
            distance: 40.0,
            yaw: 0.4,
            pitch: 0.25,
            focus_body: None,
            surface_path: false,
            plotting_frame: void_orbit::FrameSpec::Barycentric,
            speed_surface: true,
            altitude_agl: true,
            colliders: false,
            bounds: false,
            wire: false,
            terrain: true,
            last_time: time,
            paused: true,
            rate: 0,
        }
    }
    pub fn validate(&self, sim: &FleetFlight) {
        self.plotting_frame
            .assert_valid(sim.fleet.ephemeris.bodies().len());
        if let void_orbit::FrameSpec::TwoBodyRotating { primary, secondary } = self.plotting_frame {
            use void_orbit::EphemerisSource;
            assert_eq!(
                sim.fleet.ephemeris.system_of(primary),
                sim.fleet.ephemeris.system_of(secondary),
                "plotting pair spans systems"
            );
        }
        assert!(
            self.direction.is_finite() && (self.direction.length() - 1.0).abs() < 1e-9,
            "view: invalid direction"
        );
        assert!(
            self.distance.is_finite()
                && self.distance > 0.0
                && self.yaw.is_finite()
                && self.pitch.is_finite()
                && self.last_time.is_finite()
                && self.last_time <= sim.fleet.time()
                && self.rate < 9,
            "view: invalid state"
        );
        if let Some(body) = self.focus_body {
            assert!(
                body < sim.fleet.ephemeris.bodies().len(),
                "view: unknown focus body"
            );
        }
    }
    pub fn path_frame(&self) -> PathFrameKind {
        if self.surface_path {
            PathFrameKind::Surface
        } else {
            PathFrameKind::Inertial
        }
    }
    fn geometry(&self, sim: &FleetFlight) -> (FocusGeometry, usize, usize, DVec3) {
        let f = &sim.fleet;
        let ship = f.snapshot(&sim.selected);
        let mut positions = vec![DVec3::ZERO; f.ephemeris.bodies().len()];
        f.ephemeris.positions_at(f.time(), &mut positions);
        let bodies = f.ephemeris.bodies();
        let navigation = void_orbit::DominanceTree::new(bodies).dominant(&positions, ship.position);
        let reference = self.focus_body.unwrap_or(navigation);
        let body = bodies.get(reference).expect("view: unknown focus body");
        let anchor = f
            .frames()
            .transform(f.vessel_frame(&sim.selected), f.origin_frame())
            .apply_point(f.root_position_local(&sim.selected));
        let radial = anchor - positions[reference];
        (
            FocusGeometry {
                kind: if self.focus_body.is_some() {
                    FocusKind::Body
                } else {
                    FocusKind::Vessel
                },
                radial: self.focus_body.is_none().then(|| radial.normalize()),
                north: body.rotation.axis(),
                reference_radius: body.radius_meters,
                altitude: if self.focus_body.is_none() {
                    radial.length() - body.radius_meters
                } else {
                    0.0
                },
                focus_radius: if self.focus_body.is_some() {
                    body.radius_meters
                } else {
                    0.0
                },
            },
            reference,
            navigation,
            self.focus_body.map_or(anchor, |i| positions[i]),
        )
    }
    fn camera(&self) -> OrbitCamera {
        OrbitCamera::new(self.direction, self.distance)
    }
    pub fn apply(&mut self, sim: &FleetFlight, command: &ViewCommand) {
        match *command {
            ViewCommand::Configure { main_camera } => self.main_camera = main_camera,
            ViewCommand::PlotFrame { frame } => {
                frame.assert_valid(sim.fleet.ephemeris.bodies().len());
                let mut evaluator = void_orbit::FrameEvaluator::new(&sim.fleet.ephemeris, frame);
                evaluator.evaluate(&sim.fleet.ephemeris, sim.fleet.time());
                self.plotting_frame = frame;
            }
            ViewCommand::Focus { body } => {
                self.focus_body = body;
                if body.is_none() {
                    // Re-enter ship view relative to this ship's local ground, rather than
                    // keeping the previous planet's inertial direction below its horizon.
                    let fleet = &sim.fleet;
                    let surface = fleet.body_frames(sim.nearby_body(&sim.selected)).1;
                    let frames = fleet.frames();
                    let radial = frames
                        .transform(fleet.vessel_frame(&sim.selected), surface)
                        .apply_point(fleet.root_position_local(&sim.selected))
                        .normalize();
                    let east = if radial.x.hypot(radial.y) > 1e-9 {
                        DVec3::new(-radial.y, radial.x, 0.0).normalize()
                    } else {
                        DVec3::X
                    };
                    self.direction = frames
                        .transform(surface, fleet.origin_frame())
                        .apply_direction((east + 0.3 * radial).normalize());
                }
                self.distance = body.map_or(40.0, |i| {
                    sim.fleet
                        .ephemeris
                        .bodies()
                        .get(i)
                        .expect("view: unknown focus body")
                        .radius_meters
                        * 4.0
                });
            }
            ViewCommand::Drag { x, y } => {
                assert!(x.is_finite() && y.is_finite(), "view: invalid drag");
                if self.main_camera {
                    let (geometry, _, _, _) = self.geometry(sim);
                    let state =
                        void_view::view_state(ViewMode::Single, false, &geometry, self.distance);
                    let mut camera = self.camera();
                    camera.drag(x, y, state.up);
                    self.direction = camera.direction;
                } else {
                    self.yaw -= x * 0.006;
                    self.pitch = (self.pitch + y * 0.006).clamp(-1.5, 1.5);
                }
            }
            ViewCommand::Zoom { pixels } => {
                assert!(pixels.is_finite(), "view: invalid zoom");
                // Clamp the resulting distance, including extremely long but finite wheel input.
                let distance =
                    self.distance * (-pixels * if self.main_camera { 0.002 } else { 0.003 }).exp();
                let (min, max) = if self.main_camera {
                    let (geometry, _, _, _) = self.geometry(sim);
                    let state =
                        void_view::view_state(ViewMode::Single, false, &geometry, self.distance);
                    (state.min_distance, state.max_distance)
                } else {
                    (2.0, 2e8)
                };
                self.distance = distance.clamp(min, max);
            }
            ViewCommand::Toggle { setting } => {
                let flag = match setting {
                    Toggle::Colliders => &mut self.colliders,
                    Toggle::Bounds => &mut self.bounds,
                    Toggle::Wire => &mut self.wire,
                    Toggle::Terrain => &mut self.terrain,
                    Toggle::SpeedSurface => &mut self.speed_surface,
                    Toggle::AltitudeAgl => &mut self.altitude_agl,
                    Toggle::PathFrame => &mut self.surface_path,
                };
                *flag = !*flag;
            }
        }
        self.update(sim);
    }
    pub fn update(&mut self, sim: &FleetFlight) {
        assert!(
            self.last_time.is_finite() && self.yaw.is_finite() && self.pitch.is_finite(),
            "view: invalid state"
        );
        assert!(self.rate < 9, "view: invalid rate");
        let (geometry, reference, navigation, _) = self.geometry(sim);
        let view = void_view::view_state(ViewMode::Single, false, &geometry, self.distance);
        let mut camera = self.camera();
        if self.main_camera {
            camera.distance =
                camera.clamp_distance(camera.distance, view.min_distance, view.max_distance);
            let (body, weight) =
                void_view::camera_spin(&view, self.path_frame(), reference, navigation);
            let elapsed = sim.fleet.time() - self.last_time;
            assert!(elapsed >= 0.0, "view: world clock moved backwards");
            if elapsed > 0.0 {
                let rotation = &sim.fleet.ephemeris.bodies()[body].rotation;
                camera.corotate(
                    rotation.axis(),
                    rotation.rate() * elapsed * weight * (1.0 - view.map_weight),
                );
                let mut evaluator =
                    void_orbit::FrameEvaluator::new(&sim.fleet.ephemeris, self.plotting_frame);
                let before = evaluator.evaluate(&sim.fleet.ephemeris, self.last_time);
                let after = evaluator.evaluate(&sim.fleet.ephemeris, sim.fleet.time());
                let q = |axes: [DVec3; 3]| {
                    DQuat::from_mat3(&glam::DMat3::from_cols(axes[0], axes[1], axes[2]))
                };
                let delta = q(after.axes) * q(before.axes).inverse();
                camera.direction = (DQuat::IDENTITY.slerp(delta.normalize(), view.map_weight)
                    * camera.direction)
                    .normalize();
            }
            camera.clamp_to_up(view.up);
            self.direction = camera.direction;
            self.distance = camera.distance;
        }
        self.last_time = sim.fleet.time();
    }
    pub fn sample(&self, sim: &FleetFlight) -> CameraSample {
        let f = &sim.fleet;
        let (geometry, _, _, focus) = self.geometry(sim);
        let view = void_view::view_state(ViewMode::Single, false, &geometry, self.distance);
        let (focus_frame, focus_local) = match self.focus_body {
            Some(body) => (f.body_frames(body).0, DVec3::ZERO),
            None => (
                f.vessel_frame(&sim.selected),
                f.root_position_local(&sim.selected),
            ),
        };
        let direction = if self.main_camera {
            self.direction
        } else {
            let surface = f.body_frames(sim.nearby_body(&sim.selected)).1;
            let frames = f.frames();
            let up = frames
                .transform(f.vessel_frame(&sim.selected), surface)
                .apply_point(f.root_position_local(&sim.selected))
                .normalize();
            let east = if up.x.hypot(up.y) > 1e-9 {
                DVec3::new(-up.y, up.x, 0.0).normalize()
            } else {
                DVec3::X
            };
            let north = up.cross(east);
            frames.transform(surface, f.origin_frame()).apply_direction(
                east * (self.yaw.cos() * self.pitch.cos())
                    + north * (self.yaw.sin() * self.pitch.cos())
                    + up * self.pitch.sin(),
            )
        };
        let offset = direction * self.distance;
        CameraSample {
            eye: focus + offset,
            focus,
            view,
            focus_frame,
            focus_local,
            offset,
        }
    }
}
impl FleetFlight {
    pub fn view_command(&mut self, command: &ViewCommand) {
        let mut view = self.presentation.clone();
        view.apply(self, command);
        self.presentation = view;
    }
    pub fn update_presentation(&mut self) {
        let mut view = self.presentation.clone();
        view.update(self);
        self.presentation = view;
    }
}

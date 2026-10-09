//! Shared-world view in the nonrotating axes of one system's moving barycentre.
use crate::CoupledWorld;
use glam::DVec3;
use std::{cell::RefCell, rc::Rc};
use void_frames::{BodyId, BodyStates, FrameId, FrameSource, SplitPosition, SystemId};
use void_orbit::{CelestialBody, EphemerisSource};

pub type SharedWorld = Rc<RefCell<CoupledWorld>>;

#[derive(Clone)]
pub struct FrameEphemeris {
    pub world: SharedWorld,
    pub system_index: usize,
    bodies: Vec<CelestialBody>,
    offset: SplitPosition,
    query_frame: Option<FrameId>,
    prediction: Option<void_orbit::PredictionContext>,
    prediction_anchor: Option<u64>,
}
impl FrameEphemeris {
    pub fn new(world: SharedWorld, system: &str) -> Self {
        let w = world.borrow();
        let system_index = w.system_index(system);
        let bodies = w.bodies.clone();
        drop(w);
        Self {
            world,
            system_index,
            bodies,
            offset: SplitPosition::ORIGIN,
            query_frame: None,
            prediction: None,
            prediction_anchor: None,
        }
    }
}
impl BodyStates for FrameEphemeris {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        let world = self.world.borrow();
        let m = world
            .membership
            .get(body.0)
            .expect("frame ephemeris: unknown body");
        let states = world.at(t);
        let g = &states[m.system];
        let origin = &states[self.system_index];
        let p = g.body_position(m.local);
        let position = if m.system == self.system_index {
            p
        } else {
            g.origin.relative(&origin.origin) + p
        };
        let position = if self.offset == SplitPosition::ORIGIN {
            position
        } else {
            g.origin
                .difference(&origin.origin)
                .translate(p)
                .difference(&self.offset)
                .vector()
        };
        (
            position,
            g.velocity - origin.velocity + g.body_velocity(m.local),
        )
    }
}
/// The frame tree's view of the shared world: systems at their split barycentres, each body
/// relative to its own system (unlike `BodyStates`, which is relative to the selected system).
impl FrameSource for FrameEphemeris {
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3) {
        self.world.borrow().system_state(system, t)
    }
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        self.world.borrow().body_in_system(body, t)
    }
}

struct FramePredictionSnapshot {
    world: CoupledWorld,
    system_index: usize,
    bodies: Vec<CelestialBody>,
    offset: SplitPosition,
    query_frame: Option<FrameId>,
    context: Option<void_orbit::PredictionContext>,
    anchor: u64,
}
impl void_orbit::PredictionSnapshot for FramePredictionSnapshot {
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
    fn into_source(self: Box<Self>) -> Box<dyn EphemerisSource> {
        Box::new(FrameEphemeris {
            world: Rc::new(RefCell::new(self.world)),
            system_index: self.system_index,
            bodies: self.bodies,
            offset: self.offset,
            query_frame: self.query_frame,
            prediction: self.context,
            prediction_anchor: Some(self.anchor),
        })
    }
}
impl EphemerisSource for FrameEphemeris {
    fn export_prediction(
        &self,
    ) -> Result<Box<dyn void_orbit::PredictionSnapshot>, void_orbit::PredictionError> {
        if let Some(context) = &self.prediction {
            context.check()?;
        }
        Ok(Box::new(FramePredictionSnapshot {
            world: self.world.borrow().clone(),
            system_index: self.system_index,
            bodies: self.bodies.clone(),
            offset: self.offset,
            query_frame: self.query_frame,
            context: None,
            anchor: self
                .prediction_anchor
                .unwrap_or_else(|| self.world.borrow().continuation_fingerprint()),
        }))
    }
    fn adopt_prediction(
        &mut self,
        snapshot: Box<dyn void_orbit::PredictionSnapshot>,
    ) -> Result<(), void_orbit::PredictionError> {
        let copy = snapshot
            .into_any()
            .downcast::<FramePredictionSnapshot>()
            .map_err(|_| void_orbit::PredictionError::IncompatibleSnapshot)?;
        let mut world = self.world.borrow_mut();
        if world.continuation_fingerprint() != copy.anchor
            || !world.compatible_prediction(&copy.world)
        {
            return Err(void_orbit::PredictionError::IncompatibleSnapshot);
        }
        *world = copy.world;
        Ok(())
    }

    fn prediction_snapshot(
        &self,
        budget: void_orbit::PredictionBudget,
        cancel: void_orbit::CancellationToken,
    ) -> Result<Box<dyn void_orbit::PredictionSnapshot>, void_orbit::PredictionError> {
        let context = void_orbit::PredictionContext::new(budget, cancel);
        let world = self.world.borrow().prediction_copy(&context)?;
        Ok(Box::new(FramePredictionSnapshot {
            world,
            system_index: self.system_index,
            bodies: self.bodies.clone(),
            offset: self.offset,
            query_frame: self.query_frame,
            context: Some(context),
            anchor: self.world.borrow().continuation_fingerprint(),
        }))
    }
    fn prediction_context(&self) -> Option<&void_orbit::PredictionContext> {
        self.prediction.as_ref()
    }
    fn try_extend_to(&mut self, t: f64) -> Result<(), void_orbit::PredictionError> {
        if let Some(context) = &self.prediction {
            self.world.borrow_mut().try_extend_prediction(t, context)
        } else {
            self.extend_to(t);
            Ok(())
        }
    }

    fn system_count(&self) -> usize {
        self.world.borrow().ids.len()
    }
    fn system_of(&self, body: usize) -> SystemId {
        SystemId(
            self.world
                .borrow()
                .membership
                .get(body)
                .expect("frame ephemeris: unknown body")
                .system,
        )
    }
    fn origin_system(&self) -> SystemId {
        SystemId(self.system_index)
    }
    fn set_origin_system(&mut self, system: SystemId) {
        assert!(
            system.0 < self.world.borrow().ids.len(),
            "frame ephemeris: unknown origin system"
        );
        self.system_index = system.0;
        self.offset = SplitPosition::ORIGIN;
        self.query_frame = None;
    }
    fn physics_offset(&self) -> SplitPosition {
        self.offset
    }
    fn set_physics_offset(&mut self, offset: SplitPosition) {
        self.offset = offset;
        self.query_frame = None;
    }
    fn physics_query_frame(&self) -> Option<FrameId> {
        self.query_frame
    }
    fn set_physics_query_frame(&mut self, frame: Option<FrameId>) {
        self.query_frame = frame;
    }
    fn local_view(&self, system: SystemId) -> Option<Box<dyn EphemerisSource>> {
        assert!(
            system.0 < self.world.borrow().ids.len(),
            "frame ephemeris: unknown view system"
        );
        let mut view = self.clone();
        view.system_index = system.0;
        view.offset = SplitPosition::ORIGIN;
        view.query_frame = None;
        Some(Box::new(view))
    }
    fn bodies(&self) -> &[CelestialBody] {
        &self.bodies
    }
    fn step_seconds(&self) -> f64 {
        self.world.borrow().step_seconds
    }
    fn start_time(&self) -> f64 {
        self.world.borrow().start_time()
    }
    fn end_time(&self) -> f64 {
        self.world.borrow().time()
    }
    fn retained_bytes(&self) -> usize {
        self.world.borrow().retained_bytes()
    }
    fn extend_to(&mut self, t: f64) {
        assert!(
            self.world.borrow_mut().extend_to(t, 100_000),
            "frame ephemeris: extension exceeded step budget"
        );
    }
    fn forget_before(&mut self, _: f64) {
        panic!("frame ephemeris: prune the shared world explicitly")
    }
    fn frame_acceleration_at(&self, t: f64) -> DVec3 {
        self.world.borrow().at(t)[self.system_index].acceleration
    }
    fn states_at(&self, t: f64, positions: &mut [DVec3], velocities: Option<&mut [DVec3]>) {
        assert_eq!(
            positions.len(),
            self.bodies.len(),
            "frame ephemeris: wrong output size"
        );
        if let Some(v) = velocities.as_ref() {
            assert_eq!(
                v.len(),
                positions.len(),
                "frame ephemeris: wrong output size"
            );
        }
        let world = self.world.borrow();
        let states = world.at(t);
        let origin = &states[self.system_index];
        let mut velocities = velocities;
        for (i, m) in world.membership.iter().enumerate() {
            let g = &states[m.system];
            let p = g.body_position(m.local);
            positions[i] = if m.system == self.system_index {
                p
            } else {
                g.origin.relative(&origin.origin) + p
            };
            if self.offset != SplitPosition::ORIGIN {
                positions[i] = g
                    .origin
                    .difference(&origin.origin)
                    .translate(p)
                    .difference(&self.offset)
                    .vector();
            }
            if let Some(v) = velocities.as_deref_mut() {
                v[i] = g.velocity - origin.velocity + g.body_velocity(m.local);
            }
        }
    }
    fn positions_at(&self, t: f64, positions: &mut [DVec3]) {
        self.states_at(t, positions, None);
    }
    fn body_position(&self, body: usize, t: f64) -> DVec3 {
        BodyStates::body_state(self, BodyId(body), t).0
    }
}

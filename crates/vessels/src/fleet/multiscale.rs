//! Precision anchors for the existing Fleet and its existing owner machinery.
use super::*;

/// `local` is measured in `system`'s barycentric axes; galaxy position stays split.
pub struct PreciseVesselSnapshot {
    pub system: SystemId,
    pub anchor: SplitPosition,
    pub residual: VesselSnapshot,
    pub local: VesselSnapshot,
    pub position: SplitPosition,
    pub velocity: DVec3,
}
pub(super) struct PhysicsView {
    system: SystemId,
    offset: SplitPosition,
}
impl Fleet {
    pub(super) fn enter_system(&mut self, system: SystemId) -> PhysicsView {
        assert!(
            system.0 < self.frames.systems.len(),
            "fleet: unknown anchor system"
        );
        let previous = PhysicsView {
            system: self.ephemeris.origin_system(),
            offset: self.ephemeris.physics_offset(),
        };
        self.ephemeris.set_origin_system(system);
        self.ephemeris.set_physics_offset(SplitPosition::ORIGIN);
        self.frames.origin = self.frames.systems[system.0];
        if self.ephemeris.system_count() > 1 {
            self.ephemeris
                .set_physics_query_frame(Some(self.frames.origin));
        }
        previous
    }
    pub(super) fn set_view_offset(&mut self, offset: SplitPosition) {
        self.ephemeris.set_physics_offset(offset);
        let parent = self.frames.systems[self.ephemeris.origin_system().0];
        if offset == SplitPosition::ORIGIN {
            self.frames.origin = parent;
            if self.ephemeris.system_count() > 1 {
                self.ephemeris.set_physics_query_frame(Some(parent));
            }
            return;
        }
        let frame = match self.physics_view_frame {
            Some(frame) => {
                if self.frames.tree.parent(frame) != Some(parent) {
                    self.frames.tree.reparent(frame, parent);
                }
                frame
            }
            None => {
                let key = self.next_key;
                self.next_key += 1;
                let frame = self.frames.tree.add_split_dynamic(parent, key);
                self.dynamic.insert(key, Dynamic::PhysicsView);
                self.physics_view_frame = Some(frame);
                frame
            }
        };
        self.frames.origin = frame;
        self.ephemeris.set_physics_query_frame(Some(frame));
    }
    pub(super) fn restore_view(&mut self, previous: PhysicsView) {
        self.ephemeris.set_origin_system(previous.system);
        self.set_view_offset(previous.offset);
    }
    pub(super) fn enter_vessel(&mut self, id: &str) -> PhysicsView {
        let vessel = self.vessel(id);
        if let Owner::Scene { scene, .. } = vessel.owner {
            return self.enter_scene(scene);
        }
        let (system, offset) = (vessel.system, vessel.anchor);
        let previous = self.enter_system(system);
        self.set_view_offset(offset);
        previous
    }
    pub(super) fn enter_scene(&mut self, scene: u64) -> PhysicsView {
        let scene = &self.scenes[&scene];
        let (system, offset) = (scene.system, scene.anchor);
        let previous = self.enter_system(system);
        self.set_view_offset(offset);
        previous
    }
    pub(super) fn query_frame(&self, vessel: &Vessel) -> FrameId {
        match vessel.owner {
            Owner::Orbit { .. } => self.anchor_frames[&vessel.id],
            Owner::Scene { scene, .. } => self.scenes[&scene].anchor_frame,
        }
    }
    pub fn vessel_anchor_frame(&self, id: &str) -> FrameId {
        self.query_frame(self.vessel(id))
    }
    pub fn vessel_system(&self, id: &str) -> SystemId {
        self.vessel(id).system
    }
    pub fn precise_snapshot(&self, id: &str) -> PreciseVesselSnapshot {
        let v = self.vessel(id);
        let system = v.system;
        let anchor = match v.owner {
            Owner::Orbit { .. } => v.anchor,
            Owner::Scene { scene, .. } => self.scenes[&scene].anchor,
        };
        let residual = self.snapshot_in_frame(v, self.query_frame(v));
        let local = self.snapshot_in_frame(v, self.frames.systems[system.0]);
        let position = self
            .frames()
            .to_galaxy(self.vessel_frame(id), self.centre_of_mass_local(id));
        let velocity = self.ephemeris.system_state(system, self.time).1 + local.velocity;
        PreciseVesselSnapshot {
            system,
            anchor,
            residual,
            local,
            position,
            velocity,
        }
    }
    /// Launch in an explicitly named system: position and velocity are relative to that
    /// system barycentre, NOT galaxy absolute coordinates. Fixture callers must label their
    /// artificial starting state. For absolute coordinates use `launch_at_galaxy`.
    pub fn launch_at_split(
        &mut self,
        craft: &Craft,
        system: SystemId,
        system_position: SplitPosition,
        system_velocity: DVec3,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> String {
        let previous = self.enter_system(system);
        self.set_view_offset(system_position);
        let id = self.launch(
            craft,
            FrameState {
                position: DVec3::ZERO,
                velocity: system_velocity,
            },
            rotation,
            angular_velocity,
        );
        self.restore_view(previous);
        id
    }
    /// Explicit GALAXY absolute position/velocity; split subtraction precedes local launch.
    pub fn launch_at_galaxy(
        &mut self,
        craft: &Craft,
        system: SystemId,
        galaxy_position: SplitPosition,
        galaxy_velocity: DVec3,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> String {
        let (origin, velocity) = self.ephemeris.system_state(system, self.time);
        self.launch_at_split(
            craft,
            system,
            galaxy_position.difference(&origin),
            galaxy_velocity - velocity,
            rotation,
            angular_velocity,
        )
    }
    pub fn launch_in_system(
        &mut self,
        craft: &Craft,
        system: SystemId,
        state: FrameState,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> String {
        let previous = self.enter_system(system);
        let id = self.launch(craft, state, rotation, angular_velocity);
        self.restore_view(previous);
        id
    }
    pub(super) fn reanchor_scene(&mut self, scene: u64) {
        if self.ephemeris.system_count() == 1 {
            return;
        }
        let scene = self.scenes.get_mut(&scene).expect("reanchor scene");
        let SceneFrame::Bubble(origin) = &mut scene.world.frame else {
            return;
        };
        let delta = origin.origin(origin.origin_time()).position;
        scene.anchor = scene.anchor.translate(delta);
        origin.reanchor_origin(delta);
        for id in &scene.members {
            let vessel = self.vessels.get_mut(id).expect("scene member missing");
            vessel.anchor = scene.anchor;
            self.anchor_positions.insert(id.clone(), scene.anchor);
        }
    }
    /// Accepted boundary only: move the split anchor to the accepted COM and restart the local
    /// derivative. No fuel, attitude, module state or velocity is invented by a frame change.
    pub(super) fn reanchor_orbit(&mut self, id: &str) {
        if self.ephemeris.system_count() == 1 {
            return;
        }
        let mut vessel = self.vessels.remove(id).expect("reanchor vessel");
        let Owner::Orbit { run, .. } = &mut vessel.owner else {
            self.put(vessel);
            return;
        };
        vessel.anchor = vessel
            .anchor
            .translate(DVec3::new(run.y[0], run.y[1], run.y[2]));
        run.y[..3].fill(0.0);
        let (origin, origin_velocity) = self.ephemeris.system_state(vessel.system, run.time);
        let galaxy = origin.compose(&vessel.anchor);
        let mut nearest = vessel.system;
        let current_distance = galaxy.relative(&origin).length();
        let mut distance = current_distance;
        for system in 0..self.ephemeris.system_count() {
            let (candidate, _) = self.ephemeris.system_state(SystemId(system), run.time);
            let d = galaxy.relative(&candidate).length();
            if d < distance {
                distance = d;
                nearest = SystemId(system);
            }
        }
        if nearest != vessel.system && current_distance > 1.05 * distance {
            let (next_origin, next_velocity) = self.ephemeris.system_state(nearest, run.time);
            vessel.anchor = galaxy.difference(&next_origin);
            let delta = origin_velocity - next_velocity;
            for i in 0..3 {
                run.y[i + 3] += delta[i];
            }
            vessel.system = nearest;
        }
        **run = run.restarted();
        self.put(vessel);
    }
}

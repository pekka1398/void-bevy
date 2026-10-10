//! Precision anchors for the existing Fleet and its existing owner machinery.
use super::*;
use void_frames::State;

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
    query_frame: Option<FrameId>,
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
            query_frame: self.ephemeris.physics_query_frame(),
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
        // This reusable node is only an internal coordinate view. Loads escaping this scope
        // must be registered against their persistent owner anchor by enter_vessel/enter_scene.
        self.ephemeris.set_physics_query_frame(None);
    }
    pub(super) fn restore_view(&mut self, previous: PhysicsView) {
        self.ephemeris.set_origin_system(previous.system);
        self.set_view_offset(previous.offset);
        self.ephemeris.set_physics_query_frame(previous.query_frame);
    }
    pub(super) fn enter_vessel(&mut self, id: &str) -> PhysicsView {
        let vessel = self.vessel(id);
        if let Owner::Scene { scene, .. } = vessel.owner {
            return self.enter_scene(scene);
        }
        let (system, offset) = (vessel.system, vessel.anchor);
        let previous = self.enter_system(system);
        self.set_view_offset(offset);
        if self.ephemeris.system_count() > 1 {
            self.ephemeris
                .set_physics_query_frame(Some(self.anchor_frames[id]));
        }
        previous
    }
    pub(super) fn enter_scene(&mut self, scene: u64) -> PhysicsView {
        let scene = &self.scenes[&scene];
        let (system, offset, query) = (scene.system, scene.anchor, scene.anchor_frame);
        let previous = self.enter_system(system);
        self.set_view_offset(offset);
        if self.ephemeris.system_count() > 1 {
            self.ephemeris.set_physics_query_frame(Some(query));
        }
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
            State {
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
        state: State,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn fleet() -> Fleet {
        let world = std::rc::Rc::new(std::cell::RefCell::new(void_multiscale::wide_world(
            void_multiscale::default_galaxy(),
        )));
        let ephemeris = void_multiscale::FrameEphemeris::new(world, "Aster");
        let environment = Arc::new(Environment::new(&ephemeris).with(
            3,
            void_environment::BodyEnvironment {
                atmosphere: Some(void_environment::Atmosphere::Earth(
                    void_environment::EarthAtmosphere::new(1.0),
                )),
                air_datum_meters: 0.0,
                terrain: None,
                sea_level_meters: None,
            },
        ));
        Fleet::new(ephemeris, environment, 0.0, vec![], FleetOptions::default())
    }
    fn scoped_load(fleet: &mut Fleet, id: &str) -> void_modules::Wrench {
        let previous = fleet.enter_vessel(id);
        let v = fleet.vessel(id);
        let state = fleet.snapshot(id);
        let air = vessel_air_at(
            &fleet.environment,
            &fleet.parts,
            &v.members,
            fleet.centre(&v.members),
            state.rotation,
            fleet.time,
        )
        .unwrap();
        let load = air.wrench(
            fleet.ephemeris.as_ref(),
            fleet.time,
            State {
                position: state.position,
                velocity: state.velocity,
            },
            state.rotation,
            state.angular_velocity,
        );
        assert_eq!(load.frame, fleet.query_frame(v));
        fleet.restore_view(previous);
        load
    }
    #[test]
    fn escaped_trial_load_keeps_its_owner_frame_across_scopes_at_the_same_time() {
        let mut fleet = fleet();
        let mut craft = void_assembly::fresh_craft();
        craft.parts[0].definition_id = "aero-stabilizer-pod".into();
        let (p, v) = fleet.ephemeris.body_in_system(BodyId(3), 0.0);
        let state = State {
            position: p + DVec3::X * (fleet.ephemeris.bodies()[3].radius_meters + 5000.0),
            velocity: v + DVec3::Y * 80.0,
        };
        let a = fleet.launch_in_system(&craft, SystemId(1), state, DQuat::IDENTITY, DVec3::ZERO);
        let other = fleet.launch_in_system(
            &craft,
            SystemId(2),
            State {
                position: DVec3::X * 1e10,
                velocity: DVec3::ZERO,
            },
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        let captured = scoped_load(&mut fleet, &a);
        assert_eq!(captured.frame, fleet.vessel_anchor_frame(&a));
        let reference = fleet
            .frames()
            .to_galaxy(captured.frame, captured.reference_point);
        let previous = fleet.enter_vessel(&a);
        let nested = fleet.enter_vessel(&other);
        assert_eq!(
            fleet.ephemeris.physics_query_frame(),
            Some(fleet.vessel_anchor_frame(&other))
        );
        fleet.restore_view(nested);
        assert_eq!(fleet.ephemeris.physics_query_frame(), Some(captured.frame));
        fleet.restore_view(previous);
        assert_eq!(
            fleet
                .frames()
                .to_galaxy(captured.frame, captured.reference_point),
            reference
        );
        assert_eq!(captured, scoped_load(&mut fleet, &a));
        assert_eq!(fleet.time(), 0.0);

        // Exercise the same escaping-load contract under a persistent bubble scene anchor.
        fleet.launch_in_system(
            &craft,
            SystemId(1),
            State {
                position: state.position + DVec3::Y * 10.0,
                velocity: state.velocity,
            },
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        fleet.advance(0.0);
        assert_eq!(fleet.snapshot(&a).mode, VesselMode::Bubble);
        let scene_load = scoped_load(&mut fleet, &a);
        let scene_reference = fleet
            .frames()
            .to_galaxy(scene_load.frame, scene_load.reference_point);
        let previous = fleet.enter_vessel(&other);
        fleet.restore_view(previous);
        assert_eq!(
            fleet
                .frames()
                .to_galaxy(scene_load.frame, scene_load.reference_point),
            scene_reference
        );
        assert_eq!(scene_load, scoped_load(&mut fleet, &a));
    }
}

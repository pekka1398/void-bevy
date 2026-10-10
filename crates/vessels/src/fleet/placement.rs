//! Moving an existing vessel to a declared state. This is a starting-state tool (the main game's
//! DEV "place ship"), never propulsion: it keeps the vessel's parts, resources and module states and
//! hands the vessel to an orbit owner, from which the next accepted step reconciles it into a
//! bubble or ground scene like any launch.
use super::*;

impl Fleet {
    /// `state` is relative to `system`'s barycentre, in its axes (as `launch_in_system`).
    /// Pilot commands that belong to the old situation end: throttle and turn zero, SAS off, RCS
    /// force and torque zero (its enable switch is kept), an armed guided burn aborted.
    pub fn place(
        &mut self,
        id: &str,
        system: SystemId,
        state: State,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) {
        assert!(
            state.position.is_finite() && state.velocity.is_finite(),
            "fleet: invalid placement state for {id}"
        );
        assert!(
            rotation.is_finite()
                && (rotation.length() - 1.0).abs() < 1e-9
                && angular_velocity.is_finite(),
            "fleet: invalid placement attitude for {id}"
        );
        let from = self.snapshot(id);
        self.cancel_guidance(id, "vessel placed");
        let previous = self.enter_system(system);
        let mut v = self.vessels.remove(id).expect("fleet: unknown vessel");
        self.remove_scene_body(&v, false);
        self.recentre(&v.members);
        let multi = self.ephemeris.system_count() > 1;
        v.system = self.ephemeris.origin_system();
        v.anchor = if multi {
            self.ephemeris.physics_offset().translate(state.position)
        } else {
            SplitPosition::ORIGIN
        };
        v.owner = Owner::Orbit {
            run: Box::new(PropagationRun::new(VesselState {
                time: self.time,
                position: if multi { DVec3::ZERO } else { state.position },
                velocity: state.velocity,
                mass_kg: self.mass(&v.members),
            })),
            rotation,
            angular_velocity,
        };
        self.put(v);
        self.restore_view(previous);
        self.sas.remove(id);
        self.controls.insert(id.into(), VesselControl::default());
        let rcs = self
            .rcs_controls
            .get_mut(id)
            .expect("fleet: vessel RCS control");
        rcs.force = DVec3::ZERO;
        rcs.torque = DVec3::ZERO;
        self.event(id, Some(from.mode), Some(VesselMode::Orbit), from.scene);
    }
    /// Radius of a sphere about the centre of mass that contains every part.
    pub fn bounding_radius(&self, id: &str) -> f64 {
        let v = self.vessel(id);
        let centre = self.centre(&v.members);
        v.members
            .iter()
            .map(|p| {
                let part = self.parts.part(p);
                (part.pose.position - centre).length() + part_bound_radius(part.definition)
            })
            .fold(0.0, f64::max)
    }
    /// Lowest point of the vessel (wheels at full travel included) along its parts frame's +Y,
    /// measured from the centre of mass. Negative below it.
    pub fn lowest_along_y(&self, id: &str) -> f64 {
        let v = self.vessel(id);
        let centre = self.centre(&v.members);
        v.members
            .iter()
            .map(|p| {
                let part = self.parts.part(p);
                let mut pose = part.pose;
                pose.position -= centre;
                part_lowest_y(part.definition, &pose)
            })
            .fold(f64::INFINITY, f64::min)
    }
    /// Farthest extent of any part's bounding sphere along `axis` (a unit vector in the vessel's
    /// parts-frame axes), measured from the centre of mass.
    pub fn extent_along(&self, id: &str, axis: DVec3) -> f64 {
        assert!(
            (axis.length() - 1.0).abs() < 1e-9,
            "fleet: extent axis must be unit"
        );
        let v = self.vessel(id);
        let centre = self.centre(&v.members);
        v.members
            .iter()
            .map(|p| {
                let part = self.parts.part(p);
                (part.pose.position - centre).dot(axis) + part_bound_radius(part.definition)
            })
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

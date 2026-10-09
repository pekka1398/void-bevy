//! Conservative distant coast guards for the existing Fleet propagator.
use super::*;

/// Upper bound on the shared point-mass/J2 gravity law outside `minimum_radius`.
fn gravity_bound(body: &CelestialBody, minimum_radius: f64) -> f64 {
    assert!(minimum_radius.is_finite() && minimum_radius > 0.0);
    let point = body.gm / minimum_radius.powi(2);
    // In gravity::pull, |5 cos²(theta)-1| <= 4 and |2 cos(theta)| <= 2.
    // Their triangle bound is 6 times the 1.5 J2 coefficient.
    let bound = point
        * (1.0 + 9.0 * body.j2.abs() * (body.j2_reference_radius_meters / minimum_radius).powi(2));
    assert!(
        bound.is_finite() && bound >= 0.0,
        "rails: invalid gravity bound"
    );
    bound
}

/// A whole-interval swept sphere, not a straight-line closest-approach prediction.
fn swept_clear(state: State, protected_radius: f64, acceleration_bound: f64, dt: f64) -> bool {
    assert!(
        state.position.is_finite() && state.velocity.is_finite(),
        "rails: non-finite relative state"
    );
    assert!(protected_radius.is_finite() && protected_radius >= 0.0);
    assert!(acceleration_bound.is_finite() && acceleration_bound >= 0.0);
    assert!(dt.is_finite() && dt >= 0.0);
    let distance = state.position.length();
    let displacement = state.velocity.length() * dt + 0.5 * acceleration_bound * dt * dt;
    let roundoff_guard = 50.0 + 64.0 * f64::EPSILON * distance;
    assert!(
        distance.is_finite() && displacement.is_finite(),
        "rails: non-finite sweep"
    );
    distance - protected_radius > displacement + roundoff_guard
}

impl Fleet {
    fn coast_body_envelope(&self, index: usize) -> f64 {
        let body = &self.environment.bodies()[index];
        let mut height: f64 = 0.0;
        if let Some(place) = self.environment.body(index) {
            if let Some(terrain) = &place.terrain {
                height = height.max(terrain.max_height_meters);
            }
            if let Some(sea) = place.sea_level_meters {
                height = height.max(sea);
            }
            if let Some(air) = &place.atmosphere {
                height = height.max(place.air_datum_meters + air.ceiling_meters());
            }
        }
        let band = self
            .grounds
            .iter()
            .filter(|g| g.spec.body_index == index)
            .map(|g| g.spec.band_exit_meters)
            .fold(0.0, f64::max);
        body.radius_meters + height + band
    }

    fn coast_vessel_radius(&self, vessel: &Vessel) -> f64 {
        let centre = self.centre(&vessel.members);
        vessel
            .members
            .iter()
            .map(|id| {
                let part = self.parts.part(id);
                (part.pose.position - centre).length() + part_bound_radius(part.definition)
            })
            .fold(0.0, f64::max)
    }

    /// Current policy segment length; derived from all vessels, never the selected vessel.
    /// Near-field policy remains unchanged. The long segment requires every vessel to be
    /// independently coasting in Orbit; a sleeping ground scene also keeps the short policy.
    pub fn rails_coast_chunk_seconds(&self) -> f64 {
        let short = if has_atmosphere(&self.environment) {
            self.options.flight_chunk_seconds
        } else {
            self.options.rails_chunk_seconds
        };
        let distant = self.options.distant_coast_chunk_seconds;
        if distant > short && self.distant_coast_clear(distant) {
            distant
        } else {
            short
        }
    }

    fn distant_coast_clear(&self, dt: f64) -> bool {
        if self.order.is_empty()
            || self
                .order
                .iter()
                .any(|id| !matches!(self.vessel(id).owner, Owner::Orbit { .. }))
            || self.rails_blocker().is_some()
        {
            return false;
        }
        let bodies = self.environment.bodies();
        let at = self.frames();
        let n = bodies.len();
        let mut body_pairs = Vec::new();
        let mut body_acceleration = vec![0.0; n];
        // All relative states are formed by the frame tree: split system anchors are
        // subtracted before local f64 conversion. No galaxy absolute f64 subtraction.
        for a in 0..n {
            for b in a + 1..n {
                let state = at
                    .transform(self.frames.inertial[a], self.frames.inertial[b])
                    .apply_state(State {
                        position: DVec3::ZERO,
                        velocity: DVec3::ZERO,
                    });
                let half = state.position.length() * 0.5;
                if half <= bodies[a].radius_meters + bodies[b].radius_meters {
                    return false;
                }
                body_acceleration[a] += gravity_bound(&bodies[b], half);
                body_acceleration[b] += gravity_bound(&bodies[a], half);
                body_pairs.push((a, b, state, half));
            }
        }
        // This bound is for the coupled physical gravity law. EphemerisSource does
        // not expose interpolation-error/acceleration envelopes; the numerical
        // guard below is not a formal bound for arbitrary custom interpolators.
        // First-exit argument: while all pairs remain outside their half-distance
        // spheres, these summed acceleration bounds apply. If no swept sphere can
        // reach its boundary, no pair can be the first to leave that domain.
        for (a, b, state, half) in body_pairs {
            if !swept_clear(state, half, body_acceleration[a] + body_acceleration[b], dt) {
                return false;
            }
        }
        let mut vessel_acceleration = Vec::with_capacity(self.order.len());
        let mut vessel_radii = Vec::with_capacity(self.order.len());
        for id in &self.order {
            let vessel = self.vessel(id);
            let extent = self.coast_vessel_radius(vessel);
            vessel_radii.push(extent);
            let mut pairs = Vec::with_capacity(n);
            let mut acceleration = 0.0;
            for (body, definition) in bodies.iter().enumerate() {
                // Body inertial axes do not rotate with its surface. In particular,
                // a light-year separation introduces no artificial spin speed.
                let state = at
                    .transform(self.vessel_frame(id), self.frames.inertial[body])
                    .apply_state(State {
                        position: self.centre_of_mass_local(id),
                        velocity: DVec3::ZERO,
                    });
                let half = state.position.length() * 0.5;
                if half <= self.coast_body_envelope(body) + extent {
                    return false;
                }
                acceleration += gravity_bound(definition, half);
                pairs.push((body, state, half));
            }
            for (body, state, half) in pairs {
                if !swept_clear(state, half, acceleration + body_acceleration[body], dt) {
                    return false;
                }
            }
            vessel_acceleration.push(acceleration);
        }
        // An acceleration bound for BOTH craft covers curved gravitational encounters
        // that a straight-line encounter gate alone cannot rule out.
        for (a, id) in self.order.iter().enumerate() {
            for (b, other) in self.order.iter().enumerate().skip(a + 1) {
                let relative = self.relative(id, other);
                if !swept_clear(
                    state_of(relative),
                    self.options.encounter.pack_meters + vessel_radii[a] + vessel_radii[b],
                    vessel_acceleration[a] + vessel_acceleration[b],
                    dt,
                ) {
                    return false;
                }
            }
        }
        // The same whole-vessel/body envelope includes EVERY atmosphere, regardless
        // of thermal modules. Thus the short thermal lookahead cannot miss atmosphere
        // entry during an accepted long coast segment.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_assembly::fresh_craft;
    use void_landing::{earth_size, planet_environment, planet_ephemeris};

    fn fleet(air: bool, ground_description: bool) -> Fleet {
        let planet = earth_size();
        let (ephemeris, body) = planet_ephemeris(&planet);
        let environment = if ground_description {
            planet_environment(&planet, &ephemeris, body, air)
        } else {
            Arc::new(Environment::new(&ephemeris))
        };
        Fleet::new(ephemeris, environment, 0.0, vec![], FleetOptions::default())
    }

    fn launch(fleet: &mut Fleet, position: DVec3, velocity: DVec3) -> String {
        fleet.launch(
            &fresh_craft(),
            FrameState { position, velocity },
            DQuat::IDENTITY,
            DVec3::ZERO,
        )
    }

    #[test]
    fn distant_coast_matches_short_segments_and_checkpoint_policy_is_explicit() {
        let mut distant = fleet(true, true);
        let id = launch(&mut distant, DVec3::X * 1e9, DVec3::Y * 1000.0);
        let mut short = fleet(true, true);
        launch(&mut short, DVec3::X * 1e9, DVec3::Y * 1000.0);
        short.options.distant_coast_chunk_seconds = short.options.rails_chunk_seconds;
        assert_eq!(distant.rails_coast_chunk_seconds(), 1000.0);
        assert_eq!(short.rails_coast_chunk_seconds(), 10.0);
        let before = serde_json::to_value(distant.checkpoint()).unwrap();
        assert_eq!(before["version"], 13);
        distant.rails_coast_chunk_seconds();
        assert_eq!(serde_json::to_value(distant.checkpoint()).unwrap(), before);
        let mut missing = before;
        missing["options"]
            .as_object_mut()
            .unwrap()
            .remove("distant_coast_chunk_seconds");
        assert!(serde_json::from_value::<FleetCheckpoint>(missing).is_err());
        assert!(distant.advance_on_rails(3600.0));
        assert!(short.advance_on_rails(3600.0));
        let a = distant.snapshot(&id);
        let b = short.snapshot(&id);
        assert!((a.position - b.position).length() < 0.01);
        assert!((a.velocity - b.velocity).length() < 1e-5);
        assert_eq!(distant.time(), 3600.0);
    }

    #[test]
    fn any_near_or_fast_incoming_vessel_keeps_existing_short_policy() {
        for air in [false, true] {
            let mut f = fleet(air, true);
            launch(&mut f, DVec3::X * 1e9, DVec3::ZERO);
            let short = if air { 1.0 } else { 10.0 };
            assert_eq!(f.rails_coast_chunk_seconds(), 1000.0);
            launch(&mut f, DVec3::Y * 6_800_000.0, DVec3::ZERO);
            assert_eq!(f.rails_coast_chunk_seconds(), short);

            let mut incoming = fleet(air, true);
            launch(&mut incoming, DVec3::X * 1e9, -DVec3::X * 1e7);
            assert_eq!(incoming.rails_coast_chunk_seconds(), short);
        }
    }

    #[test]
    fn bodies_without_terrain_still_block_long_coast() {
        let mut f = fleet(false, false);
        launch(&mut f, DVec3::X * 6_800_000.0, DVec3::ZERO);
        assert_eq!(f.rails_coast_chunk_seconds(), 10.0);
    }

    #[test]
    fn atmospheric_incoming_guard_does_not_require_thermal_modules() {
        for craft in [fresh_craft(), void_assembly::reentry_capsule()] {
            let mut f = fleet(true, true);
            f.launch(
                &craft,
                FrameState {
                    position: DVec3::X * 1e9,
                    velocity: -DVec3::X * 1e7,
                },
                DQuat::IDENTITY,
                DVec3::ZERO,
            );
            assert!(f.rails_blocker().is_none(), "initial state is still vacuum");
            assert_eq!(f.rails_coast_chunk_seconds(), 1.0);
        }
    }

    #[test]
    #[should_panic(expected = "fleet checkpoint: unsupported version")]
    fn old_checkpoint_version_is_rejected_even_with_new_options() {
        let f = fleet(true, true);
        let mut value = serde_json::to_value(f.checkpoint()).unwrap();
        value["version"] = serde_json::json!(12);
        let saved = serde_json::from_value::<FleetCheckpoint>(value).unwrap();
        Fleet::from_checkpoint(f.ephemeris, f.environment, saved);
    }

    #[test]
    fn acceleration_sweep_rejects_a_pair_with_zero_relative_velocity() {
        let mut f = fleet(false, true);
        let a = launch(&mut f, DVec3::X * 20_000_000.0, DVec3::ZERO);
        launch(
            &mut f,
            DVec3::X * 20_000_000.0 + DVec3::Y * 100_000.0,
            DVec3::ZERO,
        );
        assert_eq!(f.relative(&a, "v2").velocity, DVec3::ZERO);
        assert!(f.relative(&a, "v2").position.length() > f.options.encounter.pack_meters);
        assert_eq!(f.rails_coast_chunk_seconds(), 10.0);
    }

    #[test]
    fn finite_vessel_extent_and_all_environment_envelopes_are_used() {
        let mut f = fleet(true, true);
        let id = launch(&mut f, DVec3::X * 1e13, DVec3::ZERO);
        let extent = f.coast_vessel_radius(f.vessel(&id));
        assert!(extent > 0.0);
        assert!(f.coast_body_envelope(0) > f.environment.bodies()[0].radius_meters);
        // Without the two finite craft radii this stationary pair would pass the
        // 50 metre roundoff guard; their hull envelope deliberately rejects it.
        let separation = f.options.encounter.pack_meters + 50.0 + extent;
        launch(&mut f, DVec3::X * 1e13 + DVec3::Y * separation, DVec3::ZERO);
        let relative = state_of(f.relative(&id, "v2"));
        let acceleration = 2.0 * gravity_bound(&f.environment.bodies()[0], 5e12);
        assert!(swept_clear(
            relative,
            f.options.encounter.pack_meters,
            acceleration,
            1000.0
        ));
        assert_eq!(f.rails_coast_chunk_seconds(), 1.0);
    }

    #[test]
    fn relative_guard_preserves_near_pair_precision_at_a_light_year_anchor() {
        let world = std::rc::Rc::new(std::cell::RefCell::new(void_multiscale::wide_world(
            void_multiscale::default_galaxy(),
        )));
        let ephemeris = void_multiscale::FrameEphemeris::new(world, "Aster");
        let environment = Arc::new(Environment::new(&ephemeris));
        let mut f = Fleet::new(ephemeris, environment, 0.0, vec![], FleetOptions::default());
        let anchor = SplitPosition::ORIGIN.translate(DVec3::X * 9.460_730_472_580_8e15);
        let a = f.launch_at_split(
            &fresh_craft(),
            SystemId(0),
            anchor,
            DVec3::ZERO,
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        assert_eq!(f.rails_coast_chunk_seconds(), 1000.0);
        let b = f.launch_at_split(
            &fresh_craft(),
            SystemId(0),
            anchor.translate(DVec3::X * 2500.25),
            DVec3::ZERO,
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        assert!((f.relative(&a, &b).position.length() - 2500.25).abs() < 1e-6);
        assert_eq!(f.rails_coast_chunk_seconds(), 10.0);
    }

    #[test]
    fn a_remote_system_body_without_environment_keeps_short_coast() {
        let world = std::rc::Rc::new(std::cell::RefCell::new(void_multiscale::wide_world(
            void_multiscale::default_galaxy(),
        )));
        let ephemeris = void_multiscale::FrameEphemeris::new(world, "Aster");
        let body = ephemeris.bodies().len() - 1;
        let system = ephemeris.system_of(body);
        assert_ne!(system, ephemeris.origin_system());
        let position = ephemeris.body_in_system(BodyId(body), 0.0).0
            + DVec3::X * ephemeris.bodies()[body].radius_meters * 1.1;
        let environment = Arc::new(Environment::new(&ephemeris));
        let mut f = Fleet::new(ephemeris, environment, 0.0, vec![], FleetOptions::default());
        f.launch_at_split(
            &fresh_craft(),
            system,
            SplitPosition::ORIGIN.translate(position),
            DVec3::ZERO,
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        assert_eq!(f.rails_coast_chunk_seconds(), 10.0);
    }
}

//! "Place ship": put the selected vessel into a declared starting state on the normal world —
//! a site over a body, an altitude, a velocity and an attitude, or beside another vessel. It is a
//! journalled starting-state tool, never propulsion or a transfer claim. The vessel keeps its
//! parts, resources and module states; the world is unchanged.
use crate::FleetFlight;
use glam::{DMat3, DQuat, DVec3};
use serde::{Deserialize, Serialize};
use void_assembly::ControlProfile;
use void_frames::State;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Body id as in the ephemeris (for example `aurelia`).
    pub body: String,
    pub latitude_degrees: f64,
    pub longitude_degrees: f64,
    /// Centre of mass above the surface directly below: the terrain, or the sea where the sea is
    /// higher. Ignored for `Landed`, which rests the vessel's lowest point on that surface.
    pub altitude_meters: f64,
    pub velocity: PlacementVelocity,
    pub attitude: PlacementAttitude,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PlacementVelocity {
    /// At rest on the surface (terrain or sea), upright.
    Landed,
    /// Velocity relative to the rotating surface.
    Surface {
        speed: f64,
        /// Compass heading of the horizontal part: 0 north, 90 east.
        heading_degrees: f64,
        /// Angle above the local horizon; -90 straight down.
        flight_path_degrees: f64,
    },
    /// Velocity in the body's non-rotating (inertial) frame.
    Orbital {
        speed: f64,
        heading_degrees: f64,
        flight_path_degrees: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum PlacementAttitude {
    /// Rockets nose up; aircraft, rovers and crew level with the nose along the heading.
    Upright,
    /// Nose along the placement velocity.
    Prograde,
    /// Nose against the placement velocity (heat shield first for a capsule).
    Retrograde,
}

/// Nose and top axes of a vessel in its parts frame, by its operator profile.
pub fn nose_and_top(profile: Option<ControlProfile>) -> (DVec3, DVec3) {
    match profile {
        None | Some(ControlProfile::Flight) => (DVec3::Y, DVec3::Z),
        Some(ControlProfile::Aircraft | ControlProfile::Rover | ControlProfile::Eva) => {
            (DVec3::Z, DVec3::Y)
        }
    }
}

/// What lies under a site: for the place-ship panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiteInfo {
    pub terrain_height: Option<f64>,
    pub sea_level: Option<f64>,
    /// Height of the body's own star above the horizon, degrees; None on a star.
    pub sun_elevation_degrees: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    Land,
    Ocean,
}

/// Unit direction of a latitude and longitude in the body-fixed frame (+Z north, +X at 0°).
pub(crate) fn unit_site(latitude_degrees: f64, longitude_degrees: f64) -> DVec3 {
    let (la, lo) = (
        latitude_degrees.to_radians(),
        longitude_degrees.to_radians(),
    );
    DVec3::new(la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin())
}

/// Rotation taking the local `(nose, top)` pair onto the world `(forward, up)` pair. `up` need
/// not be orthogonal to `forward`; only its part across `forward` is used.
fn aim(nose: DVec3, top: DVec3, forward: DVec3, up: DVec3, fallback_up: DVec3) -> DQuat {
    let f = forward.normalize();
    let mut t = up - f * up.dot(f);
    if t.length_squared() < 1e-12 {
        t = fallback_up - f * fallback_up.dot(f);
    }
    let t = t.normalize();
    let local = DMat3::from_cols(nose, top, nose.cross(top));
    let world = DMat3::from_cols(f, t, f.cross(t));
    DQuat::from_mat3(&(world * local.transpose())).normalize()
}

impl FleetFlight {
    fn body_named(&self, id: &str) -> Result<usize, String> {
        self.fleet
            .ephemeris
            .bodies()
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| format!("Place refused: unknown body {id}"))
    }
    fn surface_height(&self, body: usize, site: DVec3) -> Option<f64> {
        let terrain = self.terrains.get(&body).map(|t| t.height(site));
        let sea = self
            .fleet
            .environment()
            .body(body)
            .and_then(|b| b.sea_level_meters);
        match (terrain, sea) {
            (Some(t), Some(s)) => Some(t.max(s)),
            (Some(t), None) => Some(t),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }
    /// The body's own star in its surface frame, or None for a star.
    fn sun_in_surface(&self, body: usize) -> Option<DVec3> {
        let fleet = &self.fleet;
        let star = fleet
            .ephemeris
            .bodies()
            .iter()
            .find(|b| {
                b.parent_index.is_none()
                    && fleet.ephemeris.system_of(b.index) == fleet.ephemeris.system_of(body)
            })
            .expect("place: body system has no root star");
        (star.index != body).then(|| {
            fleet
                .frames()
                .transform(fleet.body_frames(star.index).0, fleet.body_frames(body).1)
                .apply_point(DVec3::ZERO)
                .normalize()
        })
    }
    pub fn site_info(
        &self,
        body: &str,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<SiteInfo, String> {
        let b = self.body_named(body)?;
        let d = unit_site(latitude_degrees, longitude_degrees);
        Ok(SiteInfo {
            terrain_height: self.terrains.get(&b).map(|t| t.height(d)),
            sea_level: self
                .fleet
                .environment()
                .body(b)
                .and_then(|e| e.sea_level_meters),
            sun_elevation_degrees: self
                .sun_in_surface(b)
                .map(|sun| sun.dot(d).clamp(-1.0, 1.0).asin().to_degrees()),
        })
    }
    /// Latitude and longitude (degrees) of the selected vessel over a body.
    pub fn site_of(&self, vessel: &str, body: &str) -> Result<(f64, f64), String> {
        let b = self.body_named(body)?;
        let p = self
            .fleet
            .frames()
            .transform(self.fleet.vessel_frame(vessel), self.fleet.body_frames(b).1)
            .apply_point(self.fleet.centre_of_mass_local(vessel))
            .normalize();
        Ok((p.z.asin().to_degrees(), p.y.atan2(p.x).to_degrees()))
    }
    /// A site in daylight, nearest local noon: dry, level land or open sea (deeper than 50 m).
    pub fn daylight_site(&self, body: &str, kind: SiteKind) -> Result<(f64, f64), String> {
        let b = self.body_named(body)?;
        let terrain = self
            .terrains
            .get(&b)
            .ok_or_else(|| format!("{body} has no solid surface"))?;
        let sun = self
            .sun_in_surface(b)
            .ok_or_else(|| format!("{body} is a star"))?;
        let sea = self
            .fleet
            .environment()
            .body(b)
            .and_then(|e| e.sea_level_meters);
        if kind == SiteKind::Ocean && sea.is_none() {
            return Err(format!("{body} has no sea"));
        }
        let count = 4096;
        let best = (0..count)
            .filter_map(|i| {
                let z = 1. - 2. * (i as f64 + 0.5) / count as f64;
                let t = i as f64 * 2.399963229728653;
                let r = (1. - z * z).sqrt();
                let d = DVec3::new(r * t.cos(), r * t.sin(), z);
                let light = d.dot(sun);
                if light < 0.5 {
                    return None;
                }
                let height = terrain.height(d);
                let fits = match kind {
                    SiteKind::Ocean => height < sea.unwrap() - 50.0,
                    SiteKind::Land => {
                        let side = DVec3::Z.cross(d).try_normalize().unwrap_or(DVec3::X);
                        let other = d.cross(side);
                        let radius = terrain.radius_meters + height;
                        let level = [side, -side, other, -other].into_iter().all(|s| {
                            (terrain.height((d * radius + s * 12.0).normalize()) - height).abs()
                                < 12.0 * 0.025
                        });
                        sea.is_none_or(|s| height > s + 20.0) && level
                    }
                };
                fits.then_some((light, d))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .ok_or_else(|| format!("no daylight {kind:?} site found on {body}"))?
            .1;
        Ok((
            best.z.asin().to_degrees(),
            best.y.atan2(best.x).to_degrees(),
        ))
    }
    /// Circular orbit speed at a placement's radius, in the body's inertial frame.
    pub fn circular_speed(&self, placement: &Placement) -> Result<f64, String> {
        let b = self.body_named(&placement.body)?;
        let d = unit_site(placement.latitude_degrees, placement.longitude_degrees);
        let body = &self.fleet.ephemeris.bodies()[b];
        let r = body.radius_meters
            + self.surface_height(b, d).unwrap_or(0.0)
            + placement.altitude_meters;
        Ok((body.gm / r).sqrt())
    }

    /// Moves the selected vessel. Refusals leave the world unchanged.
    pub fn place(&mut self, placement: &Placement) -> Result<(), String> {
        let id = self.selected.clone();
        let b = self.body_named(&placement.body)?;
        let Placement {
            latitude_degrees,
            longitude_degrees,
            altitude_meters,
            velocity,
            attitude,
            ..
        } = *placement;
        let finite = [latitude_degrees, longitude_degrees, altitude_meters]
            .iter()
            .all(|v| v.is_finite());
        let (speed, heading, path) = match velocity {
            PlacementVelocity::Landed => (0.0, 0.0, 0.0),
            PlacementVelocity::Surface {
                speed,
                heading_degrees,
                flight_path_degrees,
            }
            | PlacementVelocity::Orbital {
                speed,
                heading_degrees,
                flight_path_degrees,
            } => (speed, heading_degrees, flight_path_degrees),
        };
        if !finite || !speed.is_finite() || !heading.is_finite() || !path.is_finite() {
            return Err("Place refused: values must be finite".into());
        }
        if !(-90.0..=90.0).contains(&latitude_degrees) || !(-90.0..=90.0).contains(&path) {
            return Err("Place refused: latitude and flight path must be within ±90°".into());
        }
        if speed < 0.0 {
            return Err("Place refused: speed must not be negative".into());
        }
        let fleet = &self.fleet;
        let body = &fleet.ephemeris.bodies()[b];
        let d = unit_site(latitude_degrees, longitude_degrees);
        let surface = self.surface_height(b, d);
        let (nose, top) = nose_and_top(fleet.control_profile(&id));
        let east = DVec3::Z.cross(d).try_normalize().unwrap_or(DVec3::Y);
        let north = d.cross(east);
        let horizontal = north * heading.to_radians().cos() + east * heading.to_radians().sin();
        let direction = horizontal * path.to_radians().cos() + d * path.to_radians().sin();
        let system = fleet.ephemeris.system_of(b);
        let system_frame = fleet.system_frames().systems[system.0];
        let (surface_frame, inertial_frame) = (fleet.body_frames(b).1, fleet.body_frames(b).0);
        let frames = fleet.frames();
        let to_system = frames.transform(surface_frame, system_frame);
        let (state, rotation, angular_velocity) = if velocity == PlacementVelocity::Landed {
            let surface = surface
                .ok_or_else(|| format!("Place refused: {} has no surface to land on", body.name))?;
            if attitude != PlacementAttitude::Upright {
                return Err("Place refused: a landed vessel is placed upright".into());
            }
            let r = body.radius_meters + surface - fleet.lowest_along_y(&id) + 0.05;
            let local = aim(nose, top, d, north, east);
            let upright = if nose == DVec3::Y {
                local
            } else {
                aim(nose, top, horizontal, d, north)
            };
            let state = to_system.apply_state(State {
                position: d * r,
                velocity: DVec3::ZERO,
            });
            (
                state,
                to_system.rotation() * upright,
                to_system.to_motion().angular_velocity,
            )
        } else {
            let clearance = fleet.bounding_radius(&id);
            if altitude_meters < clearance {
                return Err(format!(
                    "Place refused: altitude {altitude_meters:.1} m is inside the vessel's {clearance:.1} m bounding radius"
                ));
            }
            let r = body.radius_meters + surface.unwrap_or(0.0) + altitude_meters;
            let state = match velocity {
                PlacementVelocity::Surface { .. } => to_system.apply_state(State {
                    position: d * r,
                    velocity: direction * speed,
                }),
                PlacementVelocity::Orbital { .. } => {
                    let to_inertial = frames.transform(surface_frame, inertial_frame);
                    frames
                        .transform(inertial_frame, system_frame)
                        .apply_state(State {
                            position: to_inertial.apply_point(d * r),
                            velocity: to_inertial.apply_direction(direction) * speed,
                        })
                }
                PlacementVelocity::Landed => unreachable!(),
            };
            let local = match attitude {
                PlacementAttitude::Upright if nose == DVec3::Y => aim(nose, top, d, north, east),
                PlacementAttitude::Upright => aim(nose, top, horizontal, d, north),
                PlacementAttitude::Prograde | PlacementAttitude::Retrograde => {
                    if speed == 0.0 {
                        return Err("Place refused: prograde or retrograde needs a speed".into());
                    }
                    let sign = if attitude == PlacementAttitude::Prograde {
                        1.0
                    } else {
                        -1.0
                    };
                    aim(nose, top, direction * sign, d, north)
                }
            };
            (state, to_system.rotation() * local, DVec3::ZERO)
        };
        self.fleet
            .place(&id, system, state, rotation, angular_velocity);
        self.fleet.advance(0.0);
        self.after_place(&id);
        Ok(())
    }

    /// Moves the selected vessel ahead of `target`'s nose, turned to face it, with matched
    /// velocity and spin; `gap_meters` is at least the space between the two vessels' bounding
    /// extents along that line.
    pub fn place_near(&mut self, target: &str, gap_meters: f64) -> Result<(), String> {
        let id = self.selected.clone();
        if target == id {
            return Err("Place refused: target is the selected vessel".into());
        }
        if !self.fleet.vessel_ids().iter().any(|v| v == target) {
            return Err(format!("Place refused: unknown vessel {target}"));
        }
        if !gap_meters.is_finite() || gap_meters < 0.0 {
            return Err("Place refused: gap must be finite and not negative".into());
        }
        let fleet = &self.fleet;
        let (target_nose, target_top) = nose_and_top(fleet.control_profile(target));
        let (nose, top) = nose_and_top(fleet.control_profile(&id));
        let system = fleet.vessel_system(target);
        let to_system = fleet.frames().transform(
            fleet.vessel_frame(target),
            fleet.system_frames().systems[system.0],
        );
        let motion = to_system.to_motion();
        let centre = to_system.apply_state(State {
            position: fleet.centre_of_mass_local(target),
            velocity: DVec3::ZERO,
        });
        let forward = to_system.rotation() * target_nose;
        let distance =
            fleet.extent_along(target, target_nose) + gap_meters + fleet.extent_along(&id, nose);
        let offset = forward * distance;
        let state = State {
            position: centre.position + offset,
            velocity: centre.velocity + motion.angular_velocity.cross(offset),
        };
        let rotation = aim(
            nose,
            top,
            -forward,
            to_system.rotation() * target_top,
            to_system.rotation() * DVec3::X,
        );
        self.fleet
            .place(&id, system, state, rotation, motion.angular_velocity);
        self.fleet.advance(0.0);
        self.after_place(&id);
        Ok(())
    }

    fn after_place(&mut self, id: &str) {
        self.cancel_maneuver_warp("vessel placed");
        // Maneuvers were planned from the old trajectory.
        self.plans.remove(id);
        self.update_plans();
    }
}

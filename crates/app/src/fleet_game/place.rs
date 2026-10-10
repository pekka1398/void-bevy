//! DEV "place ship": the one way to set up a situation (reentry, splashdown, orbit, a second ship
//! nearby). It edits a draft here and applies it through the journalled `Action::Place` /
//! `Action::PlaceNear`, so recordings, replays and saves carry it like any other command.
use super::*;
use void_fleet_flight::placement::{Placement, PlacementAttitude, PlacementVelocity, SiteKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VelocityKind {
    Landed,
    Surface,
    Orbital,
}
/// The panel's editable placement.
#[derive(Resource, Clone, Debug)]
pub(super) struct PlaceDraft {
    pub body: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub velocity: VelocityKind,
    pub speed: f64,
    pub heading: f64,
    pub path: f64,
    pub attitude: PlacementAttitude,
    /// Vessel for "place near target".
    pub target: Option<String>,
    pub gap: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlaceField {
    Latitude,
    Longitude,
    Altitude,
    Speed,
    Heading,
    Path,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlaceClick {
    BodyPrevious,
    BodyNext,
    SiteHere,
    SiteLand,
    SiteOcean,
    Velocity,
    Attitude,
    Circular,
    Target,
    Apply,
    ApplyNear,
}
impl PlaceDraft {
    pub fn new(sim: &void_fleet_flight::FleetFlight) -> Self {
        let body = sim.fleet.ephemeris.bodies()[sim.navigation_body(&sim.selected)]
            .id
            .clone();
        let (latitude, longitude) = sim
            .site_of(&sim.selected, &body)
            .expect("selected vessel's navigation body");
        Self {
            body,
            latitude,
            longitude,
            altitude: 1000.0,
            velocity: VelocityKind::Surface,
            speed: 0.0,
            heading: 90.0,
            path: 0.0,
            attitude: PlacementAttitude::Upright,
            target: None,
            gap: 5.0,
        }
    }
    pub fn placement(&self) -> Placement {
        Placement {
            body: self.body.clone(),
            latitude_degrees: self.latitude,
            longitude_degrees: self.longitude,
            altitude_meters: self.altitude,
            velocity: match self.velocity {
                VelocityKind::Landed => PlacementVelocity::Landed,
                VelocityKind::Surface => PlacementVelocity::Surface {
                    speed: self.speed,
                    heading_degrees: self.heading,
                    flight_path_degrees: self.path,
                },
                VelocityKind::Orbital => PlacementVelocity::Orbital {
                    speed: self.speed,
                    heading_degrees: self.heading,
                    flight_path_degrees: self.path,
                },
            },
            attitude: self.attitude,
        }
    }
    pub fn value(&self, field: PlaceField) -> f64 {
        match field {
            PlaceField::Latitude => self.latitude,
            PlaceField::Longitude => self.longitude,
            PlaceField::Altitude => self.altitude,
            PlaceField::Speed => self.speed,
            PlaceField::Heading => self.heading,
            PlaceField::Path => self.path,
            PlaceField::Gap => self.gap,
        }
    }
    pub fn set(&mut self, field: PlaceField, value: f64) {
        *match field {
            PlaceField::Latitude => &mut self.latitude,
            PlaceField::Longitude => &mut self.longitude,
            PlaceField::Altitude => &mut self.altitude,
            PlaceField::Speed => &mut self.speed,
            PlaceField::Heading => &mut self.heading,
            PlaceField::Path => &mut self.path,
            PlaceField::Gap => &mut self.gap,
        } = value;
    }
}

pub(super) fn click(pilot: &mut Pilot, draft: &mut PlaceDraft, click: PlaceClick) {
    let sim = pilot.flight.session.sim();
    match click {
        PlaceClick::BodyPrevious | PlaceClick::BodyNext => {
            let bodies = sim.fleet.ephemeris.bodies();
            let n = bodies.len();
            // A loaded save can bring another world, whose bodies the draft does not name.
            let Some(i) = bodies.iter().position(|b| b.id == draft.body) else {
                pilot.notice.0 = format!(
                    "{} is not in this world; body set to {}",
                    draft.body, bodies[0].name
                );
                draft.body = bodies[0].id.clone();
                return;
            };
            let next = if click == PlaceClick::BodyNext {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            draft.body = bodies[next].id.clone();
        }
        PlaceClick::SiteHere => match sim.site_of(&sim.selected, &draft.body) {
            Ok((lat, lon)) => (draft.latitude, draft.longitude) = (lat, lon),
            Err(reason) => pilot.notice.0 = reason,
        },
        PlaceClick::SiteLand | PlaceClick::SiteOcean => {
            let kind = if click == PlaceClick::SiteLand {
                SiteKind::Land
            } else {
                SiteKind::Ocean
            };
            match sim.daylight_site(&draft.body, kind) {
                Ok((lat, lon)) => (draft.latitude, draft.longitude) = (lat, lon),
                Err(reason) => pilot.notice.0 = format!("Site refused: {reason}"),
            }
        }
        PlaceClick::Velocity => {
            draft.velocity = match draft.velocity {
                VelocityKind::Landed => VelocityKind::Surface,
                VelocityKind::Surface => VelocityKind::Orbital,
                VelocityKind::Orbital => VelocityKind::Landed,
            };
            if draft.velocity == VelocityKind::Landed {
                draft.attitude = PlacementAttitude::Upright;
            }
        }
        PlaceClick::Attitude => {
            draft.attitude = match draft.attitude {
                PlacementAttitude::Upright => PlacementAttitude::Prograde,
                PlacementAttitude::Prograde => PlacementAttitude::Retrograde,
                PlacementAttitude::Retrograde => PlacementAttitude::Upright,
            };
        }
        PlaceClick::Circular => {
            draft.velocity = VelocityKind::Orbital;
            draft.path = 0.0;
            match sim.circular_speed(&draft.placement()) {
                Ok(speed) => draft.speed = speed,
                Err(reason) => pilot.notice.0 = reason,
            }
        }
        PlaceClick::Target => {
            let fleet = &sim.fleet;
            let mut others: Vec<_> = fleet
                .vessel_ids()
                .into_iter()
                .filter(|id| *id != sim.selected)
                .collect();
            others.sort_by(|a, b| {
                fleet
                    .relative(a, &sim.selected)
                    .position
                    .length_squared()
                    .total_cmp(&fleet.relative(b, &sim.selected).position.length_squared())
            });
            draft.target = draft
                .target
                .as_ref()
                .and_then(|t| others.iter().position(|o| o == t))
                .map_or(others.first(), |i| others.get(i + 1))
                .cloned();
        }
        PlaceClick::Apply => {
            let placement = draft.placement();
            apply(pilot, Action::Place { placement });
        }
        PlaceClick::ApplyNear => {
            let Some(target) = draft.target.clone() else {
                pilot.notice.0 = "Place refused: choose a target vessel first".into();
                return;
            };
            apply(
                pilot,
                Action::PlaceNear {
                    target,
                    gap_meters: draft.gap,
                },
            );
        }
    }
}
fn apply(pilot: &mut Pilot, action: Action) {
    neutral_pilot(&mut pilot.flight.session);
    match pilot.flight.session.execute(action) {
        Outcome::Applied => {
            pilot.flight.paused = true;
            pilot.flight.rate = 0;
            pilot.forecast.coast = None;
            pilot.docking.own = None;
            pilot.docking.target = None;
            refresh_ports(&mut pilot.docking, &pilot.flight.session);
            pilot.notice.0 = "Ship placed (paused). P resumes; F6 saves this situation.".into();
        }
        Outcome::Refused(reason) => pilot.notice.0 = reason,
        other => panic!("unexpected placement outcome {other:?}"),
    }
}

/// Line count of `describe`; the panel reserves exactly this much room.
pub(super) const DESCRIBE_LINES: usize = 6;

/// The panel's summary of the draft and the site under it, always `DESCRIBE_LINES` lines. Each
/// line holds one kind of value so a longer value is clipped at the panel edge, never folded.
pub(super) fn describe(draft: &PlaceDraft, sim: &void_fleet_flight::FleetFlight) -> String {
    let name = sim
        .fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|b| b.id == draft.body)
        .map_or(draft.body.as_str(), |b| b.name.as_str());
    let site = match sim.site_info(&draft.body, draft.latitude, draft.longitude) {
        Ok(info) => format!(
            "terrain {} · sea {} · sun {}",
            info.terrain_height
                .map_or("none".into(), |h| format!("{h:.0} m")),
            match (info.sea_level, info.terrain_height) {
                (Some(sea), Some(t)) if t < sea => format!("{:.0} m deep", sea - t),
                (Some(_), _) => "dry".into(),
                (None, _) => "none".into(),
            },
            info.sun_elevation_degrees
                .map_or("—".into(), |e| format!("{e:+.0}°")),
        ),
        // Refusal reasons are single sentences; keep them on this line.
        Err(reason) => reason.replace('\n', " "),
    };
    let velocity = match draft.velocity {
        VelocityKind::Landed => "landed".to_owned(),
        kind => format!(
            "{} {:.1} m/s · heading {:.0}° · path {:+.1}°",
            if kind == VelocityKind::Surface {
                "surface"
            } else {
                "orbital"
            },
            draft.speed,
            draft.heading,
            draft.path
        ),
    };
    let target = draft.target.as_ref().map_or("none".into(), |t| {
        if sim.fleet.vessel_ids().contains(t) {
            format!("{} ({t})", sim.fleet.snapshot(t).name)
        } else {
            format!("{t} (gone)")
        }
    });
    let text = format!(
        "PLACE SHIP · {} ({})\n{name} · lat {:.3}° lon {:.3}° · alt {:.0} m\n{site}\n{velocity}\nattitude {:?}\nnear target · gap {:.1} m · {target}",
        sim.fleet.snapshot(&sim.selected).name,
        sim.selected,
        draft.latitude,
        draft.longitude,
        draft.altitude,
        draft.attitude,
        draft.gap
    );
    assert_eq!(
        text.lines().count(),
        DESCRIBE_LINES,
        "place summary line count: {text}"
    );
    text
}

//! Automatic navigation in the maneuver panel: pick a target body, then Depart, Correct or
//! Capture generates one maneuver node. The search runs on a background thread from a checkpoint
//! copy of the world; its plan is accepted only if the world it was computed from is unchanged
//! (`Action::AcceptNavigation`). Generating never ignites the engine.
use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};
use void_fleet_flight::navigation_job::{NavigationJob, NavigationResult, baseline};
use void_orbit::{NavigationOperation, NavigationRequest};

/// Periapsis altitude asked of a capture or arrival, metres.
const PERIAPSIS_ALTITUDE_M: f64 = 100_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavClick {
    TargetPrevious,
    TargetNext,
    Operation(Operation),
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Depart,
    Correct,
    Capture,
}
impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Depart => "Departure",
            Self::Correct => "Correction",
            Self::Capture => "Capture",
        }
    }
}

struct Search {
    // A Mutex only to make the resource Sync; one system polls it.
    receiver: std::sync::Mutex<Receiver<Result<NavigationResult, String>>>,
    label: String,
    started: std::time::Instant,
}

#[derive(Resource, Default)]
pub(super) struct NavigationUi {
    pub target: Option<usize>,
    search: Option<Search>,
    /// A finished search waiting for its prediction cache before it is accepted.
    pending: Option<(String, NavigationResult)>,
    status: String,
}
impl NavigationUi {
    pub fn busy(&self) -> bool {
        self.search.is_some() || self.pending.is_some()
    }
}

/// Why a navigation control cannot be used now.
pub(super) fn unavailable(click: NavClick, nav: &NavigationUi, flight: &Flight) -> Option<String> {
    match click {
        NavClick::Cancel => (!nav.busy()).then(|| "No navigation search is running".into()),
        NavClick::TargetPrevious | NavClick::TargetNext => nav
            .busy()
            .then(|| "Cancel the running search before changing the target".into()),
        NavClick::Operation(_) => {
            if nav.busy() {
                return Some("A navigation search is already running".into());
            }
            if nav.target.is_none() {
                return Some("Choose a navigation target".into());
            }
            let sim = flight.session.sim();
            sim.plan_engine(&sim.selected).err()
        }
    }
}

pub(super) fn click(pilot: &mut Pilot, nav: &mut NavigationUi, click: NavClick) {
    match click {
        NavClick::TargetPrevious | NavClick::TargetNext => {
            let sim = pilot.flight.session.sim();
            let n = sim.fleet.ephemeris.bodies().len();
            let here = sim.navigation_body(&sim.selected);
            let step = |i: usize| {
                if click == NavClick::TargetNext {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }
            };
            let mut next = step(nav.target.unwrap_or(here));
            // The body the vessel is navigating around is not a destination.
            if next == here {
                next = step(next);
            }
            nav.target = Some(next);
            nav.status.clear();
        }
        NavClick::Cancel => {
            // The thread finishes its bounded search on its own; its result is dropped.
            nav.search = None;
            nav.pending = None;
            nav.status = "Search cancelled".into();
        }
        NavClick::Operation(operation) => start(pilot, nav, operation),
    }
}

fn start(pilot: &mut Pilot, nav: &mut NavigationUi, operation: Operation) {
    let target = nav.target.expect("operation enabled without a target");
    let sim = pilot.flight.session.sim();
    let id = sim.selected.clone();
    let now = sim.fleet.time();
    // After the last planned burn, so an existing plan is kept and extended.
    let earliest = sim
        .plans
        .get(&id)
        .and_then(|p| p.plan.burns().last())
        .map_or(now, |b| now.max(b.end_time))
        + 30.0;
    let request = NavigationRequest {
        operation: match operation {
            Operation::Depart => NavigationOperation::Departure,
            Operation::Correct => NavigationOperation::Correction,
            Operation::Capture => NavigationOperation::Capture,
        },
        target_body: target,
        reference_body: sim.navigation_body(&id),
        earliest_departure: earliest,
        latest_departure: earliest + wait_seconds(sim),
        min_flight_seconds: 60.0,
        max_flight_seconds: flight_seconds(sim, target),
        periapsis_altitude_m: PERIAPSIS_ALTITUDE_M,
    };
    let label = format!(
        "{} to {}",
        operation.label(),
        sim.fleet.ephemeris.bodies()[target].name
    );
    // The search is from this instant; pause so the world it was computed from stays current.
    pilot.flight.paused = true;
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        pilot.flight.session.sim(),
        pilot.flight.session.recording_initial().clone(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("navigation search".into())
        .spawn(move || {
            // The receiver is gone after Cancel; nothing is left to tell.
            let _ = sender.send(
                NavigationJob {
                    checkpoint,
                    request,
                }
                .solve(),
            );
        })
        .expect("spawn navigation search thread");
    nav.status = format!("{label}: searching (paused)");
    nav.search = Some(Search {
        receiver: std::sync::Mutex::new(receiver),
        label,
        started: std::time::Instant::now(),
    });
}

/// One local orbit of waiting samples every departure phase; the search never parks for months.
fn wait_seconds(sim: &void_fleet_flight::FleetFlight) -> f64 {
    let reference = sim.navigation_body(&sim.selected);
    let local = sim.fleet.body_fixed_state(&sim.selected, reference);
    let gm = sim.fleet.ephemeris.bodies()[reference].gm;
    std::f64::consts::TAU * (local.position.length().powi(3) / gm).sqrt()
}

/// The longest flight searched: half the target's period for a moon of the current body, or
/// twice the Hohmann time between sibling orbits. A search setting, not a predicted arrival.
fn flight_seconds(sim: &void_fleet_flight::FleetFlight, target: usize) -> f64 {
    let bodies = sim.fleet.ephemeris.bodies();
    let reference = &bodies[sim.navigation_body(&sim.selected)];
    let body = &bodies[target];
    let target_period = body.orbit_period_seconds.unwrap_or(7.0 * 86400.0);
    let seconds = if body.parent_index == reference.parent_index {
        reference
            .orbit_period_seconds
            .map_or(target_period, |period| {
                ((period.powf(2.0 / 3.0) + target_period.powf(2.0 / 3.0)) / 2.0).powf(1.5)
            })
    } else {
        target_period * 0.5
    };
    seconds.max(86400.0)
}

/// Collects a finished search, warms its prediction a slice per frame, then accepts it.
pub(super) fn poll(pilot: &mut Pilot, nav: &mut NavigationUi) {
    if let Some(search) = &nav.search {
        let received = search
            .receiver
            .lock()
            .expect("navigation receiver lock")
            .try_recv();
        match received {
            Err(TryRecvError::Empty) => {
                nav.status = format!(
                    "{}: searching {:.0}s (paused)",
                    search.label,
                    search.started.elapsed().as_secs_f64()
                );
                return;
            }
            Err(TryRecvError::Disconnected) => {
                panic!("navigation search thread ended without a result")
            }
            Ok(Err(reason)) => {
                nav.status = format!("{}: {reason}", search.label);
                nav.search = None;
                return;
            }
            Ok(Ok(result)) => {
                let label = search.label.clone();
                nav.search = None;
                nav.pending = Some((label, result));
            }
        }
    }
    let Some((label, result)) = &nav.pending else {
        return;
    };
    if baseline(pilot.flight.session.sim()) != result.baseline {
        nav.status = format!("{label}: world changed during the search; generate again");
        nav.pending = None;
        return;
    }
    if !pilot
        .flight
        .session
        .prepare_navigation_preview(result.prediction_until)
    {
        nav.status = format!("{label}: preparing prediction");
        return;
    }
    let (label, result) = nav.pending.take().unwrap();
    nav.status = match pilot.flight.session.execute(Action::AcceptNavigation {
        result: Box::new(result),
    }) {
        Outcome::Applied => {
            pilot.forecast.coast = None;
            format!("{label}: node added; check the prediction, then Execute")
        }
        Outcome::Refused(reason) => format!("{label}: {reason}"),
        other => panic!("unexpected navigation outcome {other:?}"),
    };
}

/// Three fixed lines: target, the automatic search settings, and the last status.
pub(super) fn describe(nav: &NavigationUi, sim: &void_fleet_flight::FleetFlight) -> String {
    let bodies = sim.fleet.ephemeris.bodies();
    let Some(target) = nav.target.filter(|t| *t < bodies.len()) else {
        return format!("target —\nchoose with ◀ Target ▶\n{}", nav.status);
    };
    format!(
        "target {}\nwait ≤ {:.1} h · flight ≤ {:.1} d · Pe {:.0} km\n{}",
        bodies[target].name,
        wait_seconds(sim) / 3600.0,
        flight_seconds(sim, target) / 86400.0,
        PERIAPSIS_ALTITUDE_M / 1000.0,
        nav.status
    )
}

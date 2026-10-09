//! Bounded transfer search. Two-body guesses are checked against the ordinary finite-thrust
//! flight plan; neither the anchor nor the caller's existing plan is changed.
use crate::{
    AdvanceOutcome, EphemerisSource, FlightPlan, ManeuverSpec, PlanEngine, PropagationRun,
    ReferenceMode, Tolerances, Trajectory, VesselPropagator, VesselState,
};
use glam::{DMat3, DVec3};
use void_frames::BodyId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NavigationOperation {
    Departure,
    Correction,
    Capture,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct NavigationRequest {
    pub operation: NavigationOperation,
    pub target_body: usize,
    pub reference_body: usize,
    pub earliest_departure: f64,
    pub latest_departure: f64,
    pub min_flight_seconds: f64,
    pub max_flight_seconds: f64,
    pub periapsis_altitude_m: f64,
}
#[derive(Clone, Debug)]
pub struct NavigationSolution {
    pub maneuver: ManeuverSpec,
    pub delta_v_mps: f64,
    pub closest_time: f64,
    pub closest_distance_m: f64,
    pub periapsis_altitude_m: f64,
    pub relative_speed_mps: f64,
    pub verified_until: f64,
    /// Capture is only reported after negative target-relative two-body energy was checked.
    pub captured: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum NavigationError {
    InvalidRequest(String),
    NoSolution(String),
    InsufficientFuel,
    Impact { body: usize, time: f64 },
    PredictionBudget,
    CaptureUnavailable(String),
}
impl std::fmt::Display for NavigationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(s) | Self::NoSolution(s) | Self::CaptureUnavailable(s) => {
                f.write_str(s)
            }
            Self::InsufficientFuel => f.write_str("not enough propellant for the candidate burn"),
            Self::Impact { body, time } => {
                write!(f, "predicted impact with body {body} at T+{time:.1}")
            }
            Self::PredictionBudget => {
                f.write_str("navigation prediction exhausted its integration budget")
            }
        }
    }
}
impl std::error::Error for NavigationError {}
const STEP_BUDGET: u64 = 120_000;
const WINDOW_COAST_BUDGET: u64 = 2_000_000;

fn coast(
    ep: &mut dyn EphemerisSource,
    anchor: &PropagationRun,
    t: f64,
    tol: Tolerances,
) -> Result<VesselState, NavigationError> {
    let mut run = anchor.restarted();
    match VesselPropagator::new(ep, tol).advance(ep, &mut run, t, STEP_BUDGET, None, None) {
        AdvanceOutcome::Reached => Ok(run.state()),
        AdvanceOutcome::Budget => Err(NavigationError::PredictionBudget),
        AdvanceOutcome::Impact { .. } => {
            let i = run.impact.expect("impact outcome");
            Err(NavigationError::Impact {
                body: i.body,
                time: i.time,
            })
        }
    }
}
fn axes(
    ep: &mut dyn EphemerisSource,
    state: VesselState,
    reference: usize,
) -> Result<DMat3, NavigationError> {
    ep.extend_to(state.time);
    let (p, v) = ep.body_state(BodyId(reference), state.time);
    let r = state.position - p;
    let v = state.velocity - v;
    if v.length_squared() == 0.0 || r.cross(v).length_squared() == 0.0 {
        return Err(NavigationError::NoSolution(
            "maneuver reference has a degenerate Frenet frame".into(),
        ));
    }
    let tangent = v.normalize();
    let normal = r.cross(v).normalize();
    Ok(DMat3::from_cols(tangent, normal, tangent.cross(normal)))
}
fn spec(time: f64, reference: usize, dv: DVec3) -> ManeuverSpec {
    ManeuverSpec {
        start_time: time,
        reference_body: reference,
        reference_mode: ReferenceMode::Fixed,
        prograde: dv.x,
        normal: dv.y,
        radial: dv.z,
    }
}
fn verify(
    ep: &mut dyn EphemerisSource,
    anchor: &PropagationRun,
    engine: PlanEngine,
    tol: Tolerances,
    m: ManeuverSpec,
    end: f64,
) -> Result<FlightPlan, NavigationError> {
    verify_budget(ep, anchor, engine, tol, m, end, STEP_BUDGET)
}
fn verify_budget(
    ep: &mut dyn EphemerisSource,
    anchor: &PropagationRun,
    engine: PlanEngine,
    tol: Tolerances,
    m: ManeuverSpec,
    end: f64,
    max_steps: u64,
) -> Result<FlightPlan, NavigationError> {
    let mut plan = FlightPlan::new(ep, tol, engine, (end - anchor.time).max(1.0));
    plan.rebase(anchor);
    plan.add(m);
    let burn = plan
        .status(0)
        .as_ref()
        .map_err(|_| NavigationError::InsufficientFuel)?;
    if burn.end_time >= end {
        return Err(NavigationError::NoSolution(
            "burn lasts beyond the requested arrival window".into(),
        ));
    }
    plan.set_coast_seconds(end - burn.end_time);
    plan.extend(ep, max_steps);
    if let Some(i) = plan.impact() {
        return Err(NavigationError::Impact {
            body: i.body,
            time: i.time,
        });
    }
    if !plan.complete() {
        return Err(NavigationError::PredictionBudget);
    }
    Ok(plan)
}
fn relative(ep: &dyn EphemerisSource, path: &Trajectory, target: usize, t: f64) -> (DVec3, DVec3) {
    let (p, v) = path.sample(t);
    let (bp, bv) = ep.body_state(BodyId(target), t);
    (p - bp, v - bv)
}
fn closest(
    ep: &dyn EphemerisSource,
    path: &Trajectory,
    target: usize,
    from: f64,
) -> (f64, f64, f64) {
    let mut best = (from, f64::INFINITY, 0.0);
    // Each adaptive integration interval is searched independently; no one-ellipse assumption.
    for i in 1..path.count() {
        let lo = path.time(i - 1).max(from);
        let hi = path.time(i);
        if lo > hi {
            continue;
        }
        let mut a = lo;
        let mut b = hi;
        for _ in 0..18 {
            let l = a + (b - a) / 3.0;
            let r = b - (b - a) / 3.0;
            if relative(ep, path, target, l).0.length_squared()
                < relative(ep, path, target, r).0.length_squared()
            {
                b = r;
            } else {
                a = l;
            }
        }
        for t in [lo, hi, (a + b) * 0.5] {
            let (p, v) = relative(ep, path, target, t);
            if p.length() < best.1 {
                best = (t, p.length(), v.length());
            }
        }
    }
    best
}
fn solution(
    ep: &dyn EphemerisSource,
    plan: &FlightPlan,
    target: usize,
    m: ManeuverSpec,
    captured: bool,
) -> NavigationSolution {
    let (t, d, v) = closest(ep, &plan.trajectory, target, m.start_time);
    NavigationSolution {
        maneuver: m,
        delta_v_mps: DVec3::new(m.prograde, m.normal, m.radial).length(),
        closest_time: t,
        closest_distance_m: d,
        periapsis_altitude_m: d - ep.bodies()[target].radius_meters,
        relative_speed_mps: v,
        verified_until: plan.computed_until(),
        captured,
    }
}

/// Universal-variable, zero-revolution Lambert guess; short and long-way branches are both
/// considered by the search. This is an independent mathematical implementation.
fn lambert(r1: DVec3, r2: DVec3, dt: f64, gm: f64, long: bool) -> Option<DVec3> {
    let a = r1.length();
    let b = r2.length();
    let cos = (r1.dot(r2) / (a * b)).clamp(-1.0, 1.0);
    let sine = (1.0 - cos * cos).sqrt() * if long { -1.0 } else { 1.0 };
    let k = sine * (a * b / (1.0 - cos)).sqrt();
    if !k.is_finite() || k.abs() < 1e-9 {
        return None;
    }
    let time = |z: f64| -> Option<(f64, f64)> {
        let (c, s) = stumpff(z);
        let y = a + b + k * (z * s - 1.0) / c.sqrt();
        if c <= 0.0 || y < 0.0 {
            return None;
        }
        Some((((y / c).powf(1.5) * s + k * y.sqrt()) / gm.sqrt(), y))
    };
    // Scan instead of assuming the negative-z endpoint has a legal y.
    let mut previous = None;
    let mut bracket = None;
    for i in 0..=256 {
        let z = -16.0 + 55.0 * i as f64 / 256.0;
        if let Some((t, _)) = time(z) {
            if let Some((pz, pt)) = previous
                && pt <= dt
                && t >= dt
            {
                bracket = Some((pz, z));
                break;
            }
            previous = Some((z, t));
        }
    }
    let (mut lo, mut hi) = bracket?;
    for _ in 0..64 {
        let z = (lo + hi) * 0.5;
        if time(z)?.0 < dt {
            lo = z;
        } else {
            hi = z;
        }
    }
    let (_, y) = time((lo + hi) * 0.5)?;
    let f = 1.0 - y / a;
    let g = k * (y / gm).sqrt();
    Some((r2 - f * r1) / g)
}
fn stumpff(z: f64) -> (f64, f64) {
    if z.abs() < 1e-6 {
        (
            0.5 - z / 24.0 + z * z / 720.0,
            1.0 / 6.0 - z / 120.0 + z * z / 5040.0,
        )
    } else if z > 0.0 {
        let q = z.sqrt();
        ((1.0 - q.cos()) / z, (q - q.sin()) / (q * q * q))
    } else {
        let q = (-z).sqrt();
        ((q.cosh() - 1.0) / (-z), (q.sinh() - q) / (q * q * q))
    }
}

pub fn solve_navigation(
    ep: &mut dyn EphemerisSource,
    anchor: &PropagationRun,
    engine: PlanEngine,
    tol: Tolerances,
    req: &NavigationRequest,
) -> Result<NavigationSolution, NavigationError> {
    assert!(
        anchor.impact.is_none(),
        "navigation anchor cannot be an impact"
    );
    let n = ep.bodies().len();
    if req.target_body >= n || req.reference_body >= n {
        return Err(NavigationError::InvalidRequest(
            "target/reference body is absent".into(),
        ));
    }
    for v in [
        req.earliest_departure,
        req.latest_departure,
        req.min_flight_seconds,
        req.max_flight_seconds,
        req.periapsis_altitude_m,
    ] {
        assert!(v.is_finite(), "non-finite navigation request");
    }
    if req.earliest_departure < anchor.time
        || req.latest_departure < req.earliest_departure
        || req.min_flight_seconds <= 0.0
        || req.max_flight_seconds < req.min_flight_seconds
        || req.periapsis_altitude_m < 0.0
    {
        return Err(NavigationError::InvalidRequest(
            "invalid departure/arrival search window or target altitude".into(),
        ));
    }
    if ep.system_of(req.target_body) != ep.system_of(req.reference_body) {
        return Err(NavigationError::InvalidRequest(
            "target and departure reference must belong to the same system".into(),
        ));
    }
    if req.operation == NavigationOperation::Capture {
        return capture(ep, anchor, engine, tol, req);
    }
    if req.target_body == req.reference_body {
        return Err(NavigationError::InvalidRequest(
            "departure/correction target must differ from maneuver reference".into(),
        ));
    }
    let parent = ep.bodies()[req.target_body].parent_index.ok_or_else(|| {
        NavigationError::NoSolution("transfers to the system's central star are unsupported".into())
    })?;
    let gm = ep.bodies()[parent].gm;
    let target_radius = ep.bodies()[req.target_body].radius_meters + req.periapsis_altitude_m;
    let correction_arrival = if req.operation == NavigationOperation::Correction {
        let mut run = anchor.restarted();
        let mut path = Trajectory::new();
        path.append(run.time, &run.y);
        let end = req.earliest_departure + req.max_flight_seconds;
        let outcome = VesselPropagator::new(ep, tol).advance(
            ep,
            &mut run,
            end,
            STEP_BUDGET,
            Some(&mut path),
            None,
        );
        if outcome == AdvanceOutcome::Budget {
            return Err(NavigationError::PredictionBudget);
        }
        if path.last_time() <= req.earliest_departure {
            return Err(NavigationError::NoSolution(
                "no future approach before impact".into(),
            ));
        }
        let (t, d, _) = closest(ep, &path, req.target_body, req.earliest_departure);
        let soi = ep.bodies()[req.target_body]
            .sphere_of_influence_meters
            .ok_or_else(|| {
                NavigationError::NoSolution("target has no defined sphere of influence".into())
            })?;
        if t <= req.earliest_departure || t >= path.last_time() || d > soi * 10.0 {
            return Err(NavigationError::NoSolution(
                "no predicted approach suitable for a course correction".into(),
            ));
        }
        Some(t)
    } else {
        None
    };
    let mut guesses = Vec::new();
    let mut last_error =
        NavigationError::NoSolution("no Lambert transfer fits the search window".into());
    ep.extend_to(anchor.time);
    let (reference_position, _) = ep.body_state(BodyId(req.reference_body), anchor.time);
    let parking_period = std::f64::consts::TAU
        * ((anchor.state().position - reference_position)
            .length()
            .powi(3)
            / ep.bodies()[req.reference_body].gm)
            .sqrt();
    let mut departure_times = Vec::new();
    for window in 0..=8 {
        let base = req.earliest_departure
            + (req.latest_departure - req.earliest_departure) * window as f64 / 8.0;
        for phase in 0..8 {
            let t = base + parking_period * phase as f64 / 8.0;
            if t <= req.latest_departure {
                departure_times.push(t);
            }
        }
    }
    departure_times.sort_by(f64::total_cmp);
    departure_times.dedup();
    let mut departure_run = anchor.restarted();
    let mut departure_propagator = VesselPropagator::new(ep, tol);
    for t in departure_times {
        match departure_propagator.advance(
            ep,
            &mut departure_run,
            t,
            WINDOW_COAST_BUDGET,
            None,
            None,
        ) {
            AdvanceOutcome::Reached => {}
            AdvanceOutcome::Budget => {
                last_error = NavigationError::PredictionBudget;
                break;
            }
            AdvanceOutcome::Impact { .. } => {
                let i = departure_run.impact.expect("impact outcome");
                last_error = NavigationError::Impact {
                    body: i.body,
                    time: i.time,
                };
                break;
            }
        }
        let state = departure_run.state();
        let basis = axes(ep, state, req.reference_body)?;
        if let Some(arrival) = correction_arrival
            && arrival - t >= req.min_flight_seconds
        {
            guesses.push((
                0.0,
                spec(t, req.reference_body, DVec3::ZERO),
                arrival,
                target_radius,
                state,
            ));
        }

        ep.extend_to(t + req.max_flight_seconds);
        let (cp, cv) = ep.body_state(BodyId(parent), t);
        let (rp, rv) = ep.body_state(BodyId(req.reference_body), t);
        let parking = parent != req.reference_body
            && ep.bodies()[req.reference_body].parent_index == Some(parent);
        let origin = if parking {
            rp - cp
        } else {
            state.position - cp
        };
        for flight in 0..=4 {
            let dt = req.min_flight_seconds
                + (req.max_flight_seconds - req.min_flight_seconds) * flight as f64 / 4.0;
            let arrival = correction_arrival.unwrap_or(t + dt);
            let dt = arrival - t;
            if dt < req.min_flight_seconds {
                continue;
            }
            let (tp, _) = ep.body_state(BodyId(req.target_body), arrival);
            let (pc, _) = ep.body_state(BodyId(parent), arrival);
            for long in [false, true] {
                let Some(v) = lambert(
                    origin,
                    tp - pc + (tp - pc).normalize() * target_radius,
                    dt,
                    gm,
                    long,
                ) else {
                    continue;
                };
                let inertial = if parking {
                    let excess = v + cv - rv;
                    let r = (state.position - rp).length();
                    let speed = (excess.length_squared()
                        + 2.0 * ep.bodies()[req.reference_body].gm / r)
                        .sqrt();
                    // Tangential parking-orbit departure with the excess sign. Subsequent N-body
                    // shooting solves the departure phase and direction, rather than claiming this
                    // patched-conic guess already reaches the target.
                    let tangent = basis.x_axis;
                    let sign = if tangent.dot(excess) >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    };
                    tangent * speed * sign - (state.velocity - rv)
                } else {
                    v + cv - state.velocity
                };
                let dv = basis.transpose() * inertial;
                let maxdv = engine.exhaust_velocity * (state.mass_kg / engine.dry_mass_kg).ln();
                if dv.length() > maxdv {
                    last_error = NavigationError::InsufficientFuel;
                    continue;
                }
                guesses.push((
                    dv.length(),
                    spec(t, req.reference_body, dv),
                    arrival,
                    target_radius,
                    state,
                ));
            }
        }
    }
    guesses.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<NavigationSolution> = None;
    for (_, mut m, arrival, alt, departure_state) in guesses.into_iter().take(10) {
        let trial_anchor = PropagationRun::new(departure_state);
        for _ in 0..5 {
            let end = arrival + 0.05 * (arrival - m.start_time);
            let plan = match verify(ep, &trial_anchor, engine, tol, m, end) {
                Ok(p) => p,
                Err(e) => {
                    last_error = e;
                    break;
                }
            };
            let s = solution(ep, &plan, req.target_body, m, false);
            let soi = ep.bodies()[req.target_body]
                .sphere_of_influence_meters
                .ok_or_else(|| {
                    NavigationError::NoSolution("target has no defined sphere of influence".into())
                })?;
            if s.closest_distance_m <= soi
                && s.periapsis_altitude_m >= 0.0
                && best
                    .as_ref()
                    .is_none_or(|b| s.closest_distance_m < b.closest_distance_m)
            {
                best = Some(s);
            }
            let (r, v) = relative(ep, &plan.trajectory, req.target_body, arrival);
            let Some(normal) = r.cross(v).try_normalize() else {
                last_error = NavigationError::NoSolution(
                    "candidate approach has no defined encounter plane".into(),
                );
                break;
            };
            let Some(offset_direction) = v.cross(normal).try_normalize() else {
                last_error = NavigationError::NoSolution(
                    "candidate approach has no defined transverse direction".into(),
                );
                break;
            };
            let offset = offset_direction * alt;
            let residual = r - offset;
            if residual.length() < alt * 0.05 {
                break;
            }
            let components = DVec3::new(m.prograde, m.normal, m.radial);
            let epsilon = (components.length() * 0.001).clamp(0.5, 20.0);
            let mut cols = [DVec3::ZERO; 3];
            let mut good = true;
            for j in 0..3 {
                let mut d = components;
                d[j] += epsilon;
                match verify(
                    ep,
                    &trial_anchor,
                    engine,
                    tol,
                    spec(m.start_time, m.reference_body, d),
                    end,
                ) {
                    Ok(p) => {
                        cols[j] =
                            (relative(ep, &p.trajectory, req.target_body, arrival).0 - r) / epsilon
                    }
                    Err(e) => {
                        last_error = e;
                        good = false;
                        break;
                    }
                }
            }
            if !good {
                break;
            }
            let matrix = DMat3::from_cols(cols[0], cols[1], cols[2]);
            if matrix.determinant().abs() < 1e-9 {
                break;
            }
            let correction = matrix.inverse() * residual;
            let capped = correction.clamp_length_max(components.length().max(100.0) * 0.5);
            m = spec(m.start_time, m.reference_body, components - capped);
        }
    }
    let best = best.ok_or(last_error)?;
    // The coarse window sweep and trial starts partition the pre-burn coast differently.
    // Recompute the chosen schedule from the ORIGINAL anchor before publishing metrics;
    // the user's ordinary flight plan must predict precisely the same schedule.
    let full = verify_budget(
        ep,
        anchor,
        engine,
        tol,
        best.maneuver,
        best.verified_until,
        WINDOW_COAST_BUDGET,
    )?;
    let verified = solution(ep, &full, req.target_body, best.maneuver, false);
    let soi = ep.bodies()[req.target_body]
        .sphere_of_influence_meters
        .expect("candidate target has an SOI");
    if verified.closest_distance_m > soi {
        return Err(NavigationError::NoSolution(
            "full finite-burn prediction misses the target sphere of influence".into(),
        ));
    }
    Ok(verified)
}

fn capture(
    ep: &mut dyn EphemerisSource,
    anchor: &PropagationRun,
    engine: PlanEngine,
    tol: Tolerances,
    req: &NavigationRequest,
) -> Result<NavigationSolution, NavigationError> {
    let mut run = anchor.restarted();
    let mut path = Trajectory::new();
    path.append(run.time, &run.y);
    let end = req.latest_departure + req.max_flight_seconds;
    let body = &ep.bodies()[req.target_body];
    let (gm, radius, soi) = (body.gm, body.radius_meters, body.sphere_of_influence_meters);
    let mut propagator = VesselPropagator::new(ep, tol);
    let mut selected = None;
    while run.time < end && propagator.accepted_steps < STEP_BUDGET {
        ep.extend_to(run.time);
        let (bp, _) = ep.body_state(BodyId(req.target_body), run.time);
        let distance = (run.state().position - bp).length();
        // Search only a fraction of the local dynamical time at once. Nearby parking
        // orbits expose their next periapsis promptly; distant approaches still use the
        // requested horizon and the same total accepted-step budget.
        let leg = (std::f64::consts::TAU * (distance.powi(3) / gm).sqrt() / 8.0)
            .min((end - anchor.time) / 32.0)
            .max(1.0);
        let from = run.time.max(req.earliest_departure);
        let left = (STEP_BUDGET - propagator.accepted_steps).min(5000);
        let until = (run.time + leg).min(end);
        let outcome = propagator.advance(ep, &mut run, until, left, Some(&mut path), None);
        for apsis in crate::find_apsides(&path, ep, req.target_body, from, 32) {
            if apsis.kind != crate::ApsisKind::Periapsis
                || apsis.distance_meters <= radius
                || soi.is_some_and(|s| apsis.distance_meters > s)
            {
                continue;
            }
            let (p, v) = relative(ep, &path, req.target_body, apsis.time);
            let radial = p.normalize();
            let Some(tangent) = (v - radial * v.dot(radial)).try_normalize() else {
                continue;
            };
            let dv = (tangent * (gm / apsis.distance_meters).sqrt() - v).length();
            let mass = anchor.state().mass_kg;
            let duration =
                mass * (1.0 - (-dv / engine.exhaust_velocity).exp()) * engine.exhaust_velocity
                    / engine.thrust_newtons;
            if apsis.time - duration * 0.5 >= req.earliest_departure {
                selected = Some((apsis.time, apsis.distance_meters));
                break;
            }
        }
        if selected.is_some() || matches!(outcome, AdvanceOutcome::Impact { .. }) {
            break;
        }
    }
    let (t,d)=match selected {
        Some(a)=>a,
        None if propagator.accepted_steps>=STEP_BUDGET=>return Err(NavigationError::PredictionBudget),
        None=>return Err(NavigationError::CaptureUnavailable("no safe future target periapsis with enough time to centre the capture burn was predicted".into())),
    };
    let (p, v) = path.sample(t);
    let (bp, bv) = ep.body_state(BodyId(req.target_body), t);
    let r = p - bp;
    let rel = v - bv;
    let tangent = (rel - r.normalize() * rel.dot(r.normalize()))
        .try_normalize()
        .ok_or_else(|| {
            NavigationError::CaptureUnavailable(
                "radial collision trajectory has no capture plane".into(),
            )
        })?;
    let desired = tangent * (gm / d).sqrt();
    let dv = desired - rel;
    let mass = anchor.state().mass_kg;
    let duration =
        mass * (1.0 - (-dv.length() / engine.exhaust_velocity).exp()) * engine.exhaust_velocity
            / engine.thrust_newtons;
    let start = t - duration * 0.5;
    if start < req.earliest_departure {
        return Err(NavigationError::CaptureUnavailable(
            "capture burn cannot start before the permitted departure time".into(),
        ));
    }
    let state = coast(ep, anchor, start, tol)?;
    let basis = axes(ep, state, req.target_body)?;
    let m = spec(start, req.target_body, basis.transpose() * dv);
    let period = std::f64::consts::TAU * (d.powi(3) / gm).sqrt();
    let verify_end = (t + period).max(start + duration + 1.0);
    let plan = verify(ep, anchor, engine, tol, m, verify_end)?;
    let (p, v) = relative(ep, &plan.trajectory, req.target_body, verify_end);
    if v.length_squared() * 0.5 - gm / p.length() >= 0.0 || soi.is_some_and(|s| p.length() > s) {
        return Err(NavigationError::CaptureUnavailable(
            "finite-thrust prediction does not produce a bound target orbit".into(),
        ));
    }
    Ok(solution(ep, &plan, req.target_body, m, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ephemeris, EphemerisOptions, GRAVITATIONAL_CONSTANT, SystemSpec, build_system};
    use void_frames::BodyStates;
    fn fixture() -> Ephemeris {
        let rotation = serde_json::json!({"periodSeconds":1000.,"obliquityRadians":0.,"poleLongitudeRadians":0.,"angleAtEpochRadians":0.});
        let orbit = |a: f64, phase: f64| serde_json::json!({"semiMajorAxisMeters":a,"eccentricity":0.,"inclinationRadians":0.,"longitudeOfAscendingNodeRadians":0.,"argumentOfPeriapsisRadians":0.,"meanAnomalyRadians":phase});
        let spec = serde_json::json!({"name":"Navigation fixture","root":{"id":"star","name":"Star","massKg":1e12/GRAVITATIONAL_CONSTANT,"radiusMeters":100.,"color":"#ffffff","rotation":rotation,"children":[
          {"id":"planet","name":"Planet","massKg":1e8/GRAVITATIONAL_CONSTANT,"radiusMeters":100.,"color":"#ffffff","rotation":rotation,"orbit":orbit(1e6,0.),"orbitPlane":"ecliptic","children":[
            {"id":"moon","name":"Moon","massKg":1e5/GRAVITATIONAL_CONSTANT,"radiusMeters":20.,"color":"#ffffff","rotation":rotation,"orbit":orbit(10000.,1.),"orbitPlane":"ecliptic","children":[]}
          ]},
          {"id":"outer","name":"Outer","massKg":1e8/GRAVITATIONAL_CONSTANT,"radiusMeters":100.,"color":"#ffffff","rotation":rotation,"orbit":orbit(1.5e6,1.),"orbitPlane":"ecliptic","children":[]}
        ]}});
        let built = build_system(&SystemSpec::from_json(&spec.to_string()));
        let mut ep = Ephemeris::new(
            &built,
            EphemerisOptions {
                step_seconds: 2.,
                chunk_steps: 1024,
            },
        );
        ep.extend_to(2.);
        ep
    }
    fn engine() -> PlanEngine {
        PlanEngine {
            thrust_newtons: 1e6,
            exhaust_velocity: 10000.,
            dry_mass_kg: 100.,
        }
    }
    fn tol() -> Tolerances {
        Tolerances {
            position_meters: 0.02,
            velocity_meters_per_second: 0.001,
        }
    }
    fn anchor(ep: &Ephemeris, center: usize, r: f64) -> PropagationRun {
        let (p, v) = ep.body_state(BodyId(center), 0.);
        PropagationRun::new(VesselState {
            time: 0.,
            position: p + DVec3::X * r,
            velocity: v + DVec3::Y * (ep.bodies()[center].gm / r).sqrt(),
            mass_kg: 1000.,
        })
    }
    #[test]
    fn lambert_reconstructs_circular_quarter_orbit() {
        let gm: f64 = 1e8;
        let radius = 10000.;
        let dt = std::f64::consts::FRAC_PI_2 * (radius * radius * radius / gm).sqrt();
        let v = lambert(DVec3::X * radius, DVec3::Y * radius, dt, gm, false).unwrap();
        assert!((v - DVec3::Y * 100.).length() < 1e-6);
    }
    #[test]
    fn moon_departure_is_checked_without_consuming_anchor() {
        let mut ep = fixture();
        let from = anchor(&ep, 1, 1000.);
        let original = from.state();
        let req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: 2,
            reference_body: 1,
            earliest_departure: 0.,
            latest_departure: 500.,
            min_flight_seconds: 500.,
            max_flight_seconds: 2500.,
            periapsis_altitude_m: 200.,
        };
        let result = solve_navigation(&mut ep, &from, engine(), tol(), &req);

        let s = result.unwrap();
        assert!(s.closest_distance_m < ep.bodies()[2].sphere_of_influence_meters.unwrap());
        assert!(s.periapsis_altitude_m >= 0.);
        assert_eq!(original, from.state());
        let plan = verify(
            &mut ep,
            &from,
            engine(),
            tol(),
            s.maneuver,
            s.verified_until,
        )
        .unwrap();
        let independently = solution(&ep, &plan, 2, s.maneuver, false);
        assert!((independently.closest_distance_m - s.closest_distance_m).abs() < 0.01);
    }
    #[test]
    fn interplanetary_departure_is_verified() {
        let mut ep = fixture();
        let from = anchor(&ep, 0, 1.01e6);
        let req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: 3,
            reference_body: 0,
            earliest_departure: 0.,
            latest_departure: 2500.,
            min_flight_seconds: 2500.,
            max_flight_seconds: 7500.,
            periapsis_altitude_m: 500.,
        };
        let s = solve_navigation(&mut ep, &from, engine(), tol(), &req).unwrap();
        assert!(s.closest_distance_m < ep.bodies()[3].sphere_of_influence_meters.unwrap());
        assert!(s.periapsis_altitude_m >= 0.);
    }
    #[test]
    fn course_correction_recomputes_from_a_real_approach() {
        let mut ep = fixture();
        let from = anchor(&ep, 1, 1000.);
        let mut req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: 2,
            reference_body: 1,
            earliest_departure: 0.,
            latest_departure: 500.,
            min_flight_seconds: 500.,
            max_flight_seconds: 2500.,
            periapsis_altitude_m: 200.,
        };
        let departure = solve_navigation(&mut ep, &from, engine(), tol(), &req).unwrap();
        let plan = verify(
            &mut ep,
            &from,
            engine(),
            tol(),
            departure.maneuver,
            departure.verified_until,
        )
        .unwrap();
        let time = departure.maneuver.start_time
            + (departure.closest_time - departure.maneuver.start_time) * 0.5;
        let (position, velocity) = plan.trajectory.sample(time);
        let current = PropagationRun::new(VesselState {
            time,
            position,
            velocity,
            mass_kg: plan.burns()[0].mass_after_kg,
        });
        req.operation = NavigationOperation::Correction;
        req.earliest_departure = time;
        req.latest_departure = time + 20.;
        req.min_flight_seconds = 10.;
        req.max_flight_seconds = departure.closest_time - time + 100.;
        let correction = solve_navigation(&mut ep, &current, engine(), tol(), &req).unwrap();
        assert!(correction.delta_v_mps < departure.delta_v_mps);
        assert!(
            correction.closest_distance_m <= ep.bodies()[2].sphere_of_influence_meters.unwrap()
        );
    }
    #[test]
    fn interplanetary_parking_orbit_ejection_is_verified() {
        let mut ep = fixture();
        let from = anchor(&ep, 1, 1000.);
        let req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: 3,
            reference_body: 1,
            earliest_departure: 0.,
            latest_departure: 2000.,
            min_flight_seconds: 2500.,
            max_flight_seconds: 7500.,
            periapsis_altitude_m: 500.,
        };
        let result = solve_navigation(&mut ep, &from, engine(), tol(), &req);

        let s = result.unwrap();
        assert!(s.closest_distance_m < ep.bodies()[3].sphere_of_influence_meters.unwrap());
    }
    #[test]
    fn capture_requires_and_verifies_a_safe_bound_orbit() {
        let mut ep = fixture();
        let (p, v) = ep.body_state(BodyId(1), 0.);
        let gm = ep.bodies()[1].gm;
        // Begin at apoapsis of a 1000 x 3000 m orbit; circularize at the upcoming periapsis.
        let from = PropagationRun::new(VesselState {
            time: 0.,
            position: p + DVec3::X * 3000.,
            velocity: v + DVec3::Y * (gm * (2. / 3000. - 1. / 2000.)).sqrt(),
            mass_kg: 1000.,
        });
        let req = NavigationRequest {
            operation: NavigationOperation::Capture,
            target_body: 1,
            reference_body: 1,
            earliest_departure: 0.,
            latest_departure: 0.,
            min_flight_seconds: 1.,
            max_flight_seconds: 70.,
            periapsis_altitude_m: 200.,
        };
        let s = solve_navigation(&mut ep, &from, engine(), tol(), &req).unwrap();
        assert!(s.captured);
        assert_eq!(s.maneuver.reference_body, 1);
    }
    #[test]
    #[ignore = "real-scale transfer witness; run explicitly rather than in every core test"]
    fn real_sol_parking_departures() {
        let built = build_system(&SystemSpec::from_json(include_str!("../systems/sol.json")));
        let mut ep = Ephemeris::new(
            &built,
            EphemerisOptions {
                step_seconds: 120.,
                chunk_steps: 1024,
            },
        );
        ep.extend_to(120.);
        let home = ep.bodies().iter().position(|b| b.id == "aurelia").unwrap();
        let selected =
            std::env::var("VOID_NAVIGATION_WITNESS_TARGET").unwrap_or_else(|_| "selene".into());
        let target = ep.bodies().iter().position(|b| b.id == selected).unwrap();
        let from = anchor(&ep, home, ep.bodies()[home].radius_meters + 400000.);
        let day = 86400.;
        let (wait, min, max) = if selected == "selene" {
            (28. * day, 2. * day, 6. * day)
        } else {
            (780. * day, 120. * day, 360. * day)
        };
        let req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: target,
            reference_body: home,
            earliest_departure: from.time,
            latest_departure: wait,
            min_flight_seconds: min,
            max_flight_seconds: max,
            periapsis_altitude_m: 100000.,
        };
        let actual_engine = PlanEngine {
            thrust_newtons: 250000.,
            exhaust_velocity: 350. * 9.80665,
            dry_mass_kg: 10000.,
        };
        let mut state = from.state();
        state.mass_kg = 40000.;
        let from = PropagationRun::new(state);
        let result = solve_navigation(
            &mut ep,
            &from,
            actual_engine,
            Tolerances {
                position_meters: 1.,
                velocity_meters_per_second: 0.01,
            },
            &req,
        );
        eprintln!("real Sol {selected}: {result:?}");
        let s = result.unwrap();
        assert!(s.closest_distance_m < ep.bodies()[target].sphere_of_influence_meters.unwrap());
        assert!(s.periapsis_altitude_m >= 0.);
    }
    #[test]
    fn expanded_parking_capture_stops_at_first_safe_periapsis() {
        let built = build_system(&crate::expanded_sol());
        let mut ep = Ephemeris::new(
            &built,
            EphemerisOptions {
                step_seconds: 10.,
                chunk_steps: 1024,
            },
        );
        ep.extend_to(10.);
        let home = ep.bodies().iter().position(|b| b.id == "aurelia").unwrap();
        let mut initial = anchor(&ep, home, ep.bodies()[home].radius_meters + 400000.).state();
        initial.mass_kg = 40000.;
        let from = PropagationRun::new(initial);
        let req = NavigationRequest {
            operation: NavigationOperation::Capture,
            target_body: home,
            reference_body: home,
            earliest_departure: 38.2833333333,
            latest_departure: 30. * 86400.,
            min_flight_seconds: 100.,
            max_flight_seconds: 7. * 86400.,
            periapsis_altitude_m: 100000.,
        };
        let engine = PlanEngine {
            thrust_newtons: 250000.,
            exhaust_velocity: 350. * 9.80665,
            dry_mass_kg: 10000.,
        };
        let tolerances = Tolerances {
            position_meters: 1e-6,
            velocity_meters_per_second: 1e-9,
        };
        let result = solve_navigation(&mut ep, &from, engine, tolerances, &req).unwrap();
        assert!(result.captured);
        assert!(result.maneuver.start_time < 12000.);
        assert!(
            result.verified_until < 18000.,
            "capture should not integrate the unused 37-day search window"
        );
        assert_eq!(initial, from.state());
    }
    #[test]
    fn errors_are_explicit_for_fuel_and_no_approach() {
        let mut ep = fixture();
        let from = anchor(&ep, 1, 1000.);
        let mut req = NavigationRequest {
            operation: NavigationOperation::Departure,
            target_body: 2,
            reference_body: 1,
            earliest_departure: 0.,
            latest_departure: 500.,
            min_flight_seconds: 500.,
            max_flight_seconds: 2500.,
            periapsis_altitude_m: 200.,
        };
        let mut low = engine();
        low.dry_mass_kg = 999.99;
        assert_eq!(
            solve_navigation(&mut ep, &from, low, tol(), &req).unwrap_err(),
            NavigationError::InsufficientFuel
        );
        req.operation = NavigationOperation::Correction;
        req.max_flight_seconds = 100.;
        req.min_flight_seconds = 10.;
        assert!(matches!(
            solve_navigation(&mut ep, &from, engine(), tol(), &req),
            Err(NavigationError::NoSolution(_))
        ));
    }
}

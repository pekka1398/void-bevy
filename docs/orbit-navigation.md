# Orbit navigation implementation and acceptance

Worktree: `/home/pekka/Desktop/void-bevy-navigation`; branch `work/orbit-navigation`.
Base: expanded bodies `3dcfa94`, not merged to master. Specification:
`docs/specs/orbit-navigation-and-plotting.md`.

## Controls

The ORBIT panel selects System / Inertial / Surface / Pair. Centre > selects the
plotting body, Partner > selects the second body in the same system. Keyboard
1–4 select modes, G cycles, J advances centre, Shift+J advances partner. The
readout names the reference plane. These controls preserve camera focus and
navigation target.

The MANEUVER panel contains NAVIGATION. Select Target < / > independently of
camera focus, or explicitly choose Use focus. Wait >, Flight > and Pe > cycle
shown search limits and desired periapsis altitude. Depart / Correct / Capture
each request one node and do not execute it. Generated nodes append after the
existing executable burns, from their predicted final state. Inspect the plan
message and actual finite-burn predicted trajectory before Execute; all existing
manual edit, warp and abort controls remain available.

Coast crossings and plan crossings show AN/DN, time until crossing, signed plane
crossing speed and apparent inclination when defined. Plan crossings have a
"Plan" prefix. Clicking edits the selected maneuver, or adds an editable zero-dv
node if there is no node. Existing burns are approximately centred using their
current duration; editing the burn changes its duration and prediction, so
inspect again. Eight crossings per path are displayed to keep the map readable.

## Model and limits

Plotting uses per-sample frame transforms. Body inertial/surface planes are
already equatorial in void-frames. Pair plane precession is included in plotting
velocity by differencing frame directions over 0.01 s (one-sided at ephemeris
endpoints); physical frame-tree propagation is unchanged. Nodes use sign brackets
and bisection on the existing interpolated numerical trajectory. Coplanarity and
tangencies do not create artificial pairs. No optional distant-equator cutoff is
currently imposed.

Navigation uses bounded two-body/Lambert guesses, finite-burn N-body validation
and shooting refinement. It reports actual predicted closest approach, not the
requested altitude as a guaranteed result. Capture checks a safe predicted
periapsis and a bound target-relative orbit in the resulting prediction. Search
can explicitly fail for fuel, collision, prediction budget, missing approach or
unsupported geometry. Solver quality/performance and long-term bound-orbit
stability are not guaranteed by a successful short prediction.

Model version 33 rejects older model saves and recordings explicitly. This
work introduces no automatic conversion of existing save data.

## Verification record

Implementation remains in progress. Targeted core/view/fleet tests have been
run; final app lint/build, GUI inspection and requirement audit remain pending.
Human acceptance has not occurred. Do not mark this document or its specification
complete based only on the current tests.

### Navigation core evidence (2026-10-09 working tree)

`cargo test -p void-orbit --lib navigation -j 2` passes seven scoped tests;
its additional real-scale witness is deliberately ignored in the default run.
Coverage includes planet parking orbit to moon, parking-orbit ejection to another
planet, a heliocentric transfer, correction from an actual predicted approach,
periapsis capture, insufficient fuel and missing approach. The source anchor is
unchanged. Independent reconstruction from the original anchor reproduces the
published closest distance within the test's original 0.01 m threshold. The
solver's final winner is re-integrated from that original anchor, because using
only the differently partitioned window-search coast can change encounter metrics.
`cargo clippy -p void-orbit --lib --tests -j 2 -- -D warnings` passed.

Explicit real-scale witnesses use committed **15-body `sol.json`**, not the main
58-body expanded scenery, at 120 s massive-body steps. The vessel begins in a
400 km Aurelia parking orbit, with 40 t total mass / 10 t dry mass, 250 kN thrust
and 350 s specific impulse (the existing orbit simulation fixture's engine).
These are headless solver witnesses, not main-game GUI or human acceptance.

- `cargo test -p void-orbit --lib real_sol_parking_departures -j 2 -- --ignored --nocapture`
  checks Aurelia → Selene; wait 28 days, flight 2–6 days, desired altitude 100 km.
  Result: ignition T+1386.210 s; Δv 3689.684 m/s; predicted closest centre distance
  20,080.750 km / altitude 18,343.350 km; relative speed 1817.881 m/s at
  T+182826.210 s. This minimum is at the prediction endpoint, so it is a closest
  point **within the covered interval**, not an asserted completed flyby or 100 km
  periapsis. It is inside Selene's model sphere of influence.
- `VOID_NAVIGATION_WITNESS_TARGET=ares cargo test -p void-orbit --lib real_sol_parking_departures -j 2 -- --ignored --nocapture`
  checks Aurelia → Ares; wait 780 days, flight 120–360 days, desired altitude 100 km.
  Result: ignition T+42122772.420 s; Δv 3601.116 m/s; closest centre distance
  439,787.897 km / altitude 436,398.397 km at T+66073057.031 s; relative speed
  4954.244 m/s; prediction extends to T+69338772.420 s. The encounter is inside
  Ares's model sphere of influence; this remains a rough approach, not capture.

The bounded search samples both macro departure windows and local parking-orbit
phase. It uses one sequential ordinary N-body parking coast, then finite-thrust
candidate trials, and a final original-anchor prediction. Integration exhaustion
is reported rather than switching to rails or an impulsive execution model.
Neither witness establishes expanded-catalog performance or long-term orbital
stability, and neither guarantees that the desired periapsis height is reached.

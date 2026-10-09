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

Implementation remains in progress. Targeted core/view/fleet and app integration
tests, app clippy and the app build passed on implementation commit `260dc15`.
TigerVNC inspection confirmed frame controls, a visible AN marker and separated
panels. It exposed a capture-search budget refusal in the expanded main game;
that issue was corrected. A fresh GUI check also verifies immediate paused plan
preview and appending without consuming live fuel. Final coordinate audit and scoped regressions passed. Human acceptance remains
pending.
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

### Root integration checks

- `cargo test -p void-orbit --lib navigation::tests -j 2`: seven pass, real-scale witness excluded from this command (run separately above).
- `cargo test -p void-orbit --test nodes -j 2`: five pass.
- `cargo test -p void-fleet-flight --test plans -j 2`: five pass.
- `cargo test -p void-app --lib navigation_ui_tests -j 2`: crossing placement, four frame controls, unchanged focus/vessel/fuel and journal replay pass.
- `cargo test -p void-app --lib edited_maneuver_value_reaches_the_live_plan_and_replays -j 2`: existing edit path passes.
- `cargo clippy -p void-app --lib --tests -j 2 -- -D warnings`: pass.
- `cargo build -p void-app --bin void-app -j 2`: pass. Branch-local acceptance executable is checksum-verified by `tools/navigation-acceptance.sh`.
- Agent GUI evidence: ignored `lab-log/navigation/final-pair.png`, `detached-capture.png`, `capture-result.png`; journal `gui-2.json` recorded a Capture rejection with no node and no fuel consumed. These screenshots do not constitute human acceptance.

No full workspace suite was run. Master and other worktrees' existing uncommitted
changes were not included. The navigation worktree's copied `AGENTS.md` preference
remains unstaged; implementation and spec are committed separately.

Capture GUI follow-up: a wide 7-day horizon in a tight parking orbit originally
spent the entire 120,000-step search budget before inspecting any periapsis.
Capture now advances the ordinary numerical coast in chunks tied to local
orbital time, examines actual bracketed periapses, and stops at the first safe
one that leaves sufficient lead time to centre its burn. Distant approaches
retain the requested horizon and the same total search-step budget. Verification
still covers approximately one resulting orbit; no rails/impulse substitute or
larger capture search budget was introduced.

`expanded_parking_capture_stops_at_first_safe_periapsis` exercises all 58 expanded
bodies, Aurelia 400 km parking orbit and the fleet's actual 1e-6 m / 1e-9 m/s
integration tolerances. Earliest departure T+38.283333 s, wait limit 30 days,
flight limit 7 days, requested altitude 100 km; it verifies a capture node before
T+12000 s and completed bound-orbit prediction before T+18000 s, preserving the
source anchor. This uses the orbit fixture's engine, not a full demo-craft UI
session. Scoped navigation tests now pass eight cases; scoped fleet `plans`
passes five including append, unchanged mass, checkpoint and replay. Orbit
lib/tests clippy remains clean. Main-game GUI recheck remains the root agent's
separate evidence.

### Final capture and paused preview followup

The main 58-body demo-craft GUI successfully generated a Capture node at
T+2805.54 s (11.1 m/s), without ignition or live fuel use. A later check loaded
that plan, appended a second Capture node at T+5562.46 s (11.2 m/s), and displayed
the orange appended trajectory while paused. Evidence:
`lab-log/navigation/capture-fixed-result.png` and
`lab-log/navigation/final-plan-preview-2.png`, with request/Applied commits in
`lab-log/navigation/gui-final-3.json`. These are agent checks, not human approval.

The appended FlightPlan is now fully predicted before it is committed, so its
trajectory is reviewable even without advancing live time. The fleet plans test
asserts completion, trajectory samples and coverage beyond the last burn, and
retains unchanged-fuel, checkpoint and replay checks; all five tests pass. Final
app clippy and build pass after this integration fix.

A previous unrestricted build triggered systemd-oomd termination of the VS Code
scope on 2026-10-09 19:25:20, alongside NVMe write timeouts. Subsequent build ran
separately with -j 1 under a dedicated systemd scope (MemoryHigh 3G / MemoryMax
5G); GUI review uses a separate service (MemoryHigh 2G / MemoryMax 3G), with no
concurrent build. Do not treat the interrupted link as successful evidence; the
subsequent build completed successfully.

### Coordinate-source audit

Coast predictions now retain source system and split physics offset. App plotting
uses the matching source for coast, each vessel plan and the current scene; focus
is subtracted as a split position, followed by the small camera offset before
f32 conversion. FrameEvaluator applies the source offset once via the common
frame tree. PlotPath/BodyPlots signatures include source system and offset.

Memory-capped, single-worker checks pass: orbit nodes six cases; view plot three
cases; new plot_offset case for path/node/body placement and cache invalidation
when switching precision origins; orbit/view clippy; app clippy; app label
regressions two cases. This supplements the earlier navigation, append, fuel,
checkpoint and replay evidence. Coast source tags are transient prediction data;
save schema remains unchanged and model version remains 33.

| Requirement | Evidence |
| --- | --- |
| Departure, correction, capture, one node per operation | Eight core navigation cases, real Sol departure witnesses, main GUI capture/append journal |
| Preserve existing plan, no live fuel or automatic ignition | Fleet plans tests and paused GUI full-fuel readouts |
| Four plotting frames and unchanged physical state | View placement tests, core frame tests, app control/journal test and GUI pair selection |
| Numerical AN/DN and moving reference planes | Nodes tests incl multiple crossings/coplanar/tangent/precession, visible main GUI AN |
| Clickable crossing and finite-burn timing | App navigation_ui_tests, existing manual editing/replay regression |
| Source system/precision anchor consistency | Offset/source tests, explicit coast metadata, current-camera label regression |
| Reviewable paused plan, errors and metrics | Appended-plan prediction test, GUI orange preview and explicit earlier budget refusal/fix regression |
| Runnable main game and human acceptance | Checksum acceptance script; agent GUI checked; human acceptance pending |

Work is not merged or pushed. Human approval is not inferred from agent GUI tests.

Final source build passes under the isolated, memory-capped single-worker scope.
The two app label regressions pass after coordinate wiring changes. Actual GUI
journal replay also passes with that binary:
`void-app --verify lab-log/navigation/gui-final-3.json` reports T+38.483333 s,
two vessels and selected v2. No full workspace suite or human acceptance is
claimed. The ready-to-play checkpoint is
`lab-log/navigation/acceptance-save.json` (paused, two retained/appended nodes,
no automatic ignition), for explicit human review.

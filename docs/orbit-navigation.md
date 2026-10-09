# Orbit navigation implementation and acceptance

> **2026-10-10 交付撤回：整機失去回應。** 使用者回報啟動後滑鼠／鍵盤完全無回應，需強制關機。`tools/navigation-acceptance.sh` 已停用；以下啟動步驟及先前 agent 驗證不代表此版本可安全使用。根因尚未確認，不重新啟動遊戲或以使用者桌面重現。

Worktree: `/home/pekka/Desktop/void-bevy-navigation`; branch `work/orbit-navigation`.
Base: expanded bodies `3dcfa94`, not merged to master. Specification:
`docs/specs/orbit-navigation-and-plotting.md`.

## Start the repaired acceptance build

```sh
/home/pekka/Desktop/void-bevy-navigation/tools/navigation-acceptance.sh
```

With no arguments this opens a paused, authored Aurelia 400 km orbital fixture:
original main-game rocket with its booster detached, upper-stage engine staged,
full remaining upper-stage fuel, and Selene already selected. Click **Depart**.
There is no need to press O or Space. Generating a node never ignites the engine;
inspect its predicted trajectory and metrics before B / Execute. Cancel stops an
active search or preview preparation. P resumes time. This is a declared starting
fixture, not a completed launch or transfer.

Passing explicit arguments keeps the usual game CLI, for example
`tools/navigation-acceptance.sh --load <checkpoint> --navigation-target vesper`.
The unchanged full stack has less currently usable maneuver delta-v than the
separated upper stage; stage indication is relevant to navigation refusals.

To recreate the model-34 starting fixture from source:

```sh
VOID_NAVIGATION_SAVE_FIXTURE="$PWD/lab-log/navigation/departure-ready.json" \
  cargo test -p void-fleet-flight --test plans \
  expanded_main_upper_stage_departure_witness -j 1 -- --ignored
```

## Repair of the unresponsive Depart action (2026-10-10)

The previous delivery did not cover the user's Depart interaction. It must not
be treated as accepted: the request, reference query and complete finite-burn
prediction ran synchronously on the render thread.

The repaired UI owns one headless worker process using a direct world snapshot.
It displays elapsed search time, supports actual cancellation and imposes a
120-second wall-time limit. Worker exit and UI resource teardown reap the exact
owned child. Success returns a verified plan, never a replacement live world.
The app prepares the necessary future ephemeris in slices of 32 samples per frame
before publishing, so finishing the search cannot trigger another long synchronous
integration in the renderer. Preview preparation is also cancellable.

The acceptance command rechecks the physical baseline, selected vessel and plans.
Camera/pause/plot presentation changes are allowed; changed fuel, control, time,
vessels or plans reject the stale result. `AcceptNavigation` records the exact
plan and baseline. Replay builds the normal ephemeris and commits that result,
without re-running the navigation search. Pending jobs are transient and never
silently resumed from a save. Model 34 is intentionally incompatible with model
33 saves/journals; no automatic conversion is provided.

Default wait is one local orbit; longer windows remain explicit options. Default
flight range derives from target orbital scale, rather than using seven days for
every planet. The displayed auto settings can be overridden. Navigation outcomes
remain in the navigation panel even when a camera action changes the general
status notice. Manual plan edits explicitly clear the old generated navigation
metrics rather than presenting them as predictions of the edited plan.

The solver now converts impulsive Lambert velocities into a finite Frenet-thrust
seed: transverse thrust rotates velocity without performing work, so raw Cartesian
impulse components were not valid continuous-burn components. Search samples
32 parking phases, distributes refinements across flight durations, and accepts
Newton updates only when a numerical trial reduces the arrival residual. Every
refinement uses the original plan anchor. The parking sweep shares its existing
total step budget instead of restarting that budget at each sample. No integration
tolerance, physical model or fuel limit was relaxed. Failed convergence reports a
predicted miss instead of mislabeling every failure as insufficient fuel.

## Repair verification

Verified working tree: `work/orbit-navigation`, based on `60d97e4`, with the
repair recorded in this delivery. All builds/tests used `-j 1` under dedicated
systemd scopes (MemoryHigh 3G / MemoryMax 5G), with GUI stopped during compilation.
No full workspace suite was run. Shared-target orbit tests were explicitly
recompiled from this worktree; an initial stale zero-test executable was not
counted as evidence.

- `cargo test -p void-orbit --lib navigation -j 1`: eight pass; the old optional
  15-body real-scale witness remains excluded from this command.
- `cargo test -p void-fleet-flight --test plans -j 1 -- --include-ignored`:
  seven pass, including the new expanded 58-body main upper-stage departure,
  append/fuel preservation, stale result refusal, direct checkpoint, replay and
  clearing metrics after manual edits. The real main-game witness takes about
  five seconds and uses the fleet's original tolerances.
- `cargo test -p void-app --lib navigation -j 1`: default target-scale settings,
  cancellation/timeout child reaping and existing frame/crossing edit replay pass.
- Scoped orbit/fleet/app lib/tests Clippy with `-D warnings`, `cargo fmt --all
  --check`, and main-game binary build pass. Final fleet tests and Clippy were
  repeated after the manual-edit metric fix.
- TigerVNC actual main-game input: Aurelia → Vesper search stays responsive while
  the camera moves; Cancel stops its exact child. This initial full-stack case
  completes with an explicit fuel refusal, not a claimed transfer. No-solution
  output is not evidence that all departure windows or other craft are impossible.
- TigerVNC staged upper-stage Aurelia → Selene: Depart generates a 4653.3 m/s
  node at T+4881.7 s; numerical closest altitude 267.4 km at T+876800.2 s.
  `delivery-depart-result.png` shows the path while paused and all 1470 kg fuel.
  Save/load succeeds. `delivery-gui.json` was replayed on that build; its manual
  edit precedes the final metric-clearing adjustment and is historical evidence.
- Loading that unchanged departure plan and requesting Capture appends node 2:
  872.2 m/s at T+876791.7 s, predicted altitude 262.0 km, bound orbit verified.
  `delivery-moon-capture.png` records responsive incremental preview preparation;
  `delivery-moon-capture-result.png` and `delivery-capture-save.json` contain the
  two nodes, full live fuel and `executing=false`. Final delivery binary replays
  `delivery-capture.json`: T+0.000000 s, three vessels, selected v2. This validates
  planning and publication; the full flight has not been flown in this GUI run.

The final binary was also started through the no-argument acceptance script:
`final-default-entry.png` shows the paused, staged Selene setup;
`final-default-result.png` shows the same successful departure after clicking
Depart; `final-edit-invalidates.png` confirms manual editing clears the old
navigation metrics. Those GUI checks were closed after inspection; the script
is ready to launch on the user's desktop.

Artifacts above are under ignored `lab-log/navigation/`. The branch-local binary
is checksum-checked by `tools/navigation-acceptance.sh`; the model-34 starting
checkpoint is separate from the generated two-node review checkpoint. Human
acceptance is not inferred from these checks. Nothing was merged or pushed.

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

Current model version 34 rejects older model saves and recordings explicitly. This
work introduces no automatic conversion of existing save data.

## Original model-33 verification record (historical)

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

## Withdrawal evidence — 2026-10-10

Delivery `b53ede9` is withdrawn following the user's full-desktop hard-lock report.
Previous boot `0d6c1bed7de244a58c724971e6210bb2` ends at 01:14:09 CST; the
next boot starts at 01:14:36. At 01:13:03, gdm-x-session reports SYN_DROPPED
mouse input and debounce timers late by 1157/1171 ms. Kernel and systemd-oomd
queries for 01:00–01:14 do not establish OOM or a GPU reset as the cause. Earlier
in that boot, including 00:16:39 during agent GUI startup, NVRM reports
NV_ERR_NO_MEMORY allocation failures. These earlier errors are a relevant missed
validation signal, not proof of the later hard-lock cause. pstore was not readable
with current permissions.

Agent GUI runs had dedicated memory-capped systemd services and TigerVNC X11.
The shipped script directly executed the game on the user's display, without
those limits. This validation/delivery mismatch is confirmed; adding limits alone
would not establish a fix for a potential graphics/driver lockup. No game, build,
GPU query workload, driver change or reproduction was started during triage.
The launcher now exits before checksum/file loading/process launch; shell syntax
and the refusal exit code were checked. Underlying executable and snapshots are
preserved for investigation, not approved for direct launch.

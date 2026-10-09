# Main-game UI

Main game uses Bevy native UI based on archived `ref/void/src/main.ts` and `style.css`. Full mapping and compatibility details: [spec](specs/game-ui.md).

- Top left: mission time, time rate, pause.
- Bottom left: actual staged parts, active/next/waiting status, fuel/capacity and bars, current connected-stack vacuum delta-v, stage button.
- Bottom centre: SAS, throttle bar and original keyboard controls, altitude/speed modes, original navball and heading.
- Map right: orbital extrema, impact forecast and plotting frame; fades with map transition.
- Bottom right: maneuver plan/editing controls for flight-profile craft and help.
- Top right: collapsed DEV panel; click or backquote to expand, wheel to scroll. Body/camera/visual settings plus full diagnostics.

To edit a maneuver number, click its value, press Ctrl+A to clear, type, then Enter; Esc cancels. Values must be finite and obey core conditions. Buttons and keyboard controls share the command journal. Hovering the HUD leaves flight keys working; dragging or scrolling it consumes camera input. Numeric editing captures flight keyboard input until committed/cancelled.

Visual air, clouds, ocean and stars can be independently toggled without altering physical environment behavior. Saved/replayed Presentation now requires model31. Older model30 files are explicitly rejected.

The archived fixed rocket and HTML controls do not map one-to-one onto arbitrary authored craft: native stage rows resolve PartGraph IDs, delta-v describes the current connected mass/resources, and body/reference selectors cycle actual options. Full thermal, EVA, aircraft, rover, docking and water information is retained in DEV, with selected craft/profile and notices also visible during flight.

Verification is tracked separately for headless checks, agent GUI and human acceptance. See the task spec for the concrete acceptance procedure.

HUD glyphs use bundled DejaVu Sans Mono (Bitstream Vera license in `crates/app/src/fleet_game/fonts/LICENSE`), compiled into the executable. Native navball raster labels keep the established renderer.

Authored Chinese/Japanese/Korean names use a bundled Droid Sans Fallback face, explicitly registered for CJK script fallback; its Apache 2.0 license is included beside the font.

## Build and run

```sh
cargo build -p void-app --bin void-app -j 2
mkdir -p target/acceptance
cp target/debug/void-app target/acceptance/void-app-ui
sha256sum target/acceptance/void-app-ui > target/acceptance/ui-SHA256SUMS
./tools/ui-acceptance.sh flight
```

Other entries: orbit, mars, venus, rover, aircraft, water, stars. The script verifies the executable hash and runs from this worktree. `target/acceptance/ui-SOURCE.json` records source/binary provenance; rebuilding requires updating that record too.

## 2026-10-09 review and validation

Branch `work/game-ui`, baseline `bea8a7e`; no merge/push. Primary agent reviewed reference mapping, command routing, PartGraph/resource readouts, renderer flags, serialization, fonts and input capture. Desktop preview SHA256: `934ddd3bf268ba3458aea16cdfc5b24ee047b0061a81f2aa93b4ae6a3f9dc04d`. Latest acceptance executable SHA256: `dbdee173f4d1f854ef3c8dc08c03032fb0bc427e75e49073145857f9f395bbc8`. The final change reports SAS refusal reasons in the existing notice area; the running desktop preview was kept open rather than restarted.

Headless scope: fleet-flight presentation, solar_scenery and ui_presentation suites passed, including unchanged physics under optical toggles and save/journal roundtrip. App library36 tests passed; the subsequent SAS refusal regression also passed. UI-focused regressions cover: real scene/render settings, quick pointer click applied once, Enter/Esc capture, and typed maneuver value reaching the live plan and replaying. Related app/fleet-flight lib/tests Clippy `-D warnings`, fmt, diff check and native build passed. These are targeted checks, not a full workspace run.

Agent GUI before the user's desktop-only direction: TigerVNC on RTX5060 Laptop/Vulkan, default1280x720 client and maximized1440x879 client. Checked panel positions, bars, reused navball, restored Unicode symbols, DEV expansion/scroll without camera zoom, and actual four optical checkbox commands. Early unsupported-glyph screenshots belong to the earlier candidate and are not final visual evidence. Final reviewed candidate screenshots `final-flight-window.png`, `final-dev-open.png` and `final-visuals-off.png` are in ignored `lab-log/ui-evidence/`; the subsequently rebuilt production differs only in clarified labels, compact maneuver controls, and rate captions.

The incomplete final GUI journal was preserved and explicitly recovered to `final-flight-recovered.json`:6191 committed actions, no pending action,0 discarded tail bytes. Latest binary independently verified it at T+2.133333s. Earlier complete `flight.jsonl` and `flight-save.json` also verified at T+0.050000s; those evidence files belong to the early model31 UI candidate. No automatic save migration or evidence-file patching.

User then requested a normal desktop window instead of TigerVNC. The reviewed executable was opened on display`:1`, scale1.25, and the user said the appearance looked acceptable. This is initial human visual feedback, not acceptance of every maneuver/control/profile scenario. The desktop window remains available for further hands-on inspection. No further TigerVNC work follows that instruction.

Remaining human checks: maneuver GUI in free flight, orbit fade at intermediate zoom, Vesper cloud toggling with air retained, and rover/EVA/aircraft profile usability. Existing diagnostics and controls remain connected; these scenarios have not all been independently exercised in this UI candidate's GUI.

## Merge into master (2026-10-09)

User explicitly authorized merging the reviewed UI branch `bd20325` into master. The code merge is conflict-free; model31/world5 remains unchanged. Earlier GUI and human-feedback scope above remains version-specific; merge authorization does not expand it into acceptance of every profile/control scenario. UI worktree and desktop preview are retained. No push requested.

Merged master working tree (before merge commit): fleet-flight presentation/solar_scenery/ui_presentation18 tests and app lib37 tests passed,0 failures. These checks include command routing, UI input capture, CJK font registration, renderer flags, PartGraph staging and save/journal behavior. Production sources match `bd20325` byte-for-byte; the reviewed UI executable is copied to the master acceptance entry with its build origin recorded. No full workspace run or additional GUI operation.

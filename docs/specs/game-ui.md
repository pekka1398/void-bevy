# Native flight UI

## Scope and reference

User requests the complete archived HUD: `/home/pekka/Desktop/void-bevy/ref/void/src/main.ts` (HUD creation and update) and `src/style.css`. `lab/view` supplements map label/camera behavior; it is not the complete flight HUD. Common development rules: `AGENTS.md`.

Implement in main game's existing Fleet scene, Bevy native UI, sharing the existing simulation, authoritative PartGraph, stable module IDs and command journal. No separate runtime or physics changes. `crates/app/src/fleet_game/ui.rs` owns HUD components, rendering, editing and interaction. Existing navball raster renderer/core and marker semantics are reused unchanged.

## Mapping

| Reference | Native main-game mapping |
| --- | --- |
| Clock top left | Mission day/hour/minute/second, clickable rates, pause, selected-rate indication |
| Stages bottom left | Actual selected vessel PartGraph staged parts/module data, next/waiting/active/consumed state, connected fuel/capacity bar, vacuum delta-v readout, stage button |
| Flight bottom centre | SAS active indication, vertical throttle bar/firing colour, original keyboard throttle controls, ALT/AGL and SURFACE/ORBIT toggles, body-relative readouts, 150px navball and heading/pitch |
| Orbit right | Ap/Pe/escape and predicted impact; visibility and alpha follow continuous map weight; plotting frame control |
| Maneuver bottom right | Actual selected plan/burn status, add/remove/previous/next/reference/warp/Pe/Ap/execute/abort; editable start/prograde/normal/radial; existing keyboard steppers retained; hidden for non-flight profiles |
| DEV top right, initially collapsed | Observed-body selection, near/orbit/far camera, terrain/edges/bounds/collision/exposure and visual air/cloud/ocean/stars toggles; reference camera distance/map/up/co-rotation/tiles/resolution/sea plus complete existing diagnostics, scrollable |
| Help bottom right | Clickable help with current Bevy key bindings |
| Newer game features | Compact selected craft/profile and notices remain visible; full thermal/EVA/aircraft/vehicle/docking/water diagnostics remain in DEV |

## Numerical and behavior limits

- The reference uses a fixed two-stage rocket. Native rows are derived from actual staged parts and module mappings instead of hardcoded booster/upper names. Stable IDs resolve the current selected owner after docking/separation.
- Delta-v is **current connected-stack vacuum delta-v**, labeled `stack vac`, using engine Isp, actual resource crossfeed graph, current vessel mass and remaining fuel. It does not predict future staging mass or sum overlapping engine resource pools. Non-rocket modules show a dash; multiple-engine modules require a separate combined-engine estimator and show unavailable explicitly.
- Planet selection observes an existing authored body and does not reload/reset the physical world. Current native selector cycles bodies/reference options rather than HTML drop-down menus.
- Editable numeric fields: click, Ctrl+A to clear, type numeric value, Enter to submit through `Action::EditManeuver`, Esc to cancel. Invalid/nonfinite text is rejected with reason. Existing keyboard steppers remain available.
- Native buttons enqueue Bevy picking `Pointer<Click>` events and consume each click once; dispatch does not require a held mouse button sampled by a rendered frame.
- Core-dependent actions use existing `Action` / `ViewCommand`, with explicit rejection notices and recording/playback semantics. DEV/help expansion, text draft and pointer capture are local UI state.
- Hovering HUD preserves keyboard flight shortcuts; pointer interactions consume camera drag/zoom and map clicks. Pointer capture persists until mouse release. Focused numeric editing suppresses flight shortcuts and neutralizes held turn/RCS inputs.
- Visual toggles affect optical rendering only. They do not change atmospheric physical forces, cloud/world recipes, ocean/water forces, or stellar simulation.
- Visual flags are mandatory serialized Presentation fields: model31; model30 saves/recordings are incompatible and not repaired with defaults. World configuration version remains unchanged.
- Native current key bindings remain authoritative (F1 camera presets; help is clickable; backquote DEV); reference key hints are updated accordingly.

## Acceptance

Run main game in this worktree; verify 1440×900 and 1280×800 layouts. Compare panel positions, rounded translucent boxes, numeric units, bars and original navball marker semantics. Test stage/throttle/SAS and ALT/AGL/SURFACE/ORBIT buttons, zoom smoothly to map and observe orbit fade, each visual checkbox, Vesper clouds off/on with atmosphere still enabled, DEV scroll, maneuver editable fields and actions, pointer drag capture and keyboard control on hover. Verify save/load and recording replay preserve visual flags and maneuver commands. Exercise rover/EVA/aircraft profiles and confirm existing diagnostics remain accessible.

Headless tests do not replace human acceptance of actual game behavior. Parent coordinates executable/test linking and GUI verification; no merge/push before user instruction.

## Font asset

DejaVu Sans Mono is bundled with its upstream copyright/license in `crates/app/src/fleet_game/fonts/` and decoded synchronously from compiled bytes. HUD/map Unicode glyphs do not depend on a runtime filesystem font or launch working directory. Navball core/raster marker rendering is unchanged.

Droid Sans Fallback Full is also embedded, with upstream copyright and complete Apache 2.0 license. It is explicitly registered in Bevy’s Fontique collection for Han/Hiragana/Katakana/Hangul script fallback, preserving authored multilingual names while Latin/numeric HUD text stays monospaced. The app directly names Fontique 0.9.0, the same already locked dependency used by Bevy.

Distance format preserves reference thresholds: absolute 1e9 m uses three-decimal Gm, absolute 1e4 m uses one-decimal km, otherwise one-decimal m. Mission time omits the day prefix before the first day.

# UI completion review matrix

Scope authorized by user: implement assembly, contextual vehicles, navigation/planning, session/settings and feedback without pausing for appearance approval. Human review follows the complete candidate. This authorization does not request merge or push.

Review the candidate in `work/ui-completion`, based on master `95c0581`. Keep unrelated master NOTE/status/task edits intact.

## Required checks

- All new UI systems initialize together; scroll containers, font fallback and panels work at 1280×720 and a larger desktop window.
- Modal menu/workshop/text/rebinding capture neutralizes turn, RCS translation, EVA walk and rover drive/steer. Releasing a held button outside, losing focus and selecting another owner cannot retain continuous commands.
- Workshop edits compile atomically; invalid attachments, removal of root, bad files and nonfinite values leave the draft unchanged. Actual preview, placement/rotation, stable selection, module stages and resource/statistics correspond to the compiled craft. Launch enters the current Fleet world and journal, with explicit body and unchanged preexisting vessels.
- Context controls fit and show actual profile/capabilities; docking selection uses stable part/module IDs; denied requests show reasons. Navigation updates after vessel separation, docking and loading a different body list.
- Save/load uses one shared keyboard/menu handler. External missing, malformed, old model, bad graph/world and mark mismatch reject before live mutation or journal intent. Atomic write failures preserve existing files and do not remove another writer's temp file. Successful load records and replays identically.
- Display flags/exposure remain journalled ViewCommands. UI scale/bindings are frontend preferences. Remapping/key capture never triggers a flight action in the same frame; virtual buttons retain canonical actions.
- Playback permits local menu/navigation only where supported and blocks destructive/pilot controls. Restart/reset is explicit, and rebuilds geometry/prediction/port selection without hiding rejected actions.

Target app and affected core tests/clippy/build use `-j 2`; do not run whole workspace automatically. Record candidate commit, headless results, agent GUI evidence and remaining human checks separately. Direct desktop launch honors the user's GUI preference.

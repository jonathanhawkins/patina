# Editor Parity — Animation editor (AnimationPlayer, timeline, tracks, AnimationTree)

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 17 — "Animation editor parity:
AnimationPlayer, timeline, tracks, and AnimationTree" (animation panel,
timeline, keyframes, track types, bezier, onion skinning, AnimationTree).

This execution map enumerates the concrete beads required for animation-editor
parity with Godot: the AnimationPlayer panel and animation lifecycle, the
timeline and playhead, keyframe and track editing across track types, bezier
curve editing, interpolation/loop modes, onion skinning, playback controls,
and the AnimationTree graph editor.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `anim-player-panel` The AnimationPlayer panel lists animations and supports create/rename/duplicate/delete
   Acceptance: The panel lists the player's animations, and create/rename/duplicate/delete operate on the AnimationLibrary and update the selection (test: `anim_player_panel_manage`)

2. `anim-timeline-playhead` The timeline shows a time ruler and a draggable playhead with zoom and snap
   Acceptance: Dragging the playhead scrubs the animation time, the ruler reflects the current zoom, and snapping aligns the playhead to the configured time step (test: `anim_timeline_playhead`)

3. `anim-keyframe-edit` Keyframes can be inserted, selected, moved, and deleted on tracks
   Acceptance: Inserting a key adds it at the playhead on the active track, dragging moves it in time, multi-select moves a group, and delete removes the selection (test: `anim_keyframe_edit`)

4. `anim-track-types` The editor supports the core track types (property/value, transform, method-call, audio, animation)
   Acceptance: Each supported track type can be added to an animation, renders its type-appropriate row, and stores keys in the correct typed form (test: `anim_track_types`)

5. `anim-track-add-remove` Tracks can be added (including via inspector keying) and removed
   Acceptance: Adding a track for a node/property creates the track, keying a property from the inspector targets it, and removing a track deletes it with its keys (test: `anim_track_add_remove`)

6. `anim-bezier-editing` Bezier (value-curve) tracks expose draggable handles for easing
   Acceptance: A bezier track renders an editable curve, dragging in/out handles reshapes the interpolation, and the evaluated value follows the curve between keys (test: `anim_bezier_editing`)

7. `anim-interpolation-loop-modes` Per-key interpolation modes and animation loop mode are configurable
   Acceptance: Setting a key's interpolation (nearest/linear/cubic) changes how values are sampled between keys, and toggling loop mode wraps playback at the animation boundaries (test: `anim_interpolation_loop_modes`)

8. `anim-onion-skinning` Onion skinning previews neighboring frames around the playhead
   Acceptance: Enabling onion skinning renders ghosted past/future frames with configurable count, and disabling it clears the ghosts (test: `anim_onion_skinning`)

9. `anim-playback-controls` Playback controls drive play/pause/stop, loop, speed, and autoplay-on-load
   Acceptance: Play/pause/stop control the running animation, the speed scalar adjusts playback rate, and the autoplay flag marks the animation to start on scene load (test: `anim_playback_controls`)

10. `anim-tree-graph` The AnimationTree editor edits state-machine / blend-tree nodes and transitions
    Acceptance: The AnimationTree graph lets nodes (state machine, blend space, blend nodes) be added, connected, and parameterized, and transitions between states are created and removed (test: `anim_tree_graph_edit`)

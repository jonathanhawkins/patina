# Editor Parity — Editor systems (project settings, editor settings, VCS, export, variant coverage)

Per-lane execution map for the editor's cross-cutting systems. Source lane:
`Editor systems parity: project settings, editor settings, VCS, export,
variant coverage` in `prd/EDITOR_PARITY_BEADS.md` (lane 18).

Scope: the configuration and integration surfaces that aren't tied to a single
dock or viewport — the Project Settings dialog (incl. input map, autoloads,
plugins), the Editor Settings dialog, version-control integration, the export
preset/template system, and Variant type coverage across these editors.

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

1. `systems-project-settings` Implement the Project Settings dialog (general settings tree, add/override/revert a setting) reading and writing `project.godot`.
   Acceptance: changing, adding, and reverting a project setting persists to `project.godot` and reloads on reopen (test: `systems_project_settings_persist`)

2. `systems-input-map` Implement the Project Settings Input Map tab (add/remove actions, bind key/mouse/joypad events, set deadzone) persisted to project settings.
   Acceptance: adding an action and binding an input event persists to the input map and the action resolves at runtime (test: `systems_input_map_actions_persist`)

3. `systems-autoloads` Implement autoload/singleton management (add, rename, reorder, enable, remove) writing the autoload section of project settings.
   Acceptance: adding an autoload registers a global singleton, reordering changes load order, and removal clears it (test: `systems_autoloads_register_globals`)

4. `systems-editor-settings` Implement the Editor Settings dialog covering interface, text editor, and shortcut preferences persisted to the editor settings store.
   Acceptance: changing an editor setting and a shortcut persists across editor restarts and applies immediately (test: `systems_editor_settings_persist_and_apply`)

## Next

5. `systems-export-presets` Implement export presets (create/edit/delete a preset, select platform, set resources/features, export to a target).
   Acceptance: creating a preset and exporting produces a target artifact and the preset persists in `export_presets.cfg` (test: `systems_export_presets_create_and_export`)

6. `systems-vcs-integration` Implement version-control integration (repo status, stage/unstage, commit, branch list, diff view) via the VCS interface.
   Acceptance: staging changes and committing creates a commit and the status/diff views reflect the working tree (test: `systems_vcs_stage_commit_and_diff`)

## Later

7. `systems-plugin-management` Implement editor plugin management (enable/disable plugins from Project Settings) loading and unloading the plugin at runtime.
   Acceptance: enabling a plugin loads it and registers its editor contributions; disabling unloads it (test: `systems_plugin_enable_disable`)

8. `systems-variant-coverage` Ensure Variant type coverage across settings/export serialization so every supported Variant type round-trips through the settings and preset stores.
   Acceptance: each supported Variant type set as a project/editor setting serializes and deserializes without loss (test: `systems_variant_coverage_round_trips`)

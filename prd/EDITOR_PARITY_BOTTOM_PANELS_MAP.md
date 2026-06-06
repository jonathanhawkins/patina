# Editor Parity — Bottom panels (output, debugger, monitors, audio buses, shader editor)

Per-lane execution map for the editor's bottom dock panels. Source lane:
`Bottom panels parity: output, debugger, monitors, audio buses, shader editor`
in `prd/EDITOR_PARITY_BEADS.md` (lane 12). Source surface: the Output console,
the Debugger (stack/variables/errors/profiler), the performance Monitors, the
Audio bus layout editor, and the bottom Shader editor, plus the shared
show/hide panel bar.

Scope: the bottom panel bar and the panels it toggles. Deep script-editor and
animation-editor behavior have their own lanes
(`EDITOR_PARITY_SCRIPT_EDITOR_*` and `EDITOR_PARITY_ANIMATION_EDITOR_MAP.md`);
here the shader editor is the bottom-panel shell only.

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

1. `bottom-panel-bar` Implement the bottom panel bar that lists registered panels (Output, Debugger, Audio, Shader, …) and shows/hides the active one, with an expand-to-fill toggle.
   Acceptance: clicking a panel button shows that panel and hides others, clicking the active button collapses the dock, and expand fills the editor height (test: `bottom_panel_bar_toggles_active_panel`)

2. `bottom-output-console` Implement the Output console capturing game/editor stdout, errors, and warnings with clear, and per-severity filtering.
   Acceptance: running prints appear in Output, error/warning lines are styled by severity, the severity filter hides matching lines, and Clear empties the log (test: `bottom_output_console_captures_and_filters`)

3. `bottom-debugger-stack-vars` Implement the Debugger stack-frame list and variable inspector that populate on a breakpoint/error and let the user step and inspect locals.
   Acceptance: hitting a breakpoint populates the stack frames and selecting a frame shows its local variables in the inspector (test: `bottom_debugger_stack_and_variables_populate`)

4. `bottom-debugger-errors-tab` Implement the Debugger Errors tab listing runtime errors/warnings with message, source, and a jump-to-source action.
   Acceptance: a runtime error appears in the Errors tab with its source location and activating it navigates to that line (test: `bottom_debugger_errors_tab_lists_and_navigates`)

## Next

5. `bottom-monitors` Implement the performance Monitors panel graphing FPS, memory, draw calls, and physics over time during a play session.
   Acceptance: during play the Monitors panel plots the selected metrics over time and selecting a monitor updates the graph (test: `bottom_monitors_plot_metrics`)

6. `bottom-audio-buses` Implement the Audio bus layout editor (add/remove bus, volume sliders, mute/solo/bypass, effect chain, send routing) with save/load of the layout resource.
   Acceptance: adding a bus, adjusting its volume, toggling mute, and adding an effect update the bus layout and the layout saves to and loads from a resource (test: `bottom_audio_buses_edit_and_persist`)

## Later

7. `bottom-shader-editor-shell` Implement the bottom Shader editor shell that opens when a shader resource is edited, with tabbed open shaders and a compile/error status line.
   Acceptance: editing a shader resource opens it in the bottom shader panel as a tab and a compile error is surfaced in the status line (test: `bottom_shader_editor_shell_opens_and_reports`)

8. `bottom-debugger-profiler` Implement the Debugger profiler tab measuring per-frame function time with start/stop capture and a sortable cost breakdown.
   Acceptance: starting the profiler during play captures per-frame function costs and the breakdown is sortable by total/self time (test: `bottom_debugger_profiler_captures_costs`)

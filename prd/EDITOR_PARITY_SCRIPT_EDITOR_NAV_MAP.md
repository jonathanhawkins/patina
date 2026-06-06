# Editor Parity — Script editor search, navigation, debugging, and script panel

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 14 — "Script editor parity:
search, navigation, debugging, and script panel" (search, navigation,
debugging, and script panel).

This execution map enumerates the concrete beads required for script-editor
search/navigation/debugging parity with Godot: in-file find/replace and
find-in-files, go-to-line and go-to-function/symbol, goto-definition,
bookmarks, breakpoints with debugger integration, the open-scripts panel, and
member/outline navigation.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `script-nav-find-replace` In-file find and replace supports case/whole-word/regex options and replace-all
   Acceptance: Find highlights and cycles matches honoring case/whole-word/regex toggles, and replace / replace-all substitutes matches and reports the replacement count (test: `script_nav_find_replace`)

2. `script-nav-find-in-files` Find-in-files searches across project scripts and lists results with navigation
   Acceptance: A project-wide search returns matches grouped by file with line context, and activating a result opens that file at the matched line (test: `script_nav_find_in_files`)

3. `script-nav-goto-line` Go-to-line jumps the caret to a requested line number
   Acceptance: The go-to-line action moves the caret to the requested line, clamps out-of-range input to the valid range, and centers the line in view (test: `script_nav_goto_line`)

4. `script-nav-function-list` A function/member list (outline) lets the user jump to a symbol in the current script
   Acceptance: The outline lists the script's functions/members in document order and selecting an entry moves the caret to that symbol's definition (test: `script_nav_function_list`)

5. `script-nav-goto-definition` Goto-definition (Ctrl+click / shortcut) navigates to the definition of the symbol under the caret
   Acceptance: Invoking goto-definition on a symbol opens the defining script (or scrolls within the current one) at the definition line, and is a no-op for unresolved symbols (test: `script_nav_goto_definition`)

6. `script-nav-bookmarks` Bookmarks can be toggled on lines and navigated next/previous
   Acceptance: Toggling a bookmark marks the line in the gutter, and next/previous-bookmark cycles the caret through bookmarks across the current script (test: `script_nav_bookmarks`)

7. `script-nav-breakpoints-debugger` Breakpoints can be toggled and integrate with the debugger to pause execution
   Acceptance: Toggling a breakpoint marks the line and registers it with the debugger so a running project halts at that line, exposing the stack/locals; clearing it resumes normal execution (test: `script_nav_breakpoints_debugger`)

8. `script-nav-open-scripts-panel` The open-scripts panel lists loaded scripts and switches the active script on selection
   Acceptance: The script panel lists currently open scripts, selecting one makes it the active editor buffer, and closing an entry removes it from the panel (test: `script_nav_open_scripts_panel`)

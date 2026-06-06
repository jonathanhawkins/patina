# Editor Parity — Script editor core editing features

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 13 — "Script editor core
editing features".

This execution map enumerates the concrete beads required for script-editor
core text-editing parity with Godot's GDScript code editor: syntax
highlighting, autocompletion, smart auto-indent, bracket auto-close and
matching, code folding, comment toggling, line operations, multi-caret
editing, indent/unindent, and save with whitespace normalization.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `script-core-syntax-highlight` GDScript syntax highlighting colors keywords, strings, comments, numbers, and types
   Acceptance: Opening a GDScript file applies highlighting spans for keywords, string/number literals, comments, and known types, and updates spans as text changes (test: `script_core_syntax_highlight`)

2. `script-core-autocomplete` Context-aware autocompletion suggests identifiers, members, and keywords and inserts the chosen entry
   Acceptance: Triggering completion offers ranked candidates for the current context (locals, members after `.`, keywords), and accepting one inserts it at the caret (test: `script_core_autocomplete`)

3. `script-core-auto-indent` Pressing Enter auto-indents the new line based on the previous line and block openers
   Acceptance: Newlines inherit the previous line's indentation and add one level after a block opener (e.g. line ending in `:`), respecting the configured tab/space indent style (test: `script_core_auto_indent`)

4. `script-core-bracket-autoclose-match` Brackets/quotes auto-close and matching pairs are highlighted
   Acceptance: Typing an opening bracket or quote inserts the closing counterpart with the caret between them, and the editor highlights the matching pair around the caret (test: `script_core_bracket_autoclose_match`)

5. `script-core-comment-toggle` Toggle-comment comments or uncomments the selected lines
   Acceptance: The comment-toggle action prefixes uncommented selected lines with the line-comment token and removes it from already-commented lines, preserving indentation (test: `script_core_comment_toggle`)

6. `script-core-line-operations` Duplicate-line, move-line-up/down, and delete-line operate on the caret line or selection
   Acceptance: Duplicate-line copies the current line/selection, move-line-up/down reorders it, and delete-line removes it, each updating the caret position consistently (test: `script_core_line_operations`)

7. `script-core-indent-unindent` Tab/Shift+Tab indent and unindent the selected lines by one level
   Acceptance: Indent adds one indent level to each selected line and unindent removes one (no-op at column zero), using the configured indent width (test: `script_core_indent_unindent`)

8. `script-core-code-folding` Foldable regions (functions, indented blocks) can be collapsed and expanded
   Acceptance: Foldable regions show a gutter affordance, folding hides the region body and shows a placeholder, and unfolding restores it; folds survive edits outside the region (test: `script_core_code_folding`)

9. `script-core-multi-caret` Multiple carets / column selection apply edits at all caret positions simultaneously
   Acceptance: Adding carets (e.g. add-caret or select-next-occurrence) and typing inserts the same text at every caret, and Escape collapses back to a single caret (test: `script_core_multi_caret`)

10. `script-core-save-and-trim` Saving writes the buffer and normalizes trailing whitespace / final newline per editor settings
    Acceptance: Save persists the script, and when whitespace normalization is enabled it trims trailing whitespace and ensures a single final newline; undo restores the pre-save buffer state (test: `script_core_save_and_trim`)

# Issue Triage Process

How incoming issues (bugs, feature requests, parity reports) are triaged,
prioritized, and tracked through resolution.

---

## Triage Flow

```
New Issue → Label → Prioritize → Assign → Implement → Verify → Close
              │         │            │
              ▼         ▼            ▼
         needs-triage  P0–P3    bead created
         subsystem     milestone   in br tracker
         type label
```

### Step 1: Label

Every new issue gets:

1. **Type label**: `bug`, `enhancement`, `parity`, or `question`
2. **Subsystem label**: matches the crate (e.g., `gdscene`, `gdphysics2d`)
3. **Triage label**: starts as `needs-triage`

### Step 2: Prioritize

Apply a priority label based on severity and impact:

| Priority | Label | Criteria | Response Target |
|----------|-------|----------|-----------------|
| P0 | `P0-critical` | Engine crash, data loss, blocks all testing | Same day |
| P1 | `P1-high` | Major subsystem broken, no workaround, blocks CI | 1–2 days |
| P2 | `P2-medium` | Feature broken but workaround exists, CI passes | 1 week |
| P3 | `P3-low` | Cosmetic, edge case, minor inconvenience | Best effort |

**Escalation rules**:
- Any crash in a core subsystem (gdcore, gdobject, gdscene) starts at P1 minimum
- Parity regressions (worked before, broken now) start at P1 minimum
- Issues with reproduction steps get +1 priority bump over issues without

### Step 3: Create Bead

Once prioritized, create a tracking bead in the `br` issue tracker:

```bash
br create --title "Fix: <issue summary>" \
  --priority <0-3> \
  --label <subsystem> \
  --description "GitHub issue #<number>. <details>"
```

Link the GitHub issue number in the bead description so work can be traced.

### Step 4: Assign

Beads are picked up by the flywheel worker loop:
- P0 beads are assigned immediately by the coordinator
- P1–P3 beads are pulled by workers via `br ready --unassigned`

### Step 5: Implement & Verify

The assignee must:
1. Write a failing test that reproduces the issue
2. Fix the bug or implement the feature
3. Verify all existing tests still pass
4. Report completion via the flywheel completion flow

### Step 6: Close

After verification:
- The bead is marked `done` in `br`
- The GitHub issue is closed with a reference to the fix
- For parity bugs, the oracle comparison test is updated if needed

---

## Issue Templates

Three templates are provided in `.github/ISSUE_TEMPLATE/`:

### Bug Report (`bug_report.yml`)

For runtime bugs, crashes, or incorrect behavior. Captures:
- Affected subsystem
- Severity level
- Steps to reproduce
- Expected vs actual behavior
- Platform and version info
- Whether Godot handles it correctly (parity check)

### Feature Request (`feature_request.yml`)

For new features or enhancements. Captures:
- Affected subsystem
- Problem statement
- Proposed solution
- Whether it's a Godot parity feature

### Parity Report (`parity_report.yml`)

For behavioral differences between Patina and Godot 4.6. Captures:
- Godot behavior (expected)
- Patina behavior (actual)
- Test case demonstrating the difference
- Version info for both engines

---

## Severity Classification Guide

### P0 Critical

Apply when:
- Engine panics or crashes on startup
- Data loss (scene file corruption, resource overwrite)
- All tests in a subsystem fail
- CI is completely blocked

Examples:
- `cargo test --workspace` fails to compile
- Scene tree infinite loop causes hang
- Resource save corrupts `.tres` file

### P1 High

Apply when:
- Major feature doesn't work at all
- No workaround exists for the reporter's use case
- CI is partially blocked (one test suite fails)
- A previously working feature regressed

Examples:
- `CharacterBody2D.move_and_slide()` ignores collision
- Signal connections silently dropped
- Resource UID resolution returns wrong file

### P2 Medium

Apply when:
- Feature partially works with known workaround
- CI passes but with warnings
- Parity gap in a non-critical API

Examples:
- `AnimationPlayer` plays at wrong speed (workaround: adjust keyframe times)
- `TileMap` loads tiles but ignores custom data layers
- `RichTextLabel` renders BBCode but ignores `[table]` tag

### P3 Low

Apply when:
- Cosmetic issue (wrong color, alignment off)
- Edge case that rarely occurs in practice
- Missing convenience API (core functionality works)
- Documentation improvement

Examples:
- `Vector2.from_angle()` missing (use `Vector2(cos(a), sin(a))` instead)
- Editor theme colors slightly off from Godot defaults
- Error message says "unknown node" instead of the specific type name

---

## Triage Labels Quick Reference

| Label | Category | Applied When |
|-------|----------|-------------|
| `needs-triage` | Status | New issue, not yet reviewed |
| `triaged` | Status | Priority and subsystem assigned |
| `blocked` | Status | Waiting on dependency or external input |
| `wontfix` | Resolution | Intentional difference from Godot, or out of scope |
| `duplicate` | Resolution | Same as existing issue |
| `bug` | Type | Incorrect behavior |
| `enhancement` | Type | New feature or improvement |
| `parity` | Type | Behavioral difference from Godot 4.6 |
| `question` | Type | Usage question, not a bug |
| `P0-critical` | Priority | See severity guide above |
| `P1-high` | Priority | |
| `P2-medium` | Priority | |
| `P3-low` | Priority | |
| `gdcore` | Subsystem | Core math, types, IDs |
| `gdscene` | Subsystem | Scene tree, nodes, packed scenes |
| `gdphysics2d` | Subsystem | 2D physics |
| `gdphysics3d` | Subsystem | 3D physics |
| `gdeditor` | Subsystem | Editor UI and server |
| `gdscript` | Subsystem | GDScript interop |
| `gdaudio` | Subsystem | Audio system |
| `gdplatform` | Subsystem | Platform, input, windowing |
| `gdresource` | Subsystem | Resource loading, UID, cache |

---

## Parity Bug Triage

Parity reports get special handling:

1. **Verify the Godot version**: Check that the reporter tested against
   Godot 4.6.1-stable (our oracle pin). Behavior differences in older
   Godot versions may not be bugs.

2. **Check oracle outputs**: If we have oracle data for the reported behavior
   (`fixtures/oracle_outputs/`), compare against it.

3. **Check existing tests**: Search `engine-rs/tests/` for related parity
   tests. If one exists and passes, the report may be about an edge case
   not yet covered.

4. **Write the parity test first**: Before fixing, write a test that
   demonstrates the expected Godot behavior. This prevents regressions.

5. **Update golden data if needed**: If the fix changes oracle comparison
   results, regenerate golden files and commit them with the fix.

---

## Integration with `br` Tracker

The `br` CLI is the source of truth for work tracking:

```bash
# View all open bugs by priority
br list --status open --label bug --sort priority

# View parity gaps
br list --status open --label parity

# View triage queue
br list --status open --label needs-triage

# Create a bead from a GitHub issue
br create --title "Fix: signal dropped on reparent" \
  --priority 1 --label gdscene --label parity \
  --description "GitHub #42. Signals disconnect when node is reparented."

# Check what's ready for work
br ready --unassigned
```

---

## Response Time Expectations

| Priority | First Response | Fix Merged |
|----------|---------------|------------|
| P0 | Same day | 1–2 days |
| P1 | 1 day | 3–5 days |
| P2 | 3 days | 1–2 weeks |
| P3 | 1 week | Best effort |

These are targets, not guarantees. Community contributions are welcome for
all priority levels.

---
name: regression-test
description: After fixing a bug, create in-depth e2e integration tests that catch the bug and similar classes of issues. Analyzes the fix, identifies the bug category, and generates comprehensive regression tests.
argument-hint: [bug-description or bead-id]
---

# Regression Test Generator

After a bug is fixed and verified, create comprehensive e2e integration tests that would have caught it — plus similar issues of the same class — so it can never happen again.

## Steps

### 1. Identify the Bug and Fix

Analyze what was just fixed by examining recent changes:

```bash
git diff HEAD~1 --stat
git diff HEAD~1
git log -1 --format='%s%n%n%b'
```

If `$ARGUMENTS` is provided, use it as context (could be a bead ID to look up with `br show`, or a description of the bug).

If no recent commit exists, ask the user what bug was fixed.

### 2. Classify the Bug Category

Determine which category the bug falls into. Common categories in this codebase:

- **resource-cache**: deduplication, invalidation, UID resolution, subresource sharing
- **lifecycle**: notification ordering, _ready/_process/_enter_tree sequencing, reparenting
- **signal**: dispatch ordering, argument forwarding, deferred vs immediate, connect/disconnect
- **scene-loading**: packed scene instancing, ownership, unique names, ext_resource refs
- **physics**: stepping, body sync, collision registration, determinism
- **input**: action binding, snapshot routing, keyboard/mouse event handling
- **rendering**: draw ordering, viewport composition, camera registration, visibility layers
- **classdb**: property defaults, method metadata, inheritance chains
- **nodepath**: resolution, NodeID mapping, generic vs typed
- **gdscript-interop**: instance IDs, onready, callable parity
- **concurrency**: race conditions, shared state, Arc/Mutex correctness
- **api-surface**: missing methods, wrong signatures, return type mismatches
- **config/parsing**: tscn/tres parsing, variant conversion, property stripping

### 3. Design the Test Suite

Create a new test file at `engine-rs/tests/<category>_regression_<short_descriptor>_test.rs`.

The test suite MUST include:

#### a) Direct Regression Test
A test that **exactly reproduces** the original bug scenario. This test would have FAILED before the fix and PASSES now. Name it clearly: `test_<what_broke>_regression`.

#### b) Boundary Tests (at least 3)
Tests that probe the edges of the fix:
- Just below the threshold that triggered the bug
- Just above the threshold
- At exact boundary values
- With empty/zero/None inputs where applicable

#### c) Stress/Scale Tests (at least 2)
Tests that exercise the same code path under load:
- Repeated operations (100+ iterations)
- Bulk operations
- Rapid creation/deletion cycles

#### d) Variant Tests (at least 3)
Tests covering similar-but-different scenarios in the same bug category:
- Different types/configurations that use the same code path
- Interactions with adjacent subsystems
- Composition scenarios (e.g., nested scenes + the fix area)

#### e) Negative Tests (at least 2)
Tests confirming that invalid inputs or error conditions are handled correctly:
- Graceful error handling (no panics)
- Correct error types returned
- State remains consistent after errors

### 4. Implementation Requirements

Each test file MUST follow these patterns from the codebase:

```rust
//! <Category> regression tests (<bead-id if available>).
//!
//! Covers the original bug plus boundary, stress, variant, and negative cases
//! to prevent regressions in <specific area>.

// Use the same crate imports as other tests in the category
// Reuse existing test helpers — check engine-rs/tests/ for patterns

#[test]
fn test_<name>() {
    // Arrange — set up the exact scenario
    // Act — perform the operation that was broken
    // Assert — verify correct behavior with descriptive messages
    assert!(result.is_ok(), "Expected <X> but got error: {result:?}");
}
```

- Use `#[test]` (not async unless the code under test requires it)
- Include doc comments on each test explaining WHAT it tests and WHY
- Use descriptive assertion messages that explain the expected vs actual
- If the test needs fixtures (scenes, resources), check `fixtures/` first — reuse existing ones
- If new fixture files are needed, create them in the appropriate `fixtures/` subdirectory

### 5. Verify the Tests

Run the new tests and confirm they all pass:

```bash
cd engine-rs && cargo test --test <test_file_name> -- --nocapture 2>&1
```

If any test fails, fix it. Every test must pass.

Then run the full test suite to make sure nothing was broken:

```bash
cd engine-rs && cargo test --workspace 2>&1 | tail -20
```

### 6. Summary Report

After all tests pass, output a summary:

```
## Regression Test Report

**Bug**: <one-line description>
**Category**: <bug category>
**Test file**: engine-rs/tests/<filename>.rs

### Tests created:
| # | Test Name | Type | What it guards against |
|---|-----------|------|----------------------|
| 1 | test_... | regression | Original bug scenario |
| 2 | test_... | boundary | Edge case at ... |
| ... | ... | ... | ... |

### Coverage: X tests covering Y distinct scenarios
```

## Error Handling

- If no recent fix is detectable and no arguments given, ask the user to describe the bug
- If the bug category is ambiguous, pick the closest match and note it in the test doc comment
- If existing tests already cover the exact scenario, note this and focus on the gaps
- Never create tests that duplicate existing coverage — search first with `grep -r "test_name_pattern" engine-rs/tests/`

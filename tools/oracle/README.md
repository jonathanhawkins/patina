# Oracle Tooling

This directory contains the bootstrap oracle toolchain for Patina.

Each capture type defined in [TEST_ORACLE.md](/Users/bone/dev/games/patina/TEST_ORACLE.md)
has a concrete entrypoint:

- `scene_tree_dumper.py`
- `property_dumper.py`
- `signal_tracer.py`
- `notification_tracer.py`
- `resource_roundtrip.py`
- `render_capture.py`
- `physics_tracer.py`
- `run_fixture.py`

The scripts currently wrap machine-readable probe payloads into the standard
oracle envelope using the pinned upstream Godot version and commit.

`run_fixture.gd` is the upstream-facing combined capture entrypoint. Its
envelope now includes:

- final `scene_tree`
- final `properties`
- per-frame `frame_trace`
- `signal_trace`
- `notification_trace`

Bootstrap generation command:

```sh
python3 tools/oracle/run_fixture.py
```

That command generates one scene golden and one resource golden under
`fixtures/golden/`:

- `fixtures/golden/scenes/scene_simple_hierarchy_01.json`
- `fixtures/golden/resources/resource_simple_01.json`

These bootstrap payloads are intentionally small. The next step is to replace
their checked-in inputs with direct outputs from `apps/godot/` probes running
inside the pinned `upstream/godot/` oracle.

## Conformance Gate (pat-mgixk.403)

`conformance_capture.py` + `conformance_probe.gd` are a different kind of
capture from everything above: they record whole **projects**, not single
scenes or resources, and the result is a gate rather than a reference dump.

The 2026-10-30 port audit found 93 unwired and 19 stub features behind beads
that had closed green, because every `*_parity_test.rs` suite compares Patina
with a golden Patina itself produced. The conformance gate closes that hole by
putting real Godot on the other side of the diff.

```sh
# Bless (or re-bless) every golden. Needs Godot 4.6.1; override the binary
# with GODOT_BIN=/path/to/Godot.
python3 tools/oracle/conformance_capture.py

# One project only.
python3 tools/oracle/conformance_capture.py timer_queue_free

# Re-capture and report drift without writing anything. Exit 1 on drift.
python3 tools/oracle/conformance_capture.py --check
```

Each run copies `engine-rs/fixtures/conformance/<name>/` to a temporary
directory, drops `conformance_probe.gd` in beside it and generates an
`override.cfg` registering the probe as an autoload. The fixture project on
disk is never modified, so the project Godot measures is byte-identical to the
one Patina boots.

The Patina side is `patina_runner::conformance::capture_project`, which the
runner's own `--trace-out` flag also calls:

```sh
patina-runner engine-rs/fixtures/conformance/timer_queue_free/project.godot \
    --trace-out /tmp/envelope.json
```

The diff itself runs as `engine-rs/tests/conformance_suite`, whose
`conformance_gate_ratchet` test holds the ratchet in
`engine-rs/fixtures/golden/conformance/baseline.json`. See that file's `_note`
for how to raise it.

**`print()` ordering is deliberately not compared.** Godot gives a script no
hook on `print`, so the Godot side is read off process stdout while the Patina
side is drained per node from each script's own buffer; the two orders differ
by the measurement, not by the port. Both sides filter on the `[T] ` marker
every fixture prefixes its observable lines with, which is why a fixture that
forgets the prefix silently contributes no print coverage.

## API Extraction Approach

Patina uses **behavioral oracle capture** rather than C++ source parsing to
extract API metadata from upstream Godot. The approach:

1. **GDScript probes run inside Godot**: Scripts in this directory
   (`scene_tree_dumper.gd`, `property_dumper.gd`, etc.) execute within a
   running Godot instance to capture actual runtime behavior.

2. **Machine-readable JSON output**: Each probe produces a structured JSON
   envelope containing the captured data (scene trees, property values,
   notification sequences, signal traces, physics state, render output).

3. **No C++ source parsing**: We do **not** parse Godot's C++ source code or
   header files. The oracle captures what Godot *does*, not what its source
   says. This ensures behavioral fidelity even when internal implementation
   differs from documented contracts.

4. **Pinned upstream version**: All oracle captures are produced against a
   specific pinned Godot version/commit to ensure reproducibility. The version
   is recorded in each golden envelope.

5. **Repeatable generation**: Running `python3 tools/oracle/run_fixture.py`
   regenerates goldens from the current pinned Godot. The `run_all.sh` script
   runs the full suite.

### Available Capture Types

| Probe | GDScript | Python wrapper | Output |
|-------|----------|----------------|--------|
| Scene tree structure | `scene_tree_dumper.gd` | `scene_tree_dumper.py` | Node hierarchy, classes, paths |
| Property values | `property_dumper.gd` | `property_dumper.py` | Per-node property key/value pairs |
| Signal emissions | `signal_tracer.gd` | `signal_tracer.py` | Signal name, emitter, args per frame |
| Notifications | `notification_tracer.gd` | `notification_tracer.py` | Notification IDs and ordering |
| Resource roundtrip | `resource_roundtrip.gd` | `resource_roundtrip.py` | Serialized resource fidelity check |
| Render capture | `render_capture.gd` | `render_capture.py` | Framebuffer pixel data (PPM/PNG) |
| Physics trace | — | `physics_tracer.py` | Per-frame body positions and contacts |
| Frame trace | `frame_trace_capture.gd` | `frame_trace_capture.py` | Per-frame processing order and state |
| Combined fixture | `run_fixture.gd` | `run_fixture.py` | All of the above in one envelope |

### Why Not Parse C++ Source?

- Godot's C++ implementation details change between versions without affecting
  public behavior. Source parsing would produce false positives on internal
  refactors.
- Runtime capture guarantees we test against the same behavior users observe.
- GDScript probes are simpler to maintain than a C++ AST parser.
- The oracle approach naturally supports testing against multiple Godot versions
  by re-running probes against each pinned version.

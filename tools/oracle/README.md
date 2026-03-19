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

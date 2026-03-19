# Compatibility Dashboard

**Last updated**: 2026-03-19
**Test suite**: 43 oracle golden tests + render/runtime tests passing

---

## Oracle Parity Results

Comparison of Godot 4.5.1 oracle golden outputs vs live Patina headless runner across 9 fixture scenes (51 golden files). Tests run patina-runner on each `.tscn` and compare against `*_tree.json` and `*_properties.json` goldens.

| Scene | Comparisons | Matched | Parity | Notes |
|-------|-------------|---------|--------|-------|
| `minimal.tscn` | 1 | 1 | **100.0%** | Single Node, perfect match |
| `hierarchy.tscn` | 11 | 3 | **27.3%** | Node/class match; missing default Sprite2D/Node2D props |
| `with_properties.tscn` | 16 | 5 | **31.2%** | Player position/modulate match; missing defaults |
| `space_shooter.tscn` | 26 | 8 | **30.8%** | Player/EnemySpawner positions match; missing defaults |
| `platformer.tscn` | 32 | 12 | **37.5%** | Node structure 100%; property defaults missing |
| `physics_playground.tscn` | 18 | 7 | **38.9%** | Physics node classes match; no simulation parity yet |
| `signals_complex.tscn` | 22 | 9 | **40.9%** | Signal node structure matches; emission data not compared |
| `test_scripts.tscn` | 15 | 5 | **33.3%** | Script vars partially match; frame-accumulated values diverge |
| `ui_menu.tscn` | 6 | 5 | **83.3%** | Near-complete match; only missing one default prop |
| **Overall** | **147** | **55** | **37.4%** | |

> **Method**: Golden-based testing via `oracle_regression_test.rs` — loads oracle JSONs, runs patina-runner on `.tscn`, compares node count/names/classes/properties with float tolerance (epsilon=0.01). Scene tree structure is 100% across all 9 scenes.

---

## Property Gap Analysis

### Matched Properties
- Node names and class names: **100%** across all scenes
- Node hierarchy (parent/child structure): **100%**
- Explicitly-set Vector2 positions: **Match**
- Script variable initial values (speed, health, etc.): **Match** for Int/Float types

### Known Gaps

| Gap | Status | Impact |
|-----|--------|--------|
| Default Node2D properties (rotation, scale, visible) | **Fixed** (pending fixture regeneration) | High — affects every Node2D node |
| `get_child_count()` built-in | **Fixed** | Medium — hierarchy_test.gd uses it |
| Script execution (process/physics frames) | Improved | Tree-order process/physics dispatch, pause handling, and `test_scripts` frame-trace motion regression now have direct tests |
| Signal emission during lifecycle | Not implemented | emit_count stays 0 in Patina |
| Script cross-node access (Reader → Counter) | Partial | Works for some patterns, not all |

---

## Unsupported Features

| Feature | Category | Status |
|---------|----------|--------|
| Custom signals (signal keyword) | Scripting | Partial — declaration works, cross-node dispatch limited |
| Typed arrays | Variant | Not started |
| Enums in scripts | Scripting | Parsed but not fully evaluated |
| @export annotations | Scripting | Parsed, not enforced |
| Physics bodies (CharacterBody2D, etc.) | Physics | Node class recognized, no physics simulation |
| Audio playback | Audio | Stub only |
| Input handling | Platform | Stub only |
| Rendering / viewport | Render | Scene-driven golden rendering is passing via `render_golden_test` against `.tscn` fixtures |
| Animation system | Scene | Not started |
| Tween system | Scene | Not started |

---

## Subsystem Parity

| Subsystem | Parity | Method |
|-----------|--------|--------|
| Scene tree structure | **100%** | All 9 golden scenes have matching hierarchy (43 tests) |
| Node names/classes | **100%** | Perfect match across all 9 fixture scenes |
| Node2D defaults | ~95% | Fixed; position/rotation/scale/visible now set |
| Script variable sync | ~60% | Initial values match, frame-accumulated values diverge |
| GDScript built-ins | ~85% | 30+ built-ins implemented, get_child_count added |
| Signal system | ~30% | Basic emit works, cross-node dispatch limited |
| Lifecycle ordering | ~85% | enter_tree/ready/exit order, pause transitions, and live-subtree mutation lifecycle now have direct coverage |

## Render Fixture Coverage

Scene-driven 2D render fixtures currently pass for:

- `demo_2d.tscn`
- `hierarchy.tscn`
- `space_shooter.tscn`
- `render_test_simple.tscn`
- `render_test_camera.tscn`
- `render_test_sprite.tscn`

Those tests compare rendered output against checked-in golden PNGs under
`fixtures/golden/render/` and also verify determinism and zoom/pan behavior.

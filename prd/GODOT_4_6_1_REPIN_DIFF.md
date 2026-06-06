# Godot 4.6.1 Repin Diff Report

**Date**: 2026-03-20
**Bead**: pat-9j0
**Previous pin**: Godot 4.5.1-stable (f62fdbde15035c5576dad93e586201f4d41ef0cb)
**New pin**: Godot 4.6.1-stable (14d19694e0c88a3f9e82d899a0400f27a24c176e)
**Pre-repin snapshot**: `fixtures/oracle_outputs/.pre_repin_snapshot.json`

---

## Overall Summary

| Metric | 4.5.1 | 4.6.1 | Delta |
|--------|-------|-------|-------|
| Total oracle comparisons | 63 | 71 | +8 |
| Matched properties | 57 | 59 | +2 |
| Parity % | 90.5% | 83.1% | -7.4pp |
| Scenes at 100% | 7/9 | 7/9 | unchanged |
| Oracle output files | 69 | 69+ new | see below |

The overall parity percentage dropped because Godot 4.6.1's richer oracle capture now exposes 8 additional script-exported properties that Patina does not yet emit. In absolute terms, matched properties increased from 57 to 59.

---

## Per-Fixture Breakdown

### Improved

| Scene | 4.5.1 | 4.6.1 | Change | Root cause |
|-------|-------|-------|--------|------------|
| `physics_playground.tscn` | 10/15 (66.7%) | 12/12 (100.0%) | +33.3pp | Godot 4.6.1 oracle outputs now align with Patina's collision_mask handling. Comparison count dropped from 15 to 12 (fewer default properties captured), and all 12 now match. |

### Regressed

| Scene | 4.5.1 | 4.6.1 | Change | Root cause |
|-------|-------|-------|--------|------------|
| `space_shooter.tscn` | 8/8 (100.0%) | 8/13 (61.5%) | -38.5pp | 5 new script-exported properties captured by 4.6.1 oracle |
| `test_scripts.tscn` | 4/5 (80.0%) | 4/11 (36.4%) | -43.6pp | 6 new script-exported properties + 1 existing position divergence |

### Unchanged (100% parity maintained)

| Scene | Comparisons | Parity |
|-------|-------------|--------|
| `minimal.tscn` | 1/1 | 100.0% |
| `hierarchy.tscn` | 3/3 | 100.0% |
| `with_properties.tscn` | 5/5 | 100.0% |
| `platformer.tscn` | 12/12 | 100.0% |
| `signals_complex.tscn` | 9/9 | 100.0% |
| `ui_menu.tscn` | 5/5 | 100.0% |

---

## New Unmatched Properties (4.6.1 regressions)

### space_shooter.tscn (+5 unmatched)

These are script-exported variables that Godot 4.6.1's oracle now captures but Patina does not yet export:

| Node | Property | Godot value | Patina |
|------|----------|-------------|--------|
| `/root/SpaceShooter/Player` | `speed` | `float: 200.0` | (missing) |
| `/root/SpaceShooter/Player` | `can_shoot` | `bool: true` | (missing) |
| `/root/SpaceShooter/Player` | `shoot_cooldown` | `float: 0.0` | (missing) |
| `/root/SpaceShooter/EnemySpawner` | `spawn_interval` | `float: 2.0` | (missing) |
| `/root/SpaceShooter/EnemySpawner` | `spawn_timer` | `float: 0.0` | (missing) |

### test_scripts.tscn (+6 unmatched, 1 pre-existing)

New script-exported variables exposed by 4.6.1 oracle:

| Node | Property | Godot value | Patina | Status |
|------|----------|-------------|--------|--------|
| `/root/TestScene/Mover` | `direction` | `float: 1.0` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/Mover` | `speed` | `float: 50.0` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/VarTest` | `health` | `int: 100` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/VarTest` | `is_alive` | `bool: true` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/VarTest` | `name_str` | `String: "Player"` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/VarTest` | `velocity` | `Vector2: (0,0)` | (missing) | NEW in 4.6.1 |
| `/root/TestScene/Mover` | `position` | `Vector2: (100,200)` | `Vector2: (100.83,200)` | Pre-existing divergence (frame accumulation) |

---

## Improvement Detail: physics_playground

Under Godot 4.5.1, the oracle captured 15 properties for this scene. Five of these were Godot-internal default values that `class_defaults.rs` filtering could not fully exclude, causing mismatches. Under 4.6.1, the oracle captures 12 properties (all explicitly set in the .tscn), and all 12 match Patina's output. The improvement reflects better oracle capture fidelity, not a change in Patina's runtime behavior.

---

## Oracle Infrastructure Changes

Files modified as part of the 4.6.1 repin (pat-5d6):

- `tools/oracle/common.py` -- updated version references
- `tools/oracle/run_fixture.gd` -- updated for 4.6.1 compatibility
- `upstream/godot` submodule -- advanced to 4.6.1-stable tag
- All 20 scene oracle outputs in `fixtures/oracle_outputs/` regenerated
- 8 trace golden files in `fixtures/golden/traces/` regenerated

---

## Remediation Path

The 11 unmatched properties (5 in space_shooter, 6 in test_scripts) are all script-exported variables (`@export var` or bare `var` declarations in GDScript). To close these gaps:

1. **Script variable export parity**: Patina's GDScript interop layer needs to surface script-declared variables as node properties in the oracle comparison path.
2. **Mover position divergence**: The `position` mismatch on `test_scripts/Mover` is a pre-existing issue where Patina accumulates one extra frame of movement (100.0 vs 100.833...). This requires investigation of frame-0 `_process` timing.

Closing all 11 gaps would bring overall parity to 98.6% (70/71), with only the Mover position divergence remaining.

---

## Test Verification

- Oracle parity tests: 32/32 passing
- Oracle regression tests: 43/43 passing
- Full workspace: `cargo test --workspace` passes

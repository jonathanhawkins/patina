# Repin Diff Report: Godot 4.5.1 -> 4.6.1

**Date**: 2026-03-22 (refreshed with expanded corpus)
**Upstream commit**: 14d19694e0c88a3f9e82d899a0400f27a24c176e (v4.6.1.stable.official)
**Oracle scenes**: 16 | **Comparisons**: 221 | **Matched**: 180 | **Parity**: 81.4%

---

## Per-Fixture Parity (Current — 2026-03-22)

### 2D Fixtures (Original 9 scenes)

| Fixture | Parity | Notes |
|---------|--------|-------|
| `minimal.tscn` | 100% (1/1) | Unchanged |
| `hierarchy.tscn` | 100% (3/3) | Unchanged |
| `with_properties.tscn` | 100% (5/5) | Unchanged |
| `platformer.tscn` | 100% (12/12) | Unchanged |
| `signals_complex.tscn` | 100% (9/9) | Unchanged |
| `ui_menu.tscn` | 100% (5/5) | Unchanged |
| `physics_playground.tscn` | 100% (12/12) | Improved from 66.7% (4.5.1) |
| `space_shooter.tscn` | 92.3% (12/13) | 1 script-exported property gap |
| `test_scripts.tscn` | 90.9% (10/11) | 1 position drift on Mover node |

### 3D Fixtures (New in expanded corpus)

| Fixture | Parity | Notes |
|---------|--------|-------|
| `minimal_3d.tscn` | 58.3% (7/12) | Camera3D `current`, Transform3D basis gaps |
| `hierarchy_3d.tscn` | 64.7% (11/17) | Transform3D format normalization needed |
| `indoor_3d.tscn` | 59.3% (16/27) | Light precision, camera gaps |
| `multi_light_3d.tscn` | 69.0% (20/29) | Light3D precision, shadow hints |
| `physics_3d_playground.tscn` | 74.2% (23/31) | Highest 3D parity |
| `physics_playground_extended.tscn` | 100% (26/26) | Full match |
| `signal_instantiation.tscn` | 100% (8/8) | Full match |

---

## Parity Trajectory

| Phase | Scenes | Parity | Key change |
|-------|--------|--------|------------|
| Pre-repin (4.5.1) | 9 | 90.5% (57/63) | Baseline |
| Post-repin (4.6.1, initial) | 9 | 83.1% (59/71) | Script-var comparisons exposed |
| Post-fix (commit 952bacf) | 9 | 97.2% (69/71) | Script-var merge fix |
| 2D Final (commit b478047) | 9 | 100.0% (71/71) | All 2D gaps closed |
| **Expanded corpus (current)** | **16** | **81.4% (180/221)** | +7 scenes (5 3D + 2 special), 3D gaps known |

> **Note**: The 2D-only 9-scene subset previously reported 100% (71/71). The expanded 16-scene corpus adds 3D fixtures with known gaps, reducing the overall percentage while tripling absolute coverage.

---

## Known 3D Gaps

| Domain | Gap | Impact |
|--------|-----|--------|
| Camera3D | `current` property not emitted | Affects all 3D scenes with cameras |
| Transform3D | Basis format mismatch (flat array vs nested xyz) | Structural — needs format normalization |
| Light3D | Float precision (e.g., 0.8 vs 0.800000011920929) | Cosmetic — within tolerance |
| Light3D | `shadow_enabled` hint value differs (0 vs 42) | Metadata-only, no runtime effect |

---

## Verification

- `cargo test --test lifecycle_trace_oracle_parity_test` — 18/18 pass (all oracle scenes)
- `cargo test --test repin_diff_report_test` — 30/30 pass (report validation)
- `cargo test --test physics3d_trace_comparison_test` — 13/13 pass (3D deterministic traces)
- `cargo test --test resource_scene_broad_execution_test` — 39/39 pass (resource/scene coverage)

---

## Detailed Reports

- Per-fixture diff: [prd/GODOT_4_6_1_REPIN_DIFF.md](prd/GODOT_4_6_1_REPIN_DIFF.md)
- Full parity report: [fixtures/oracle_outputs/PARITY_REPORT.md](fixtures/oracle_outputs/PARITY_REPORT.md)
- Upstream changelog audit: [prd/GODOT_4_6_1_RELEASE_DELTA_AUDIT.md](prd/GODOT_4_6_1_RELEASE_DELTA_AUDIT.md)
- Repin work queue: [prd/GODOT_4_6_1_REPIN_BEADS.md](prd/GODOT_4_6_1_REPIN_BEADS.md)

# Parity Closure Beads

These beads come directly from `prd/GODOT_4_5_1_FEATURE_AUDIT.md`.

Purpose:

- close the gap between the current measured 2D/runtime slice and a credible `Godot 4.5.1` parity claim
- finish the oracle/runtime critical path before any `4.6.1` repin
- reconcile docs and tracker state so repin reporting is honest

## Existing Critical Path

These already exist and remain the front of the queue:

- `pat-i5c` Make frame processing semantics match Godot contracts
- `pat-gnt` Generate upstream frame-trace golden for `test_scripts`
- `pat-9j5` Compare Patina frame traces against upstream frame-trace goldens
- `pat-b16` Add global lifecycle and signal ordering trace parity
- `pat-x8u` Finish scene-aware signal dispatch parity

## New Beads To Add To `br`

These are the missing audit-derived beads. Create them once the `br` database is repaired.

### P0

1. `Audit: reconcile docs, tests, and bead state before repin`
   Labels: `audit`, `docs`, `parity`
   Acceptance:
   - `COMPAT_MATRIX.md`, `COMPAT_DASHBOARD.md`, `EXIT_CRITERIA.md`, and `br` status agree
   - stale counts are fixed
   - the repin gate is stated explicitly in docs

2. `Raise oracle property parity beyond current 37.4% corpus baseline`
   Labels: `oracle`, `parity`
   Acceptance:
   - oracle tests pass against the current fixture corpus
   - published parity percentage is updated from the current 37.4% baseline
   - changed fixture/property coverage is documented

### P1

3. `Expand Node2D default-property oracle coverage`
   Labels: `object-model`, `oracle`, `parity`
   Acceptance:
   - default Node2D properties such as rotation, scale, and visibility have dedicated oracle-backed coverage
   - docs no longer describe those defaults as merely claimed

4. `Expand script frame-evolution oracle coverage`
   Labels: `scripting`, `oracle`, `runtime`
   Acceptance:
   - frame-evolution fixtures exist
   - upstream traces are checked in
   - Patina comparisons verify per-frame scripted state evolution

5. `Broaden scene-system oracle fixtures beyond current slice`
   Labels: `scene`, `oracle`, `parity`
   Acceptance:
   - new scene fixtures are checked in for scene tree, instancing, ownership, and cross-scene behavior
   - scene-system docs/test references reflect the broader corpus

6. `Broaden packed-scene and NodePath parity coverage`
   Labels: `packed-scene`, `scene`, `parity`
   Acceptance:
   - additional ownership, subresource, unique-name, and NodePath edge cases are covered
   - targeted tests or oracle fixtures justify the stronger parity claim

7. `Broaden signal parity beyond current targeted fixtures`
   Labels: `signals`, `parity`
   Acceptance:
   - additional signal ordering and wider scene-interaction cases are covered
   - parity assertions are explicit and cited in docs

8. `Broaden notification and lifecycle parity beyond current targeted fixtures`
   Labels: `notifications`, `parity`
   Acceptance:
   - additional lifecycle and notification scenarios are measured
   - compatibility docs cite the added evidence

9. `Broaden resource parity coverage for edge-case fidelity`
   Labels: `resources`, `parity`
   Acceptance:
   - resource loader/cache/UID edge cases gain added fixtures or targeted tests
   - resource status is justified as more than slice-level confidence

10. `Broaden GDScript interop parity coverage`
    Labels: `scripting`, `parity`
    Acceptance:
    - additional script-visible behavior is measured against fixtures or explicit parity contracts
    - interop status can be defended beyond current demo-scene coverage

### P2

11. `Measure platform/windowing parity for current runtime scope`
    Labels: `platform`, `parity`
    Acceptance:
    - explicit parity tests or goldens exist for the supported window lifecycle and display behavior
    - or docs are narrowed so platform/windowing stays out of the parity claim

12. `Reconcile audio status docs with current stub implementation`
    Labels: `audio`, `docs`
    Acceptance:
    - compatibility docs and milestone docs report the current `gdaudio` test count
    - audio is clearly described as a tested stub with no Godot parity claim

13. `Reconcile CI and benchmark reporting before repin`
    Labels: `ci`, `benchmarks`, `docs`
    Acceptance:
    - `EXIT_CRITERIA.md` and benchmark docs match the actual workflow and artifact expectations
    - stale CI/benchmark claims are removed

## Dependency Shape

Use this dependency structure when importing them into `br`:

- `pat-i5c` -> `pat-gnt` -> `pat-9j5`
- `pat-i5c` -> `Expand script frame-evolution oracle coverage`
- `pat-i5c` -> `Broaden scene-system oracle fixtures beyond current slice`
- `pat-b16` -> `Broaden notification and lifecycle parity beyond current targeted fixtures`
- `pat-x8u` -> `Broaden signal parity beyond current targeted fixtures`
- `Raise oracle property parity beyond current 37.4% corpus baseline` depends on:
  - `pat-9j5`
  - `Expand Node2D default-property oracle coverage`
  - `Expand script frame-evolution oracle coverage`
  - `Broaden scene-system oracle fixtures beyond current slice`
  - `Broaden packed-scene and NodePath parity coverage`
  - `Broaden signal parity beyond current targeted fixtures`
  - `Broaden notification and lifecycle parity beyond current targeted fixtures`
  - `Broaden resource parity coverage for edge-case fidelity`
  - `Broaden GDScript interop parity coverage`
- `Audit: reconcile docs, tests, and bead state before repin` depends on:
  - `Raise oracle property parity beyond current 37.4% corpus baseline`
  - `Reconcile audio status docs with current stub implementation`
  - `Reconcile CI and benchmark reporting before repin`
  - `Measure platform/windowing parity for current runtime scope`

## Notes

- Audio and editor are not parity blockers. They should not be expanded into major parity epics for the `4.5.1` closure pass.
- Platform/windowing is still a real reporting gap. Either measure it for the supported runtime scope or keep it out of the parity claim.
- Do not repin to `4.6.1` until this set and the existing runtime/oracle critical path are reconciled.

## Tracker Issue

`br create` is currently blocked by a tracker data integrity problem:

- existing issue `pat-0lo` reports `closed_at` before `created_at`

Repair the tracker first, then import the bead list above into `br`.

# Godot 4.6.1 Repin Beads

Patina is now pinned to `Godot 4.6.1-stable` (`14d19694e0c88a3f9e82d899a0400f27a24c176e`).

Important constraint:

- the repo pin and docs now point at `4.6.1-stable`
- the published parity percentages and goldens are still historical until the oracle outputs are regenerated and rerun against `4.6.1`

This bead pack is the post-repin work queue for multiple teams.

## P0: Establish The New Baseline

1. `Regenerate all upstream oracle outputs against Godot 4.6.1`
   Acceptance:
   - scene/resource/frame-trace outputs are regenerated with the new upstream pin
   - generated artifacts replace the old oracle baseline cleanly

2. `Refresh Patina-vs-Godot 4.6.1 oracle parity metrics`
   Acceptance:
   - oracle parity and regression tests run against the refreshed outputs
   - a measured `4.6.1` parity baseline is recorded

3. `Publish a 4.6.1 repin diff report by fixture and property`
   Acceptance:
   - each changed fixture/property bucket is listed
   - regressions and improvements are separated

4. `Update compatibility docs to distinguish historical 4.5.1 numbers from live 4.6.1 numbers`
   Acceptance:
   - `COMPAT_*`, `TEST_ORACLE.md`, and repin docs no longer imply that pre-repin percentages are current

## P1: Runtime Fallout Lanes

5. `Resolve 4.6.1 runtime diffs in physics_playground`
   Acceptance:
   - the remaining `physics_playground` property differences are explained, fixed, or bounded

6. `Resolve 4.6.1 runtime diffs in test_scripts frame evolution`
   Acceptance:
   - frame-evolution mismatches in `test_scripts` are explained, fixed, or bounded

7. `Revalidate %UniqueName and NodePath behavior against 4.6.1`
   Acceptance:
   - the new `%UniqueName` implementation is confirmed against `4.6.1` oracle behavior

8. `Revalidate scene lifecycle and notification ordering against 4.6.1`
   Acceptance:
   - lifecycle/notification ordering remains compatible under the new oracle outputs

9. `Revalidate signal ordering and dispatch behavior against 4.6.1`
   Acceptance:
   - signal traces and dispatch expectations still hold against the repinned target

10. `Revalidate class-default filtering and explicit-property comparison under 4.6.1`
    Acceptance:
    - `class_defaults.rs` filtering is still correct for the repinned target
    - no new false positives dominate parity reporting

11. `Revalidate resource loading, UID, and subresource behavior against 4.6.1`
    Acceptance:
    - resource parity remains justified against the repinned oracle

## P1: Goldens And Fixtures

12. `Refresh render goldens after the 4.6.1 repin`
    Acceptance:
    - render fixtures are rerun and any changed images are reviewed and documented

13. `Refresh physics trace goldens after the 4.6.1 repin`
    Acceptance:
    - deterministic trace fixtures are rerun and any changed traces are reviewed and documented

14. `Refresh frame-trace and lifecycle-trace goldens after the 4.6.1 repin`
    Acceptance:
    - trace fixtures are rerun and any changed ordering/state is reviewed and documented

## P2: Tooling, Editor, And CI Fallout

15. `Verify apps/godot GDExtension lab against 4.6.1`
    Acceptance:
    - the Godot lab still loads and emits probe output under `4.6.1`

16. `Revalidate editor REST API parity suite against 4.6.1-backed runtime behavior`
    Acceptance:
    - editor tests still pass when backed by the repinned runtime/oracle expectations

17. `Update benchmark baselines after 4.6.1 repin`
    Acceptance:
    - benchmark docs or artifacts reflect the repinned target where needed

18. `Add CI lane for repin regeneration and parity refresh`
    Acceptance:
    - CI has a clear path for oracle refresh and parity validation on the repinned target

19. `Write 4.6.1 release-delta audit for Patina-facing behavior changes`
    Acceptance:
    - observed behavior changes are summarized as Patina-impacting deltas, not vague release-note prose

## Parallelization Shape

- Team 1: `P0` oracle regeneration and parity measurement
- Team 2: runtime fallout in `physics_playground` and `test_scripts`
- Team 3: NodePath / signals / notifications / resource revalidation
- Team 4: render / physics / trace golden refresh
- Team 5: editor lab / editor REST / CI / docs fallout

## Rule

Do not claim new `4.6.1` parity percentages until `P0` is complete.

# Post-Repin Execution Map

This file is the operating map after the `4.6.1-stable` repin.

Current reality:

- tracker was previously driven to zero open beads
- script-variable merge fix (commit 952bacf) recovered parity to **97.2% (69/71)**
- 2 bounded drifts remain on `test_scripts/Mover/position` (frame accumulation)
- runtime recovery is substantially complete; docs reconciliation in progress

## Rule

Do not claim the repin is complete while the following runtime fallout beads remain open:

- `pat-epha`
- `pat-2po1`
- `pat-5hba`
- `pat-hzgp`
- `pat-ztah`
- `pat-q16u`

## Five-Team Layout

### Team 1: Core Runtime Property Export

Goal:

- make script variables visible through Patina's oracle/property surface

Claim order:

1. `pat-epha` 4.6.1 fallout: export script variables into oracle/property surface
2. `pat-ztah` 4.6.1 fallout: add regression tests for script-variable property export

### Team 2: `space_shooter` Fixture Recovery

Goal:

- recover the script-property regressions in `space_shooter`

Claim order:

1. wait for `pat-epha`
2. claim `pat-2po1`

### Team 3: `test_scripts` Fixture Recovery

Goal:

- recover the script-property regressions and frame drift in `test_scripts`

Claim order:

1. wait for `pat-epha`
2. claim `pat-5hba`
3. claim `pat-hzgp`

### Team 4: Oracle/Reporting Reconciliation

Goal:

- publish the real post-repin status once runtime fallout is fixed

Claim order:

1. wait for `pat-2po1`, `pat-5hba`, `pat-hzgp`, `pat-ztah`
2. claim `pat-q16u`
3. claim `pat-judw`

### Team 5: Docs/Policy Cleanup

Goal:

- remove contradictory repin claims and keep editor claims secondary

Claim order:

1. wait for `pat-q16u`
2. claim `pat-gyxm`
3. claim `pat-8opj`
4. claim `pat-wgjm`

## Do Not Do Yet

- new editor feature expansion
- new 3D scope work
- new parity claims in release notes

Those are downstream of runtime fallout recovery.

## Required Reporting Format

Every team report should start with:

`BEAD: <id>`

and include:

- what changed
- what test proves it
- what mismatch count moved, if any
- whether the next dependent bead is now unblocked

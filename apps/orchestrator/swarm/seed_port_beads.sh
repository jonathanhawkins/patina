#!/usr/bin/env bash
# No-op seed shim.
#
# The legacy bash seeder used to scan every PRD execution map and seed beads
# for any title not yet in the tracker. Now that the Rust planner is the single
# source of truth (driven by .orchestrator/planner.toml), this script is a
# stub.
#
# To restore the legacy behavior, copy seed_port_beads.sh.bak back over this
# file.

set -euo pipefail
echo "seed_port_beads.sh: no-op (Rust planner is the source of truth)"
exit 0

---
name: mail-reserve
description: Reserve files before editing to prevent conflicts with other agents. Advisory leases with TTL.
argument-hint: [file-patterns...]
---

# Reserve Files via Agent Mail

Claim advisory file reservations before editing to coordinate with other agents.

## Steps

1. Parse `$ARGUMENTS` for file paths/glob patterns to reserve.

2. Call `mcp__mcp-agent-mail__file_reservation_paths` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `agent_name`: your agent name (from session)
   - `paths`: the file patterns from arguments (e.g., `["engine-rs/crates/runtime/**/*.rs"]`)
   - `ttl_seconds`: 3600 (1 hour default, adjust if needed)
   - `exclusive`: true (default for editing)
   - `reason`: brief description of why (e.g., the bead ID you're working on)

3. Report results:
   - **Granted**: list of successfully reserved paths with expiry times
   - **Conflicts**: any paths that conflict with other agents' reservations — show who holds them

4. If conflicts exist, suggest:
   - Wait for the reservation to expire
   - Contact the holding agent via `/mail-send`
   - Use `force_release_file_reservation` only if the holder appears inactive

## Reservation Guidelines (from AGENTS.md)

- Reserve shared files: `AGENTS.md`, `Cargo.toml` (workspace root), `package.json`
- Crate-internal files generally don't need reservation
- Use specific patterns, not broad globs like `**/*`
- Renew with `/mail-renew` if you need more time (not yet a skill — use the tool directly)

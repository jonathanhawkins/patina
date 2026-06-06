# Patina Engine - Claude Code Configuration

## Project Overview
Patina is a monorepo for a Rust-native Godot-compatible game engine, its marketing website, and supporting tools.

## Key Paths
- `apps/web/` — Next.js website (patinaengine.com)
- `engine-rs/` — Rust engine workspace
- `apps/godot/` — GDExtension compatibility lab
- `tools/` — Development tooling
- `docs/` — Documentation and methodology
- `prd/` — Product requirements and plans

## Commands
- Website dev: `cd apps/web && pnpm dev`
- Website build: `cd apps/web && pnpm build`
- Engine build: `cd engine-rs && cargo build`
- Engine test: `cd engine-rs && cargo nextest run` (preferred) or `cargo test`
- Lint all: `pnpm lint` (root)

## Testing Rules (Non-Negotiable)
- Every bug fix MUST include a test that would have caught the bug
- Every new feature MUST include tests covering happy path AND edge cases
- Stress/concurrency tests required for any server or networking code
- Run `cargo nextest run --workspace` before every commit — never commit with failing tests
- Prefer `cargo nextest run` over `cargo test` — it runs tests in parallel and is significantly faster with 333+ integration test files
- If a test is flaky, fix the root cause — do not skip or ignore it

## Debugging Rules
- When debugging HTTP/browser issues, ALWAYS check browser console logs via Claude in Chrome (read_console_messages or javascript_tool) BEFORE attempting fixes
- When the editor server has errors, check BOTH server-side (Rust stderr) AND client-side (browser console) logs
- Never assume a fix works — verify in the browser with error tracking active for at least 15 seconds
- For network errors (ERR_CONNECTION_RESET, ERR_EMPTY_RESPONSE), the root cause is almost always server-side — check Rust panic/error output

## Editor Feature Gate — LIFTED (2026-03-19)
- **GATE LIFTED** — runtime parity exits are green. Editor feature work is the primary focus.
- All editor beads (UI, viewport, asset browser, shader editor, etc.) are available for implementation.
- DO NOT revert this gate — it was lifted because Gates 1-8 passed. See AGENTS.md.

## Important
- Always read AGENTS.md first for safety rules and conventions
- This project uses the Agent Flywheel methodology — see docs/

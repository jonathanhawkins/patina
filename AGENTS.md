# AGENTS.md - Patina Engine Operating Manual

## Project Purpose

Patina is a monorepo containing:

1. **Patina Engine** (`engine-rs/`) — A Rust-native, behavior-compatible Godot runtime built in staged vertical slices.
2. **Website** (`apps/web/`) — The patinaengine.com marketing/docs site built with Next.js, Tailwind CSS, and shadcn/ui, deployed to Cloudflare Workers.
3. **Godot Lab** (`apps/godot/`) — GDExtension compatibility lab using godot-rust for validation.
4. **Tools** (`tools/`) — Oracle dumpers, API extraction, fixture generation, render diffing, benchmarks.
5. **Fixtures** (`fixtures/`) — Scene, resource, physics, and render golden data.
6. **Tests** (`tests/`) — Compatibility, integration, golden, and performance test suites.
7. **Docs** (`docs/`) — Architecture docs, flywheel methodology, guides.

## Technology Stack

- **Engine**: Rust (2021 edition), Cargo workspace
- **Website**: Next.js 15, React 19, TypeScript, Tailwind CSS v4, shadcn/ui, Cloudflare Workers (via @cloudflare/next-on-pages)
- **Godot Lab**: Godot 4.x, godot-rust (GDExtension)
- **CI**: GitHub Actions
- **Package Manager**: pnpm (for JS/TS), Cargo (for Rust)
- **Monorepo Orchestrator**: Turborepo

## Safety Rules (Non-Negotiable)

1. **No destructive git commands**: Never run `git reset --hard`, `git clean -f`, `git push --force`, `git checkout .`, or `git branch -D` without explicit human approval.
2. **No file deletion without permission**: Do not delete files unless the bead explicitly authorizes it.
3. **No secrets in commits**: Never commit `.env`, credentials, API keys, or secrets.
4. **Unsafe Rust policy**: Every `unsafe` block must have a `// SAFETY:` comment explaining the invariant. Prefer safe abstractions.
5. **No skipping hooks**: Never use `--no-verify` or `--no-gpg-sign`.

## Coding Conventions

### Rust
- Use `rustfmt` defaults
- Use `clippy` with `#![warn(clippy::all)]`
- Prefer `thiserror` for error types
- Prefer `tracing` over `log`
- Keep crate boundaries strict — no circular dependencies
- Every public API must have a doc comment

### TypeScript/React
- Use TypeScript strict mode
- Use functional components with hooks
- Use shadcn/ui components — do not reinvent UI primitives
- Use Tailwind utility classes — no custom CSS unless absolutely necessary
- Use `next/image` for images, `next/link` for navigation

### General
- Prefer small, focused commits
- Every commit message should reference the bead it implements (if applicable)
- Write tests alongside implementation
- Keep PRs focused on a single bead or logical unit

## File Reservation Policy

- Before editing shared files (AGENTS.md, package.json, Cargo.toml workspace), announce intent via commit message or PR description.
- Crate-internal files do not require reservation.
- Website component files do not require reservation unless they are shared layout/config files.

## Recovery After Compaction

After every context compaction or session restart:
1. Re-read this file (`AGENTS.md`)
2. Check `git status` and `git log --oneline -10` to understand current state
3. Check task list or bead status for current assignments
4. Resume work on the current bead or claim a new one

## Bead Workflow

1. Check available beads/tasks
2. Claim one bead at a time
3. Implement with tests
4. Self-review ("fresh eyes") after completion
5. Mark bead as complete
6. Move to next priority bead

## Project-Specific Rules

- The Rust engine ports **behavior and contracts**, not C++ source files
- Upstream Godot is the behavioral oracle — never guess at behavior, verify against upstream
- Every compatibility test must state what observable behavior it checks
- Do not start reimplementing third-party code until classification is recorded in `THIRDPARTY_STRATEGY.md`
- No editor work until runtime milestones are stable

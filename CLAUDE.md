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
- Engine test: `cd engine-rs && cargo test`
- Lint all: `pnpm lint` (root)

## Important
- Always read AGENTS.md first for safety rules and conventions
- This project uses the Agent Flywheel methodology — see docs/

# Flywheel Glossary

This file explains the Flywheel/ACFS acronyms that show up in Patina docs and
agent prompts.

## Core Tasking

### `br` — beads_rust

The task database. `br` stores bead definitions, dependencies, claims, and
status changes.

Patina status: **installed and in active use**

### `bv` — beads_viewer

The prioritization and graph-analysis layer on top of `br`. It helps surface
the best ready work and critical paths.

Patina status: **installed**, but current orchestration is still relying
heavily on explicit execution maps and coordinator routing.

### Bead

A self-contained unit of work with acceptance criteria, dependencies, and
enough context for an agent to execute it.

## Coordination

### Agent Mail

The directed messaging and advisory file-reservation layer. It gives each agent
an identity and lets agents hand work off to each other by thread.

Patina status: **installed and now functioning as the coordination layer**

### Reservation

An advisory lock on files or paths announced through Agent Mail. Reservations
help prevent multiple agents from editing the same shared surface at once.

## Flywheel / Setup

### `ACFS` — Agentic Coding Flywheel Setup

The broader upstream machine/setup bundle from the Flywheel ecosystem. It is
the environment/bootstrap layer that can install or manage supporting tools
like tmux helpers, command guards, and shell integration.

Think: **the setup toolkit**

Patina status: **partially adopted**. The local machine now has most of the
portable stack tools, but Patina is still not a full upstream ACFS bootstrap
because the Ubuntu/VPS-level `acfs` layer itself is not installed here.

### `NTM` — Named Tmux Manager

A tmux fleet manager from the Flywheel ecosystem. It is meant to help spawn,
name, message, attach to, and kill multiple agent sessions cleanly.

Think: **tmux swarm control**

Patina status: **installed**. `ntm` is available locally and shell integration
has been added to `~/.zshrc`.

### `DCG` — Destructive Command Guard

A shell-level guard that blocks dangerous commands like `git reset --hard`,
`git clean -f`, and force pushes unless explicitly approved.

Think: **mechanical safety rail**

Patina status: **installed**. `dcg doctor` passes locally and the Claude Code
hook layer is wired.

## Memory / Reporting

### `CASS` — Context-Aware Session Summaries

A cross-session memory layer that mines previous agent sessions for repeated
failures, useful patterns, and lessons learned.

Think: **project memory**

Patina status: **installed locally as `cm` and `cass`**, but still at an early
adoption stage in repo workflow terms.

### `UBS` — Unified Bead Status

A higher-level status/reporting layer over the bead graph. It summarizes
progress, blockers, completion rates, and project velocity beyond raw `br`
output.

Think: **project dashboard**

Patina status: **installed locally as `ubs`**, with repo-level `.ubsignore`
added.

## Practical Summary

The Flywheel stack is easiest to understand as:

- `ACFS` sets up the environment
- `NTM` manages tmux agent sessions
- `br` stores tasks
- `bv` prioritizes tasks
- Agent Mail coordinates agents
- `CASS` remembers prior lessons
- `UBS` summarizes overall progress
- `DCG` blocks dangerous commands

For Patina today, the pieces that are truly live are:

- `br`
- `bv`
- Agent Mail
- `ntm`
- `cass`
- `dcg`
- `ubs`
- `slb`
- `ru`
- `cm`
- `caam`
- tmux
- repo-local orchestration docs and hooks

The pieces that are still only partial or missing are:

- full `ACFS` bootstrap layer / `acfs` command

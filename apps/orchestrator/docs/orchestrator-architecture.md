# Orchestrator Architecture

## Purpose

The orchestrator exists because the current Flywheel stack gives Patina most of
the pieces for parallel agent execution, but not a robust always-on dispatcher.

Patina needs a local control plane that can keep a worker swarm moving toward
runtime parity with minimal manual tending.

## Problem Statement

Today the repo already has:

- `br` for task tracking
- `bv` for graph-aware triage
- Agent Mail for coordination
- `tmux` and `ntm` for worker sessions

What is still missing is a reliable coordinator loop that:

- consumes worker completion reports quickly
- closes or reopens beads deterministically
- assigns the next ready bead immediately
- keeps the visible swarm wall saturated

Without that layer, the human operator still has to hunt for idle panes even
when open work remains.

## Design Goals

1. Single-writer tracker discipline
   Only the orchestrator mutates `br`.

2. Read-only workers
   Worker panes execute bead work and report status, but do not own tracker
   state changes.

3. Fast refill
   Finished panes should receive their next assignment immediately.

4. Tool compatibility
   Preserve compatibility with `br`, `bv`, Agent Mail, and `ntm`.

5. Rebuildable state
   Derived runtime state should be safe to throw away and regenerate.

## High-Level Model

```text
workers -> Agent Mail / pane status -> orchestrator -> br/bv -> next bead -> workers
```

The orchestrator acts as:

- dispatcher
- tracker writer
- session supervisor
- operator UI backplane

## Planned Runtime Pieces

### 1. Session Supervisor

Responsibilities:

- maintain named tmux / `ntm` sessions
- ensure lane identity per pane
- restart dead panes intentionally
- keep the wall layout stable

### 2. Worker State Observer

Responsibilities:

- inspect pane output
- detect likely-done states
- detect likely-stalled states
- correlate panes with bead IDs

### 3. Assignment Engine

Responsibilities:

- query ready work from `bv` / `br`
- prefer highest-value ready beads
- honor lane affinity when useful
- avoid duplicate assignment

### 4. Tracker Writer

Responsibilities:

- serialize `br update` / `br close` operations
- reconcile drift between pane reality and tracker state
- rebuild tracker sidecar state if required

### 5. Operator Surface

Responsibilities:

- present active workers
- present current ready/open work
- expose recent completions
- allow manual overrides from the boss pane

## Near-Term Plan

### Phase 1

Codify the orchestrator area and centralize session scripts.

### Phase 2

Implement a local coordinator loop that:

- polls pane state
- identifies finished panes
- assigns next ready beads

### Phase 3

Replace ad hoc status polling with explicit worker reports via Agent Mail.

### Phase 4

Add policy around bead saturation targets, idle thresholds, and automatic lane
refill.

## Non-Goals

- replacing `br` immediately
- embedding orchestration logic into `engine-rs/`
- making Patina-specific engine code depend on the orchestrator

## Open Question

If `br` continues to be unstable under this workload even with a strict
single-writer orchestrator, then the next step is likely:

- a separate upstream issue against `beads_rust`, or
- a compatibility-preserving fork with better single-writer semantics

That decision should happen after the orchestrator proves the real bottleneck.

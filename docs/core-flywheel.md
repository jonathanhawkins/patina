# Core Flywheel Operating Guide

This document describes the operational mechanics of the Agent Flywheel methodology as applied to the Patina Engine project. It covers the integrated toolchain, essential terminology, the six-stage operating loop, human and agent responsibilities, and practical getting-started guidance.

---

## The Three Integrated Tools

The Flywheel runs on three tools that work together to provide task management, prioritization, and inter-agent communication.

### 1. br (beads_rust)

The durable task management backend. `br` stores bead definitions, dependency graphs, status transitions, and metadata. Every unit of work in the project is a bead managed by `br`.

- Repository: [https://github.com/Dicklesworthstone/beads_rust](https://github.com/Dicklesworthstone/beads_rust)
- Role: Create, update, query, and close beads. Maintains the authoritative task graph.
- Key commands: `br create`, `br list`, `br claim`, `br complete`, `br status`

### 2. bv (beads_viewer)

The graph-theory routing and visualization layer. `bv` reads the bead graph from `br` and computes optimal task priority using PageRank, betweenness centrality, and dependency analysis.

- Repository: [https://github.com/Dicklesworthstone/beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (also available as `beads_viewer_rust`)
- Role: Compute priority ordering, identify critical paths, generate triage reports, visualize the dependency graph.
- Key commands: `bv --robot-triage`, `bv --critical-path`, `bv --next`
- When multiple agents each independently query `bv` for priority, emergent coordination occurs without centralized scheduling.

### 3. Agent Mail

The targeted messaging system for inter-agent communication. Agent Mail provides identity, directed messaging, and advisory file reservations.

- Role: Send messages between specific agents (not broadcast), announce file reservations with TTL expiry, coordinate handoffs, and flag blockers.
- Messages are targeted: agent-to-agent, not broadcast channels.
- File reservations are advisory but enforced by convention and pre-commit guards.

---

## Five Essential Terms

| Term | Definition |
|------|-----------|
| **Bead** | A self-contained unit of work with a description, acceptance criteria, dependencies, and sufficient context for an agent to execute without external reference. Analogous to a Jira ticket but optimized for AI agent execution. |
| **Ready Bead** | A bead whose dependencies are all satisfied and which is available to be claimed. Only ready beads should be picked up for implementation. |
| **Claim** | The act of an agent taking ownership of a ready bead. Only one agent should claim a bead at a time. Claims are tracked in `br` and communicated via Agent Mail. |
| **Reservation** | An advisory lock on one or more files, announced via Agent Mail with a TTL. Reservations prevent edit collisions when multiple agents work in the same codebase. They are not rigid locks but are enforced by convention and pre-commit guards. |
| **Thread** | A single agent session from start to compaction or termination. A thread reads AGENTS.md, claims beads, implements, and reviews. After compaction, a new thread begins by re-reading AGENTS.md and resuming. |

---

## The Six-Stage Operating Loop

The Flywheel operates as a continuous loop through six stages. Each cycle produces completed beads and feeds learning back into subsequent cycles.

### Stage 1: Plan

**Owner**: Human (with AI assistance for drafting)

- Produce or refine the master plan as a comprehensive markdown document.
- Synthesize competing plans from multiple frontier models into a single superior hybrid.
- Iterate through 4-5+ refinement rounds until the plan reaches sufficient detail (typically 3,000-6,000+ lines).
- The plan describes architecture, workflows, interactions, and system-wide decisions.

### Stage 2: Encode

**Owner**: Human + AI agent (typically Claude Code Opus)

- Convert the plan into beads with full dependency graphs.
- Each bead must include: detailed description, testing specifications, acceptance criteria, explicit dependencies, and sufficient embedded context.
- A typical complex project generates 200-500 beads.
- Polish beads through 4-6+ refinement rounds ("check your beads N times, implement once").

### Stage 3: Triage

**Owner**: `bv` (automated) + Human oversight

- `bv` computes priority ordering from the dependency graph using PageRank and betweenness centrality.
- Identify the critical path and surface the highest-impact ready beads.
- Human reviews triage output and adjusts priorities when strategic considerations override graph analysis.
- Run `bv --robot-triage` to generate the current priority report.

### Stage 4: Coordinate

**Owner**: Agent Mail + Human monitoring

- Agents claim ready beads based on `bv` priority.
- File reservations are announced via Agent Mail before editing shared areas.
- Agents stagger launch (30+ seconds apart) to avoid thundering-herd contention.
- Human monitors for collisions, blockers, and drift.

### Stage 5: Implement

**Owner**: AI agents (fungible generalists)

- Each agent reads AGENTS.md, claims a bead, implements with tests, and performs fresh-eyes self-review.
- All agents work on `main` branch (single-branch model).
- Every code change maps to a bead. Every bead must have explicit acceptance criteria.
- Implementation includes unit tests, integration tests, and documentation updates as specified by the bead.

### Stage 6: Close

**Owner**: Agent (with human review as needed)

- Mark the bead as complete in `br`.
- Self-review passes ("fresh eyes" review after implementation).
- Cross-agent review catches integration issues.
- Update compatibility matrix, docs, and other tracking artifacts.
- Feed lessons learned back into CASS for future sessions.
- Claim next priority bead and return to Stage 4.

---

## Human Responsibilities

The human operator is not writing code in the Flywheel model. The human's responsibilities are:

### Plan Synthesis

- Generate competing plans from multiple frontier models (GPT Pro, Claude Opus, Gemini, Grok).
- Synthesize the best ideas into a single hybrid plan.
- Iterate until the plan is comprehensive and internally consistent.

### Bead Polishing

- Review bead quality across 4-6+ refinement rounds.
- Identify duplicates, missing dependencies, incomplete context, and gaps.
- Ensure every bead has sufficient embedded context for standalone execution.

### AGENTS.md Maintenance

- Keep AGENTS.md current with project conventions, safety rules, and tool documentation.
- This is the single most important coordination artifact.
- After compaction, the most common human intervention is: "Reread AGENTS.md."

### Monitoring

- Spend 10-30 minutes per cycle checking progress.
- Use `br list` and `bv --robot-triage` to monitor bead completion.
- Watch for agents drifting from the plan or producing non-idiomatic output.
- Manage compactions by sending "reread AGENTS.md" prompts.
- Handle rate limiting with account switching if needed.

### Escalation

- Create new beads for unanticipated issues discovered during implementation.
- Resolve architectural ambiguities that agents cannot decide independently.
- Approve any destructive operations (file deletion, force push, reset).
- Intervene when strategic drift is detected: "Where are we? Do we have the thing we are trying to build?"

---

## Agent Automation Mechanics

### Fungible Agents

All agents are generalists. They read the same AGENTS.md and can pick up any bead. This prevents single points of failure: when one agent crashes or needs compaction, others resume its work.

### Agent Composition (Recommended)

| Phase | Recommended Models |
|-------|-------------------|
| Planning | GPT Pro (Extended Reasoning) |
| Plan synthesis | GPT Pro |
| Bead creation | Claude Code (Opus) |
| Implementation | Claude Code + Codex + Gemini (ratio 2:1:1) |
| Review | Claude Code + Gemini |

### Swarm Sizing

- Start with 6-8 active agents during early stabilization.
- Expand only after bead quality and file boundaries prove stable.
- Stagger agent launches by 30+ seconds to avoid contention.

### Recovery After Compaction

Every agent must, after compaction or context reset:

1. Re-read AGENTS.md.
2. Check `git status` and `git log --oneline -10`.
3. Check bead status for current assignments.
4. Resume work on the current bead or claim a new one.

---

## Artifact Progression

Work flows through a defined artifact pipeline, with each stage having different cost characteristics for changes:

```
Raw Idea  -->  Plan  -->  Bead Graph  -->  Ready Bead  -->  Claimed  -->  Completed
```

### Detailed Progression

| Stage | Artifact | Cost of Fixes | Description |
|-------|----------|---------------|-------------|
| Raw Idea | Notes, conversations | ~0.5x | Initial concepts, feature requests, architectural musings |
| Plan | Markdown document | 1x | Full architecture, workflows, system-wide decisions |
| Bead Graph | Task definitions with dependencies | 5x | Execution boundaries, acceptance criteria, context |
| Ready Bead | Bead with all dependencies satisfied | 5x | Available for agent pickup |
| Claimed | Bead under active implementation | 25x | Agent is writing code and tests |
| Completed | Merged code with passing tests | 50x+ | In production, changes require regression work |

The key insight: catching errors in plan space costs far less than discovering them during implementation.

---

## Getting Started: 30-Minute Quick Start

### Minutes 0-5: Install the Tools

```bash
# Install br (beads_rust)
cargo install --git https://github.com/Dicklesworthstone/beads_rust

# Install bv (beads_viewer)
cargo install --git https://github.com/Dicklesworthstone/beads_viewer

# Set up Agent Mail (follow repo instructions)
# Agent Mail is typically configured per-project
```

### Minutes 5-10: Read the Foundation

```bash
# Read the project operating manual
cat AGENTS.md

# Understand the project scope
cat PORT_SCOPE.md

# Review the architecture
cat ARCHITECTURE_MAP.md
```

### Minutes 10-15: Check the Bead Graph

```bash
# List all beads and their status
br list

# Get the current triage report
bv --robot-triage

# See the critical path
bv --critical-path

# Find the next ready bead to work on
bv --next
```

### Minutes 15-20: Claim and Understand a Bead

```bash
# Claim a ready bead
br claim <bead-id>

# Read the bead's full description, acceptance criteria, and dependencies
br show <bead-id>

# Announce file reservations if editing shared areas
# (via Agent Mail)
```

### Minutes 20-28: Implement

- Write the implementation as specified by the bead.
- Write tests alongside the implementation.
- Every `unsafe` block gets a `// SAFETY:` comment.
- Keep commits small and focused. Reference the bead in commit messages.

### Minutes 28-30: Close

```bash
# Self-review: re-read your changes with fresh eyes
git diff

# Mark the bead as complete
br complete <bead-id>

# Move on
bv --next
```

---

## Common Failure Modes

| Failure Mode | Symptom | Root Cause | Remedy |
|-------------|---------|------------|--------|
| Vague beads | Agents improvise inconsistently, output varies wildly | Insufficient detail in bead descriptions | Return to bead space, add missing detail and context |
| Missing dependencies | Agents work on blocked tasks, produce broken code | Incomplete dependency graph | Run `bv` analysis, add missing dependency edges |
| Thin AGENTS.md | Agents produce non-idiomatic code, ignore conventions | Operating manual lacks project-specific rules | Expand AGENTS.md with concrete conventions and examples |
| No Agent Mail | File conflicts, duplicate work, wasted cycles | Agents not coordinating reservations | Enable Agent Mail, enforce reservation policy |
| Thundering herd | Multiple agents claim same bead, edit same files | Agents launched simultaneously | Stagger launches by 30+ seconds |
| Scope creep in beads | Beads grow unbounded, never complete | Bead describes too much work | Split into smaller beads with clear boundaries |
| Strategic drift | Busy swarm heading in wrong direction | No periodic reality checks | Human reviews "where are we?" every few cycles |
| Compaction amnesia | Agent forgets conventions after context reset | AGENTS.md not re-read after compaction | Prompt agent: "Reread AGENTS.md" |
| Premature optimization | Performance work before baselines exist | No benchmark infrastructure | Establish baselines first, then optimize |
| Unsafe sprawl | `unsafe` blocks proliferate without justification | Missing safety policy or review | Enforce SAFETY comment policy, audit regularly |

---

## Helper Utilities

### DCG (Destructive Command Guard)

DCG mechanically blocks dangerous shell and git operations. It prevents commands like `git reset --hard`, `git clean -f`, `git push --force`, and `git branch -D` from executing without explicit human approval. DCG operates at the shell level and cannot be bypassed by agents.

### UBS (Unified Bead Status)

UBS provides a consolidated view of bead progress across the project. It aggregates status from `br` and presents completion rates, blockers, and velocity metrics.

### CASS (Context-Aware Session Summaries)

CASS mines prior agent sessions for repeated failures, useful patterns, and lessons learned. It feeds this knowledge back into subsequent sessions so agents avoid known pitfalls.

- Repository: [https://github.com/Dicklesworthstone/cass_memory_system](https://github.com/Dicklesworthstone/cass_memory_system)
- Role: Persistent cross-session learning, failure pattern detection, context enrichment.

---

## Reference: Dicklesworthstone's GitHub Repositories

| Repository | Purpose |
|-----------|---------|
| [beads_rust](https://github.com/Dicklesworthstone/beads_rust) | Bead task management backend (br) |
| [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) | Graph-theory priority routing and visualization (bv) |
| [agentic_coding_flywheel_setup](https://github.com/Dicklesworthstone/agentic_coding_flywheel_setup) | ACFS - standardized machine setup and agent environment |
| [cass_memory_system](https://github.com/Dicklesworthstone/cass_memory_system) | Cross-session learning and pattern mining |

---

## Summary

The Flywheel methodology separates planning from execution, invests heavily in upfront specification, and uses graph-theory routing to coordinate fungible AI agents. The three tools (br, bv, Agent Mail) provide the infrastructure. The six-stage loop (Plan, Encode, Triage, Coordinate, Implement, Close) provides the rhythm. The human provides strategic oversight, plan synthesis, and escalation handling. The agents provide implementation throughput.

The core principle: **planning compounds returns**. Each planning cycle produces better artifacts that feed into faster, safer implementation.

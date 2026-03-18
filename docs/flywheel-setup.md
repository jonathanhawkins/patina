# Flywheel Setup Guide

Practical instructions for installing and configuring the Agent Flywheel toolchain for the Patina Engine project.

---

## Prerequisites

- Rust toolchain (stable, 2021 edition) via [rustup](https://rustup.rs/)
- Git
- A terminal multiplexer (tmux recommended, WezTerm also supported)
- One or more AI coding agent accounts (Claude Code, Codex, Gemini)

---

## 1. Install br (beads_rust)

`br` is the bead task management backend. It stores bead definitions, dependency graphs, and status transitions.

**Repository**: [https://github.com/Dicklesworthstone/beads_rust](https://github.com/Dicklesworthstone/beads_rust)

### Installation

```bash
# Install from source via cargo
cargo install --git https://github.com/Dicklesworthstone/beads_rust

# Verify installation
br --version
```

### Initial Configuration

```bash
# Initialize a bead store in your project
cd /Users/light/dev/games/patina
br init

# Create your first bead
br create --title "Example bead" --description "Description with acceptance criteria"
```

### Key Commands

| Command | Purpose |
|---------|---------|
| `br init` | Initialize bead store in current project |
| `br create` | Create a new bead |
| `br list` | List all beads with status |
| `br show <id>` | Show full bead details |
| `br claim <id>` | Claim a bead for implementation |
| `br complete <id>` | Mark a bead as complete |
| `br status` | Show overall project status |
| `br dep <id> --on <id>` | Add a dependency between beads |

---

## 2. Install bv (beads_viewer)

`bv` is the graph-theory routing and visualization layer. It computes optimal task priority using PageRank and betweenness centrality analysis.

**Repository**: [https://github.com/Dicklesworthstone/beads_viewer](https://github.com/Dicklesworthstone/beads_viewer)

Alternative Rust implementation: [https://github.com/Dicklesworthstone/beads_viewer_rust](https://github.com/Dicklesworthstone/beads_viewer_rust)

### Installation

```bash
# Install from source via cargo
cargo install --git https://github.com/Dicklesworthstone/beads_viewer

# Or use the Rust-native version
cargo install --git https://github.com/Dicklesworthstone/beads_viewer_rust

# Verify installation
bv --version
```

### Key Commands

| Command | Purpose |
|---------|---------|
| `bv --robot-triage` | Generate triage report for agent consumption |
| `bv --critical-path` | Show the critical path through the bead graph |
| `bv --next` | Recommend the next bead to work on |
| `bv --visualize` | Generate a visual dependency graph |
| `bv --stats` | Show graph statistics and health metrics |

### How Priority Routing Works

`bv` reads the bead graph from `br` and applies graph-theory algorithms:

1. **PageRank**: Identifies beads that many other beads depend on (high-impact foundational work).
2. **Betweenness centrality**: Identifies beads that sit on many shortest paths (bottleneck work).
3. **Ready filter**: Only surfaces beads whose dependencies are all satisfied.

When multiple agents each independently query `bv` for priority, they naturally distribute across the highest-impact ready work without centralized scheduling.

---

## 3. Setting Up Agent Mail

Agent Mail provides identity, directed messaging, and advisory file reservations between agents.

### Configuration

Agent Mail is typically configured per-project through AGENTS.md conventions and a local message store.

```bash
# Agent Mail uses a local message directory
mkdir -p .agent-mail

# Each agent has an identity (typically assigned at launch)
# Messages are targeted: agent-to-agent, not broadcast
```

### File Reservations

File reservations prevent edit collisions when multiple agents work concurrently:

```bash
# Reserve a file before editing (via Agent Mail)
# Reservations have a TTL and are advisory
# Pre-commit guards enforce reservation checks
```

### Reservation Rules for Patina

- **Required**: Before editing shared files (AGENTS.md, workspace Cargo.toml, package.json, foundation documents).
- **Not required**: Crate-internal files, website component files (unless shared layout/config).
- **TTL**: Reservations expire automatically. Renew if work takes longer than expected.

---

## 4. Setting Up DCG (Destructive Command Guard)

DCG mechanically blocks dangerous shell and git operations at the shell level. Agents cannot bypass it.

### What DCG Blocks

- `git reset --hard`
- `git clean -f`
- `git push --force`
- `git checkout .` (destructive checkout)
- `git branch -D` (force delete)
- `rm -rf` on critical paths
- Any command classified as destructive without explicit human approval

### Installation

DCG is part of the ACFS (Agentic Coding Flywheel Setup) toolkit:

```bash
# Install ACFS which includes DCG
# Repository: https://github.com/Dicklesworthstone/agentic_coding_flywheel_setup
git clone https://github.com/Dicklesworthstone/agentic_coding_flywheel_setup
cd agentic_coding_flywheel_setup
# Follow the setup instructions in the repository README
```

### Verification

```bash
# Test that DCG is active by attempting a blocked command
# (It should be rejected with a clear error message)
git reset --hard  # Should be blocked by DCG
```

---

## 5. CASS Memory System

CASS (Context-Aware Session Summaries) mines prior agent sessions for repeated failures, useful patterns, and lessons learned. It provides persistent cross-session learning.

**Repository**: [https://github.com/Dicklesworthstone/cass_memory_system](https://github.com/Dicklesworthstone/cass_memory_system)

### What CASS Does

- **Session mining**: Extracts patterns from completed agent sessions.
- **Failure detection**: Identifies repeated failures so agents avoid known pitfalls.
- **Context enrichment**: Feeds learned patterns back into subsequent sessions.
- **Knowledge persistence**: Maintains a growing knowledge base across all project sessions.

### Setup

```bash
# Clone and install CASS
git clone https://github.com/Dicklesworthstone/cass_memory_system
cd cass_memory_system
# Follow the installation instructions in the repository README

# CASS integrates with br and Agent Mail to capture session data
```

### Usage

CASS runs in the background and is typically queried during:

- Agent session startup (to load relevant prior knowledge)
- Bead claiming (to check for known issues with similar work)
- Post-implementation review (to capture new lessons)

---

## 6. NTM (Named Tmux Manager)

NTM manages multiple agent sessions using tmux. It provides commands for spawning, monitoring, and communicating with agent swarms.

### Installation

NTM is part of the ACFS toolkit:

```bash
# Install via ACFS
# Repository: https://github.com/Dicklesworthstone/agentic_coding_flywheel_setup
```

### Key Commands

```bash
# Spawn a swarm with specific agent composition
# --cc = Claude Code agents, --cod = Codex agents, --gmi = Gemini agents
ntm spawn patina --cc=2 --cod=1 --gmi=1

# Send marching orders to all agents in the swarm
ntm send patina "Reread AGENTS.md. Check bv --robot-triage for priority. Claim and implement the next ready bead."

# List active sessions
ntm list

# Attach to a specific agent session
ntm attach patina-cc-1

# Kill a specific session
ntm kill patina-cc-2

# Kill all sessions in the swarm
ntm kill patina --all
```

### Swarm Launch Best Practices

- **Stagger launches**: Wait 30+ seconds between agent spawns to avoid thundering-herd contention on initial bead selection.
- **Start small**: Begin with 6-8 agents during early stabilization.
- **Expand carefully**: Only add agents after bead quality and file boundaries prove stable.
- **Monitor**: Check on agents every 10-30 minutes using `br list` and `bv --robot-triage`.

---

## 7. Recommended Agent Composition

### By Project Phase

| Phase | Models | Notes |
|-------|--------|-------|
| Planning | GPT Pro (Extended Reasoning) | Best for generating comprehensive initial plans |
| Plan synthesis | GPT Pro | Synthesizes competing plans into hybrid |
| Bead creation | Claude Code (Opus) | Best for structured task decomposition |
| Implementation | Claude Code + Codex + Gemini (2:1:1) | Mixed swarm for implementation throughput |
| Review | Claude Code + Gemini | Cross-model review catches more issues |

### Swarm Sizing Guidelines

| Project Phase | Recommended Size | Rationale |
|--------------|-----------------|-----------|
| Foundation (Phase 0) | 2-4 agents | Low parallelism, mostly documentation |
| Oracle/Fixtures (Phase 1) | 4-6 agents | Moderate parallelism, independent fixture work |
| Core Runtime (Phase 3) | 6-8 agents | High parallelism, strict crate boundaries |
| Vertical Slice (Phase 4+) | 8-12 agents | Maximum parallelism, well-established boundaries |

---

## 8. All Relevant Repositories

| Repository | URL | Purpose |
|-----------|-----|---------|
| beads_rust | [github.com/Dicklesworthstone/beads_rust](https://github.com/Dicklesworthstone/beads_rust) | Bead task management (br) |
| beads_viewer | [github.com/Dicklesworthstone/beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) | Graph-theory priority routing (bv) |
| beads_viewer_rust | [github.com/Dicklesworthstone/beads_viewer_rust](https://github.com/Dicklesworthstone/beads_viewer_rust) | Rust-native bv implementation |
| agentic_coding_flywheel_setup | [github.com/Dicklesworthstone/agentic_coding_flywheel_setup](https://github.com/Dicklesworthstone/agentic_coding_flywheel_setup) | ACFS - machine setup, DCG, NTM |
| cass_memory_system | [github.com/Dicklesworthstone/cass_memory_system](https://github.com/Dicklesworthstone/cass_memory_system) | Cross-session learning and pattern mining |

---

## Quick Verification Checklist

After setup, verify the following:

- [ ] `br --version` returns a version number
- [ ] `bv --version` returns a version number
- [ ] `br init` succeeds in the project directory
- [ ] `bv --robot-triage` runs without errors
- [ ] DCG blocks `git reset --hard` with a clear error
- [ ] NTM can spawn and list sessions
- [ ] AGENTS.md is present and current at the project root
- [ ] Agent Mail directory exists (`.agent-mail/`)

---

## Troubleshooting

| Issue | Resolution |
|-------|-----------|
| `br` command not found | Ensure `~/.cargo/bin` is in your PATH |
| `bv` shows empty graph | Run `br list` to verify beads exist |
| DCG not blocking commands | Verify ACFS setup completed; check shell profile sourcing |
| Agents not coordinating | Verify Agent Mail is configured; check reservation policy |
| Compaction amnesia | Send "Reread AGENTS.md" to the affected agent session |
| Rate limiting | Switch between provider accounts; reduce swarm size temporarily |

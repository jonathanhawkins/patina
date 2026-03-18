# RISK_REGISTER.md - Patina Engine Risk Register

This document catalogs known risks to the Patina Engine project, their assessed likelihood and impact, and the mitigation strategies for each.

---

## Risk Assessment Scale

| Rating | Likelihood | Impact |
|--------|-----------|--------|
| **Low** | Unlikely to occur or only under unusual conditions | Minimal disruption; easily recoverable |
| **Medium** | Plausible; has occurred in similar projects | Noticeable delay or quality impact; recoverable with effort |
| **High** | Likely to occur without active mitigation | Significant schedule, quality, or viability impact |

---

## Risk Register

### RISK-001: Scope Explosion

| Field | Value |
|-------|-------|
| **Description** | The project scope grows beyond what staged milestones can absorb. Teams begin implementing subsystems that are not yet needed, or "just one more feature" requests accumulate without corresponding bead decomposition and prioritization. |
| **Likelihood** | High |
| **Impact** | High |
| **Mitigation** | Enforce PORT_SCOPE.md as the authoritative scope boundary. Stage milestones aggressively with clear exit criteria. Defer editor and broad platform parity explicitly. Every implementation task must map to a bead; unbounded work is rejected. Human operator performs periodic "where are we?" reality checks. |
| **Owner** | Lead Architect |

---

### RISK-002: False Compatibility Confidence

| Field | Value |
|-------|-------|
| **Description** | Visual demos or passing smoke tests create a misleading impression of Godot parity. Teams declare subsystems "working" based on a handful of cases while substantial behavioral gaps remain undetected. |
| **Likelihood** | High |
| **Impact** | High |
| **Mitigation** | Require oracle-backed fixtures for all compatibility claims. Distinguish visual demos from measured parity in all status reports. Preserve golden artifacts in version control. Quantify parity with fixture pass rates, not subjective assessments. Every compatibility test must state what observable behavior it checks. |
| **Owner** | Compatibility Lead |

---

### RISK-003: Agent Collisions and Merge Chaos

| Field | Value |
|-------|-------|
| **Description** | Multiple AI agents edit the same files simultaneously, producing merge conflicts, lost work, or semantically incompatible changes that pass syntactic merges but break behavior. |
| **Likelihood** | Medium |
| **Impact** | High |
| **Mitigation** | Mandatory file reservations via Agent Mail before editing shared areas. Bead ownership ensures only one agent works on a given task. Strict crate boundaries limit the blast radius of any single agent's work. Short-lived branches and frequent sync reduce conflict window. DCG blocks destructive recovery attempts. Pre-commit guards enforce reservation checks. |
| **Owner** | Infrastructure Lead |

---

### RISK-004: Over-Reimplementation of Third-Party Code

| Field | Value |
|-------|-------|
| **Description** | Teams reimplement third-party libraries (from Godot's thirdparty/ directory) in Rust when wrapping, vendoring, or replacing with existing Rust crates would be more appropriate. This wastes effort and introduces bugs in well-tested code. |
| **Likelihood** | Medium |
| **Impact** | Medium |
| **Mitigation** | Require classification in THIRDPARTY_STRATEGY.md before any third-party implementation work begins. Prefer proven Rust ecosystem components where reasonable. Four-bucket classification (replace, wrap, vendor, reimplement) forces explicit decisions. No team starts reimplementing until the classification decision is made and recorded. |
| **Owner** | Lead Architect |

---

### RISK-005: Unsafe Rust Sprawl

| Field | Value |
|-------|-------|
| **Description** | `unsafe` blocks proliferate across crates without proper justification, auditing, or containment. This undermines the memory safety guarantees that motivate the Rust port. |
| **Likelihood** | Medium |
| **Impact** | High |
| **Mitigation** | Isolate unsafe code behind narrow, audited interfaces. Require a `// SAFETY:` comment on every `unsafe` block explaining the invariant being upheld. Add focused tests around unsafe boundaries. Track unsafe surface area as a project metric. Periodic audits of unsafe usage. AGENTS.md enforces the policy for all agents. |
| **Owner** | Runtime Lead |

---

### RISK-006: Performance Regression from Early Architecture Choices

| Field | Value |
|-------|-------|
| **Description** | Architectural decisions made during early phases (crate boundaries, data structures, threading model) create performance bottlenecks that are expensive to fix once code is built on top of them. |
| **Likelihood** | Medium |
| **Impact** | Medium |
| **Mitigation** | Capture benchmarks from the first runnable slice (Phase 4). Make performance visible continuously rather than measuring only at the end. Establish upstream Godot baselines for comparison. Define acceptable regression thresholds in BENCHMARKS.md. Performance-sensitive crate interfaces should be designed for future optimization without breaking changes. |
| **Owner** | Infrastructure Lead |

---

### RISK-007: Legal/Licensing Mistakes

| Field | Value |
|-------|-------|
| **Description** | Vendored, wrapped, or reimplemented third-party code introduces licensing obligations that are not properly tracked, attributed, or complied with. This could range from missing attribution to GPL contamination of the Rust codebase. |
| **Likelihood** | Low |
| **Impact** | High |
| **Mitigation** | Maintain dependency provenance for every third-party component. Review licenses before any port/wrap/reuse decision. Document every third-party path in THIRDPARTY_STRATEGY.md with license classification. Prefer behavior-driven specs and tests over direct source translation to maintain clean-room discipline where feasible. Flag any copyleft dependencies for explicit legal review before integration. |
| **Owner** | Lead Architect |

---

## Review Cadence

This risk register should be reviewed:

- At every phase transition (milestone completion).
- When a new risk is identified during implementation.
- During periodic "where are we?" reality checks.

New risks should be added with the next available RISK-NNN identifier. Retired risks should be marked as "Closed" with a resolution note rather than deleted.

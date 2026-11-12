# Patina Engine — Release Process

> Phase 9 deliverable: release train documentation.

## Versioning

Patina follows [Semantic Versioning](https://semver.org/):

- **0.x.y** — pre-1.0 development. Breaking changes bump minor, fixes bump patch.
- All workspace crates share the same version via `workspace.package.version` in the root `Cargo.toml`.

## Release Cadence

- **Patch releases** (0.1.x): as needed for bug fixes.
- **Minor releases** (0.x.0): at each phase milestone completion.
- **No fixed schedule** — releases are milestone-driven, not time-driven.

## Release Checklist

### 1. Pre-release Validation

```bash
# All workspace tests must pass
cd engine-rs && cargo test --workspace

# Release build must succeed
cargo build --workspace --release

# Clippy clean (warnings are errors in CI)
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --all -- --check
```

### 2. Version Bump

Update `workspace.package.version` in `engine-rs/Cargo.toml`:

```toml
[workspace.package]
version = "0.2.0"  # bump appropriately
```

All crates inherit this version automatically.

### 3. Changelog

Update `CHANGELOG.md` in the repository root. Format:

```markdown
## [0.2.0] - 2026-04-01

### Added
- Feature descriptions

### Fixed
- Bug fix descriptions

### Changed
- Breaking or notable changes
```

### 4. Tag and Push

```bash
git add -A
git commit -m "Release v0.2.0"
git tag -a v0.2.0 -m "Patina Engine v0.2.0"
git push origin main --tags
```

### 5. GitHub Release

If the repository's `release.yml` workflow is enabled and committed, it should:
- Run the full test suite
- Build release binaries
- Create a GitHub Release with the tag
- Attach build artifacts

If that workflow is not present in the current checkout or is not yet enabled,
create the release manually via `gh release create` and treat this document as
the source of truth for the required release steps.

### 6. Post-release

- Bump version to next dev version (e.g., `0.3.0-dev`) if desired.
- Announce in project channels.

## CI/CD Pipeline

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | push/PR to main | Format, lint, test, release build |
| `release.yml` | tag `v*` push | Full validation + GitHub Release, when the workflow is present/enabled |

## Hotfix Process

1. Branch from the release tag: `git checkout -b hotfix/v0.2.1 v0.2.0`
2. Apply fix with test.
3. Bump patch version.
4. Tag and push.

## Oracle Parity Gate

No release may ship if oracle parity drops below the threshold documented in `prd/V1_EXIT_CRITERIA.md`. The release workflow enforces this by running the full oracle regression suite.

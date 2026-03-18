# Releasing Patina Engine

## Crate Publish Order

Crates must be published in dependency order. Each crate must be published and available on crates.io before its dependents:

1. **gdcore** — No internal dependencies
2. **gdvariant** — Depends on gdcore
3. **gdobject** — Depends on gdcore, gdvariant
4. **gdresource** — Depends on gdcore, gdvariant
5. **gdserver2d** — Depends on gdcore
6. **gdscene** — Depends on gdcore, gdvariant, gdresource, gdobject
7. **gdphysics2d** — Depends on gdcore
8. **gdrender2d** — Depends on gdcore, gdserver2d
9. **gdaudio** — Depends on gdcore
10. **gdplatform** — Depends on gdcore
11. **gdscript-interop** — Depends on gdcore, gdvariant
12. **gdeditor** — Depends on gdcore, gdscene, gdresource
13. **patina-runner** — Depends on all of the above

## Version Bump Process

1. Decide on the version bump level: `patch`, `minor`, or `major`.
2. Update `version` in `engine-rs/Cargo.toml` under `[workspace.package]`.
   - All crates share the workspace version. One change updates them all.
3. Update inter-crate dependency versions if they specify exact versions.
4. Update `CHANGELOG.md` with the new version section.
5. Commit: `git commit -m "Bump version to X.Y.Z"`
6. Tag: `git tag vX.Y.Z`
7. Push: `git push && git push --tags`

## Changelog Template

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added
- New feature or capability

### Changed
- Changes to existing functionality

### Fixed
- Bug fixes

### Removed
- Removed features or deprecated items
```

## Pre-Release Checklist

- [ ] All tests pass: `cd engine-rs && cargo test --workspace`
- [ ] Clippy clean: `cd engine-rs && cargo clippy --workspace -- -D warnings`
- [ ] Docs build: `cd engine-rs && cargo doc --workspace --no-deps`
- [ ] Benchmarks run without errors: `cd engine-rs && cargo run --example benchmarks`
- [ ] No uncommitted changes: `git status` shows clean tree
- [ ] Version bumped in `engine-rs/Cargo.toml`
- [ ] CHANGELOG.md updated
- [ ] Git tag created
- [ ] Website build passes: `cd apps/web && pnpm build`

## Publishing

```bash
cd engine-rs

# Dry run first:
cargo publish -p gdcore --dry-run
cargo publish -p gdvariant --dry-run
# ... etc for each crate in order

# Actual publish (in order):
cargo publish -p gdcore
cargo publish -p gdvariant
cargo publish -p gdobject
cargo publish -p gdresource
cargo publish -p gdserver2d
cargo publish -p gdscene
cargo publish -p gdphysics2d
cargo publish -p gdrender2d
cargo publish -p gdaudio
cargo publish -p gdplatform
cargo publish -p gdscript-interop
cargo publish -p gdeditor
cargo publish -p patina-runner
```

Wait for each crate to be indexed before publishing its dependents. This typically takes 30-60 seconds.

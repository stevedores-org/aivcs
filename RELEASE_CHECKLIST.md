# Release Checklist

## Versioning Policy

AIVCS follows [Semantic Versioning](https://semver.org/):

- **MAJOR** — breaking API or storage format changes
- **MINOR** — new features, backward-compatible
- **PATCH** — bug fixes, documentation

The workspace version in `Cargo.toml` is the single source of truth. All crates inherit `version.workspace = true`.

## Pre-Release

- [ ] All CI checks pass on `develop` (`cargo fmt`, `cargo clippy`, `cargo test`)
- [ ] `CHANGELOG.md` is updated with the new version entry
- [ ] Workspace version in `Cargo.toml` is bumped
- [ ] Version consistency test passes: `cargo test version_consistency`
- [ ] No `TODO` or `FIXME` items blocking release
- [ ] Documentation in `docs/` is up to date

## Release

- [ ] Merge `develop` into `main` via pull request
- [ ] Tag the merge commit: `git tag v<VERSION> && git push origin v<VERSION>`
- [ ] Verify the `release.yml` GitHub Actions workflow completes:
  - Linux binary built
  - macOS binary built
  - GitHub Release created with both binaries attached
- [ ] Verify the release page has correct notes and artifacts

## Post-Release

- [ ] Announce the release (if applicable)
- [ ] Bump workspace version in `Cargo.toml` to next dev version (e.g. `0.2.0`)
- [ ] Create a new `## [Unreleased]` section in `CHANGELOG.md`

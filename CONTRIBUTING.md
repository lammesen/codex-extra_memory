# Contributing

## Prerequisites

- Rust stable toolchain
- Cargo

## Local checks

Run these before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

## Pull requests

- Open PRs against `main`.
- Keep changes scoped and include rationale in PR description.
- Ensure CI checks pass:
  - `ci / fmt`
  - `ci / clippy`
  - `ci / test`
  - `dependency-review / dependency-review` (when dependency/workflow files change)

## Release model

- Pushes to `main` publish crates.io prereleases and create GitHub prereleases.
- Tags `vX.Y.Z` publish stable crates and create stable GitHub releases.

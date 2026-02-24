# Contributing to codex-extra-memory

Thank you for considering contributing! This document explains how to get started.

## Development Setup

1. Install Rust via [rustup](https://rustup.rs/):
   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Install nightly toolchain (for `rustfmt`):
   ```sh
   rustup toolchain install nightly --component rustfmt
   ```

3. Clone and build:
   ```sh
   git clone https://github.com/lammesen/codex-extra_memory.git
   cd codex-extra_memory
   cargo build --workspace
   ```

4. Run the full check suite:
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-features --locked
   cargo doc --workspace --no-deps --all-features --locked
   cargo deny check advisories bans licenses sources
   cargo audit --deny warnings
   ```

## Commit Conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

Common types:
- `feat:` — new feature (triggers minor version bump)
- `fix:` — bug fix (triggers patch version bump)
- `docs:` — documentation only
- `chore:` — maintenance, CI, dependencies
- `refactor:` — code change that neither fixes a bug nor adds a feature
- `test:` — adding or updating tests
- `perf:` — performance improvement

Breaking changes: add `!` after the type (e.g., `feat!:`) or add `BREAKING CHANGE:` in the footer. This triggers a major version bump.

## Pull Request Process

1. Fork the repository and create a feature branch from `main`.
2. Make your changes, ensuring:
   - All tests pass (`cargo test --workspace --all-features --locked`)
   - Code is formatted (`cargo +nightly fmt --all`)
   - Clippy is happy (`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`)
   - Documentation builds (`cargo doc --workspace --no-deps --all-features --locked`)
   - Security checks pass (`cargo deny check advisories bans licenses sources` and `cargo audit --deny warnings`)
3. Write commit messages following Conventional Commits.
4. Open a pull request against `main`.
5. Address any review feedback.

## Release Process

Releases are automated via [release-plz](https://release-plz.ieni.dev/):

1. Merge conventional commits to `main`.
2. release-plz automatically creates a Release PR with version bump and changelog.
3. Merging the Release PR publishes to crates.io and creates a GitHub Release.

## Optional: Pre-commit Hooks

If you'd like local checks before committing:

```sh
# .git/hooks/pre-commit
#!/bin/sh
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Make it executable: `chmod +x .git/hooks/pre-commit`

## Code of Conduct

Be kind and respectful. We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

# codex-extra-memory

Rust codex-native memory extension with MCP + CLI, central SQLite persistence, and managed AGENTS.md sync.

## Workspace Layout

```
crates/codex-extra-memory-core/      # Core: DB, parser, sync, capture, compaction
crates/codex-extra-memory-mcp/       # MCP stdio server (mcpkit)
crates/codex-extra-memory-cli/       # CLI binary (codex-memory)
crates/codex-extra-memory-installer/ # Codex config installer/uninstaller
install/                             # Shell install/uninstall scripts
```

## Build & Test Commands

```sh
cargo build --workspace                                    # Build all crates
cargo test --workspace --all-features --locked             # Run all tests
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings # Lint
cargo +nightly fmt --all --check                           # Format check (nightly required)
cargo doc --workspace --no-deps --all-features --locked    # Build docs
cargo deny check advisories bans licenses sources          # Supply-chain policy
cargo audit --deny warnings                                # RustSec advisory audit
```

## Code Style

- **Edition**: 2024, MSRV 1.85
- **Formatting**: `rustfmt.toml` with `max_width = 100`, use nightly rustfmt
- **Lints**: Clippy `pedantic` + `nursery` + `cargo` enabled, `unsafe_code` forbidden
- **All warnings are errors** in CI (`RUSTFLAGS="-Dwarnings"`)

## Commit Conventions

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` new feature (minor bump)
- `fix:` bug fix (patch bump)
- `docs:` documentation
- `chore:` maintenance/CI
- `refactor:` code restructuring
- `test:` tests
- `feat!:` or `BREAKING CHANGE:` footer for breaking changes (major bump)

## CI/CD Overview

- **ci.yml**: check, fmt, clippy, test, doc, semver-checks (codex-extra-memory-core), machete
- **security.yml**: cargo-audit + cargo-deny (advisories, bans, licenses, sources)
- **codeql.yml**: static analysis for Rust
- **dependency-review.yml**: blocks risky dependency changes in PRs
- **scorecard.yml**: OSSF Scorecard → GitHub Security tab
- **release-plz.yml**: auto Release PR + crates.io publish + GitHub Release
- **container.yml**: Docker build with cargo-chef → GHCR + provenance/SBOM + Trivy scan
- **dependabot-auto.yml**: auto-approve/merge Dependabot patch/minor PRs

## License

MIT OR Apache-2.0

# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public issue.**
2. Use [GitHub's private vulnerability reporting](https://github.com/lammesen/codex-extra_memory/security/advisories/new).
3. Alternatively, email the maintainer directly.

You should receive an acknowledgment within 48 hours, and a detailed response
within 7 days indicating next steps.

## Security Measures

This project employs the following security measures:

- **`unsafe` code is forbidden** via `Cargo.toml` lint policy.
- **`cargo-audit`** runs in CI (`.github/workflows/security.yml`) with `--deny warnings`.
- **`cargo-deny`** runs in CI with blocking checks for advisories, bans, licenses, and sources.
- **CodeQL** runs on pull requests, pushes to `main`, and weekly schedule.
- **Dependency Review** runs on pull requests and blocks low+ severity vulnerabilities.
- **Pinned GitHub Actions** are used across workflows to reduce action supply-chain drift.
- **Container release workflow** publishes SBOM/provenance metadata and scans images with Trivy.
- **OSSF Scorecard** runs weekly and uploads SARIF findings to the Security tab.
- **Dependabot** updates Cargo and GitHub Actions dependencies weekly.

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | Yes       |

Only the latest release receives security updates.

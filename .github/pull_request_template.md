## Description

<!-- Briefly describe the changes in this PR. -->

## Checklist

- [ ] Tests pass (`cargo test --workspace --all-features --locked`)
- [ ] Security checks pass (`cargo deny check advisories bans licenses sources` and `cargo audit --deny warnings`)
- [ ] Code is formatted (`cargo +nightly fmt --all --check`)
- [ ] Clippy is happy (`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`)
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Documentation is updated (if applicable)

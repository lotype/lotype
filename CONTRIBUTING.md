# Contributing

## Setup

The Rust toolchain is pinned in `rust-toolchain.toml`; rustup installs it automatically.

Commit messages are checked locally and in CI by [committed](https://github.com/crate-ci/committed):

```sh
cargo install committed --version 1.1.12 --locked
git config core.hooksPath .githooks
```

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), with the rules in `committed.toml`:

```text
<type>(<optional scope>): <imperative, lowercase summary, max 72 chars, no trailing period>
```

Types: `build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, `test`.
Scope is the crate without the `lotype-` prefix, e.g. `feat(core): add block tree`.

## Before pushing

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for design and code conventions.

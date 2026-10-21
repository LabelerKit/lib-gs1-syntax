# CLAUDE.md

Guidance for working in this repository.

## Overview

lib-gs1-syntax is a Rust workspace for GS1 Application Identifier element
strings: parsing, validation and the GS1 Digital Link form. See
[README.md](README.md) for the crates and commands.

## Commit messages — Conventional Commits (required)

All commits **must** follow the [Conventional Commits](https://www.conventionalcommits.org/)
specification. This is enforced by the `commit-msg` hook in
[`.githooks/commit-msg`](.githooks/commit-msg), which also **rejects any
`Co-Authored-By:` trailer** — this project does not record co-authors in commits.

```
type(scope)?: description
```

- **Allowed types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`,
  `build`, `ci`, `perf`, `revert`.
- **scope** is optional and names the crate (`gs1-syntax`, `gs1-syntax-gen`, …).
- Append `!` before the colon for a breaking change.

**Claude: before committing, make sure the hook is active.** `core.hooksPath` is
per-clone and not versioned:

```sh
git config --get core.hooksPath || git config core.hooksPath .githooks
```

## Quality gates

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

MSRV is 1.89; the toolchain is pinned in `rust-toolchain.toml`.

## Generated and vendored data

- `crates/gs1-syntax/src/ai.rs` is generated — do not edit it. Change the
  dictionary or the generator, then run `cargo run -p gs1-syntax-gen && cargo fmt --all`.
- The vendored dictionary `crates/gs1-syntax/data/gs1-syntax-dictionary.txt` must
  stay byte-identical to upstream. Its provenance lives in
  `crates/gs1-syntax/data/UPSTREAM`; CI checks the checksum recorded there.
- Keep [NOTICE](NOTICE) in step when adding anything derived from GS1 sources.

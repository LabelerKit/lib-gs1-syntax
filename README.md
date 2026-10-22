# lib-gs1-syntax

GS1 Application Identifier element strings: parsing, validation, and the GS1
Digital Link URI form. The rules are generated from the
[GS1 Barcode Syntax Dictionary](https://github.com/gs1/gs1-syntax-dictionary),
so the library carries every AI the standard defines rather than a hand-kept
subset. It has no dependencies and does no I/O.

## Crates

| Crate | Description |
| --- | --- |
| [`gs1-syntax`](crates/gs1-syntax) | Parse and validate GS1 Application Identifier element strings |
| [`gs1-syntax-iso15223`](crates/gs1-syntax-iso15223) | Device-label data titles (ISO 15223-1 / UDI) for GS1 application identifiers |
| [`gs1-syntax-gen`](crates/gs1-syntax-gen) | Generates gs1-syntax's AI table from the GS1 Barcode Syntax Dictionary |

## Usage

The crates are not published to crates.io. Depend on them as a git dependency,
pinned to a commit:

```toml
[dependencies]
gs1-syntax = { git = "https://github.com/LabelerKit/lib-gs1-syntax", rev = "<commit>" }
```

```rust
use gs1_syntax::{ElementString, SEPARATOR};

let parsed = ElementString::parse("(01)09501101020917(10)ABC123")?;
let symbol_form = parsed.concatenated(SEPARATOR);
```

See [`crates/gs1-syntax/README.md`](crates/gs1-syntax/README.md) for the
checks performed, the Digital Link API, and how the library is verified.

## Regenerating

`crates/gs1-syntax/src/ai.rs` is generated and committed. After replacing the
vendored dictionary, regenerate and format:

```sh
cargo run -p gs1-syntax-gen && cargo fmt --all
```

Replacing it is itself scripted. `update-dictionary.py` fetches the newest
`gs1-syntax-dictionary.txt` from the dictionary's `main` branch and, when its
SHA-256 differs from the one in `UPSTREAM`, vendors it byte for byte and moves
`UPSTREAM` and both NOTICE files along with it (the NOTICE prose describing
the snapshot is left for a person):

```sh
python3 crates/gs1-syntax-gen/update-dictionary.py && cargo run -p gs1-syntax-gen && cargo fmt --all
```

The *Upstream dictionary* workflow
([`.github/workflows/upstream-dictionary.yml`](.github/workflows/upstream-dictionary.yml))
runs exactly this every Monday at 06:00 UTC, or on demand from the Actions
tab, and opens or refreshes a pull request on the `auto/gs1-syntax-dictionary`
branch, starting CI on it. The pull request is opened even when the tests fail
on the new data, and as a draft when the generator refuses the file. For it to
work, the repository must allow GitHub Actions to create pull requests
(Settings → Actions → General); GitHub also suspends scheduled workflows in a
repository with no activity for 60 days, so re-enable it there if it stops.

The test vectors and code lists are rebuilt from a checkout of
[gs1-syntax-engine](https://github.com/gs1/gs1-syntax-engine) at the commit
recorded in `crates/gs1-syntax/data/UPSTREAM`; the scripts refuse a checkout
at any other commit, so a provenance header cannot name the wrong one. Each
script's docstring says more; the invocations are:

```sh
python3 crates/gs1-syntax-gen/extract-vectors.py <engine>/src/c-lib/syntax crates/gs1-syntax/data/reference-vectors.txt
python3 crates/gs1-syntax-gen/extract-dl-vectors.py <engine>/src/c-lib/dl.c crates/gs1-syntax/data/digital-link-vectors.txt
python3 crates/gs1-syntax-gen/code-lists.py <engine>/src/c-lib/syntax crates/gs1-syntax/src/codes.rs && cargo fmt --all
```

## Development

Enable the commit hook once per clone. Commits must follow
[Conventional Commits](https://www.conventionalcommits.org/); the hook also
rejects `Co-Authored-By:` trailers.

```sh
git config core.hooksPath .githooks
```

Quality gates:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The toolchain is pinned to 1.94.1 in `rust-toolchain.toml`. The minimum
supported Rust version is 1.89 (`rust-version` in the workspace manifest).

CI (`.github/workflows/ci.yml`) runs the gates above, a 1.89 build, and a
check that the generated table and the vendored dictionary are unchanged. It
can also be started by hand (`workflow_dispatch`), which is how the upstream
dictionary workflow runs it on the pull requests it opens.

## Licence

Apache-2.0 (see [LICENSE](LICENSE)). Copyright 2026 Krystian Duma; portions
Copyright GS1 AISBL, BWIPP project and Zint Project. See [NOTICE](NOTICE) for
what is derived from the GS1 Barcode Syntax Dictionary and the GS1 Barcode
Syntax Engine, and under which terms.

This project is not affiliated with or endorsed by GS1 AISBL. GS1 is a
registered trademark of GS1 AISBL.

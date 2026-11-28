# Contributing to ccdig

Thanks for your interest in improving ccdig.

## Development setup

You need [rustup](https://rustup.rs/). [rust-toolchain.toml](rust-toolchain.toml)
pins the toolchain used for development and CI, with clippy and rustfmt;
`cargo` installs it on first use. For the dependency check, install
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny); for the
module structure check, [ast-grep](https://ast-grep.github.io/) 0.51.0
(`cargo install ast-grep --locked --version 0.51.0`); for the snapshot
review, [cargo-insta](https://insta.rs/docs/cli/) 1.49.0
(`cargo install cargo-insta --locked --version 1.49.0`); and for the
coverage check, [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
0.9.1 (`cargo install cargo-llvm-cov --locked --version 0.9.1`).

## Checks

Each of these must pass before a pull request is merged. CI runs the same
commands.

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo insta test --check --unreferenced=reject
cargo deny check
ast-grep test
ast-grep scan --error=unused-suppression --error=no-suppress-all
.github/scripts/check-architecture.sh
.github/scripts/check-readme-sync.sh
.github/scripts/coverage.sh
```

Every clippy group, `restriction` included, is denied in
[Cargo.toml](Cargo.toml). The few lints that are allowed say why next to
them. In code, silence a lint only with
`#[expect(clippy::name, reason = "...")]`, so the exception goes away when
it is no longer needed. Upgrading the toolchain can add lints; review them
when you do.

[clippy.toml](clippy.toml) and the ast-grep rules under
[.ast-grep/rules](.ast-grep/rules) hold the module structure in place: which
module may use which, and where I/O may happen.
[ARCHITECTURE.md](ARCHITECTURE.md) explains it, and
`.github/scripts/check-architecture.sh` fails when its module table, the
rules, and the files that may do I/O disagree. The `layer-*` rules are
written from the table: change the table, then run the script with
`--write`. Silence an ast-grep rule on
one line only with `// ast-grep-ignore: <rule-id>` and a reason next to it.

## How we work

- Write a failing test first, then the smallest change that makes it pass.
- Tests return `Result` and compare with `check_eq` from `src/testing.rs`,
  since `assert_eq!` panics. Prefer table-driven tests.
- Rendered output is checked against [insta](https://insta.rs/) snapshots
  under `testdata/`, through `snapshot` in `src/testing.rs`. A mismatch
  panics inside insta, the one exception to the rule above. When output
  changes on purpose, `cargo test` fails and writes a `.snap.new` next to
  the snapshot; accept or reject each change with `cargo insta review`.
  With `CI` set, insta writes nothing, so run `cargo insta test --review`
  instead. `cargo insta test --check --unreferenced=reject` fails on a
  snapshot no test uses any more.
- A table lists the cases you thought of; a property states what holds for
  any input, such as "no generated token is left in the output", and
  proptest looks for the input that breaks it. Write properties in the
  module's `tests/properties.rs`, run them with `for_all` from
  `src/testing.rs`, and list a new file in
  [ARCHITECTURE.md](ARCHITECTURE.md). `PROPTEST_CASES=5000 cargo test`
  tries more inputs, and `PROPTEST_RNG_SEED` replays a run. A failing input
  is not saved to disk, since it can look like a secret: add the smallest
  one the failure reports to a table-driven test, building any secret in it
  at run time.
- `.github/scripts/coverage.sh` fails when line coverage drops below the
  floor it sets. Raise the floor when coverage goes up.
- Keep commits focused, with a message that makes sense on its own.
- Add a line to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md)
  for any user-visible change.
- [README.md](README.md) is the source and [README.ja.md](README.ja.md) its
  translation. Change both in the same pull request;
  `.github/scripts/check-readme-sync.sh` fails when their structure, code,
  links, or numbers differ.

## Test fixtures must be synthetic

Real session transcripts contain private conversations, customer names,
file paths, and sometimes secrets. **Never commit a real transcript, or any
excerpt of one, as a fixture or in an issue.** Write a minimal synthetic
JSONL line that reproduces the shape you need instead. Build fake secrets
at run time so secret scanners do not flag the source.

## Reporting bugs

Open an issue with the ccdig version, your OS, the command you ran, and a
synthetic JSONL line that reproduces the problem. For security issues,
follow [SECURITY.md](SECURITY.md) instead.

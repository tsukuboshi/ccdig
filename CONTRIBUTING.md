# Contributing to ccdig

Thanks for your interest in improving ccdig.

## Development setup

You need [rustup](https://rustup.rs/). [rust-toolchain.toml](rust-toolchain.toml)
pins the toolchain used for development and CI, with clippy and rustfmt;
`cargo` installs it on first use. For the other checks, install
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny),
[taplo](https://taplo.tamasfe.dev/), and
[rumdl](https://github.com/rvben/rumdl).

## Checks

Each of these must pass before a pull request is merged. CI runs the same
commands.

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check
taplo fmt --check
rumdl check .
.github/scripts/check-readme-sync.sh
```

Every clippy group, `restriction` included, is denied in
[Cargo.toml](Cargo.toml). The few lints that are allowed say why next to
them. In code, silence a lint only with
`#[expect(clippy::name, reason = "...")]`, so the exception goes away when
it is no longer needed. Upgrading the toolchain can add lints; review them
when you do.

## How we work

- Write a failing test first, then the smallest change that makes it pass.
- Tests return `Result` and compare with `check_eq` from `src/testing.rs`,
  since `assert_eq!` panics. Prefer table-driven tests.
- Rendered output is checked against golden files under `testdata/`;
  regenerate them with `UPDATE_GOLDEN=1 cargo test` and review the diff.
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

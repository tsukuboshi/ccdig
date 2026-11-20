# Changelog

All notable changes to this project will be documented in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A Rust rewrite of promptdig 0.1.0. `ccdig prompts`, `sessions`, and
  `digest` take the same flags and print the same output, with `promptdig`
  in messages read as `ccdig`.
- `ccdig replies`: the assistant's text blocks.
- `ccdig tools`: tool calls with their input as JSON.
- `ccdig results`: tool results, each with the tool and input of the call
  it answers.
- Filters for every command: `--grep` with `-i` (matched after masking),
  `--session` (ID prefix), `--branch`, and `--limit`. For replies, tools,
  and results: `--skill` and `--subagents`. For tools and results:
  `--tool`. For results: `--errors`.
- Masking of every string in a tool's input and of tool results.
- `ccdig` with no command prints the help and exits 0. Output to a closed
  pipe, as under `| head`, ends quietly.

### Changed

- `version` prints `ccdig v0.1.0` from the crate version.
- A URL password now ends at `?` and `#` as well as `/`, so
  `https://host:8443?email=a@b.com` is no longer masked as a password.
- Help text and the details of flag errors follow clap; the `ccdig:` prefix
  and the exit status 1 are kept.

### Removed

- The `completion` command cobra added.

[Unreleased]: https://github.com/tsukuboshi/ccdig/commits/main

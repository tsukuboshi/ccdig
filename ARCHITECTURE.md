# Architecture

ccdig is a pipeline. The command line and the environment become a
`Config`; transcripts flow from the files through decoding, picking,
masking, and filtering into records; and the records are rendered as text
for the shell to write. Input and output stay at the two ends, and every
stage in between is a function of the values it is given.

```text
            cli (shell)                         pipeline
argv, env ──► args ─► resolve ─► Config ──► collect ──► render ──► stdout, stderr
               ▲        ▲                     │
            clap only  environment         transcript
                        (env, git, TZ,     (walk, reader:
                         --mask-values      the files)
                         files)
```

## Code map

The modules, in the order they may depend on each other: each uses only
those before it.

| Module | Role |
| --- | --- |
| `failure` | The one error type, `Failure`. |
| `text` | Sanitizing text for the terminal and Markdown. |
| `when` | Timestamps, day ranges, and named time zones. |
| `transcript` | Producer: `walk` lists the files, `reader` streams their lines, `entry` decodes one line. |
| `classify` | Whether a user line is something a person typed, and folding slash commands back. |
| `redact` | The masking rules and what they counted. |
| `record` | The records that flow between the stages and that the JSON schema prints. |
| `select` | Tester: the filters, as `Query`. |
| `collect` | The stages joined: `read` picks one file's records as `Draft`s and filters them, `mask::Mask::seal` masks their metadata, and `gather` joins the files. `kinds` holds one picker per kind of record. |
| `render` | Consumer: JSON Lines, JSON, Markdown, and the digest. It uses only `record`, `text`, `when`, and `failure`. |
| `config` | `Config`, what a run does, as plain data. |
| `pipeline` | Runs a `Config`: collect, render, and add up what masking counted. |
| `cli` | The shell: `args` (clap), `environment` (everything read from outside), `resolve` (flags and environment into a `Config`), and writing the output and the exit status. |

## Invariants

These are checked, so a change that breaks one fails CI.
`.github/scripts/check-architecture.sh` also checks that this file agrees
with the code: the table lists the modules `src/lib.rs` declares, the
`layer-*` rules follow its order, and the files below that may do I/O are
the ones that say so.

- **Dependencies point one way.** A module uses only modules before it in
  the table above, or the ones its row names after "uses only". The
  `layer-*` rules under `.ast-grep/rules` check every `crate::` path;
  `no-super-super` and `no-super-in-top-level` close the ways around them.
  A new module needs a row in the table and a `layer-<module>.yml` rule
  with the modules it may not use.
- **clap stays in `cli/args.rs`.** The rest of the crate sees plain values,
  and `config` has its own enums for the formats (`clap-only-in-args`).
- **I/O happens only at the ends.** Reading the environment, the file
  system, other programs, the clock, and the system time zone is limited
  to `main.rs`, `cli/environment.rs`, `transcript/walk.rs`,
  `transcript/reader.rs`, and the tests. [clippy.toml](clippy.toml) lists
  the calls, and each of those files says it may make them with
  `#![expect(clippy::disallowed_methods, reason = "...")]`, which fails once
  the file stops needing it.
- **Text is masked before the filters, metadata after.** `--grep` searches
  masked text, and `--project` matches the real paths. A picked record is a
  `Draft`, and only `Mask::seal`, which masks the metadata, gives it out,
  so skipping that step does not compile.
- **Output does not depend on the machine.** Times come from the
  transcripts, the zone from `--tz` or `TZ`, and nothing reads the clock.

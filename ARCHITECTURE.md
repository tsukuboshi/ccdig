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
| `text` | Sanitizing text for the terminal, Markdown, and HTML. |
| `when` | Timestamps, the ranges of `--since` and `--until`, and named time zones. |
| `transcript` | Producer: `walk` lists the files and reads each sub-agent's `.meta.json`, which `meta` decodes; `reader` streams their lines, `entry` decodes one line. |
| `classify` | Whether a user line is something a person typed and a reply one the harness wrote, folding slash commands back, why a tool call did not run, and what the user answered through a dialog. |
| `redact` | The masking rules: what each replaces and with what, and what they counted. `collect::mask` decides where in the pipeline they apply. |
| `record` | The records that flow between the stages and that the JSON schema prints. |
| `stats` | Grouping records into the lines of `ccdig stats`: the keys `--by` takes, and what each group counts and adds up. |
| `filter` | Tester: the filters, as `Query`. |
| `collect` | The stages joined: `read` picks one file's records as `Draft`s and filters them, `mask::Mask::seal` masks their metadata, and `gather` joins the files. `kinds` holds one picker per kind of record, and `excerpt` the lines `--context` lists. |
| `render` | Consumer: JSON Lines, JSON, Markdown, HTML, and the digest. `view` decides what the records show and Markdown and HTML write it, in the words `words` holds for each `--lang`. HTML writes only through `html::markup::Page`, which takes text escaped and markup as `'static` strings, and `html::prose` renders Markdown with raw HTML, links off the web, and images taken out. It uses only `record`, `text`, `when`, and `failure`. |
| `config` | `Config`, what a run does, as plain data. |
| `pipeline` | Runs a `Config`: collect, render, and add up what masking counted. |
| `cli` | The shell: `args` (clap), `environment` (everything read from outside, time zones included), `resolve` (flags and environment into a `Config`), and writing the output and the exit status. |

## Invariants

These are checked, so a change that breaks one fails CI.
`.github/scripts/check-architecture.sh` also checks that this file agrees
with the code: the table lists the modules `src/lib.rs` declares, each
`layer-*` rule is the one the script writes from the table with `--write`,
the files below that may do I/O are the ones that say so, and the property
files below are the ones there are.

- **Dependencies point one way.** A module uses only modules before it in
  the table above, or the ones its row names after "uses only". The
  `layer-*` rules under `.ast-grep/rules` check every `crate::` path,
  in macro calls and attributes too; `no-super-super`,
  `no-super-in-top-level`, and `no-crate-alias-or-glob` close the ways
  around them. A new module needs a row in the table; then run
  `.github/scripts/check-architecture.sh --write` to write its rule.
- **clap stays in `cli/args.rs`.** The rest of the crate sees plain values,
  and `config` has its own enums for the formats (`clap-only-in-args`).
- **I/O happens only at the ends.** Reading the environment, the file
  system, other programs, the clock, the system time zone, and the host's
  zone database is limited to `main.rs`, `cli/environment.rs`,
  `transcript/walk.rs`, `transcript/reader.rs`, and the tests.
  [clippy.toml](clippy.toml) lists the calls, and each of those files says
  it may make them with
  `#![expect(clippy::disallowed_methods, reason = "...")]` at its top, which
  fails once the file stops needing it. An `#[expect]` on a single item
  would hide I/O from this list, so `io-expect-only-at-file-level` denies
  it. The tests look zones up in the database bundled into the binary.
- **Text is masked before the filters, metadata after.** `--grep` searches
  masked text, and `--project` matches the real paths. A picker returns
  each record as a `Draft`, with its text masked, and only `Mask::seal`,
  which masks the metadata, gives the record out, so code that skips that
  step does not compile. The lines `--context` lists are cut from the
  masked text, after the filters. The session titles are text Claude Code
  generates from the conversation, so they are masked as text: the title
  then once each time it changes while the file is read, and the last
  title once the file is read, in `Mask::seal`. No filter reads either.
  Markdown and HTML mask once more, after the filters: `Mask::indent_json`
  indents a tool's output that is JSON and masks the strings decoding
  brings out, which the record's counts add up.
- **The machine reaches the output only through `cli/environment.rs`.**
  Nothing reads the clock; times come from the transcripts. What the
  machine decides is read there: the system zone when neither `--tz` nor
  `TZ` names one, and the host's zone database, which comes before the
  bundled copy.
- **Properties hold for any input.** The modules that read what a
  transcript or the command line holds say what must hold whatever it is,
  in a properties file next to their tests:
  `text/tests/properties.rs`, `when/tests/properties.rs`,
  `transcript/entry/tests/properties.rs`,
  `transcript/reader/tests/properties.rs`,
  `classify/denial/tests/properties.rs`,
  `classify/human/tests/properties.rs`,
  `classify/response/tests/properties.rs`,
  `classify/slash/tests/properties.rs`, `redact/tests/properties.rs`,
  `stats/tests/properties.rs`, `filter/tests/properties.rs`,
  `collect/tests/properties.rs`,
  `render/tests/properties.rs`, and `cli/tests/properties.rs`. Each test
  there runs through `testing::for_all` (`property-test-calls-for-all`),
  which writes no failing input to disk, as one can look like a secret.
  proptest is used nowhere else
  (`proptest-only-in-properties`), and never through its own runner
  (`properties-through-for-all`). A new properties file needs a place in
  this list.

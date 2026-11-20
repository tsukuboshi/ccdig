# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Dig prompts, replies, tool calls, and tool results out of Claude Code
session transcripts.

[日本語](README.ja.md)

## What it does

Claude Code stores every session as JSONL under `~/.claude/projects/`. The
lines mix what you typed with what the harness wrote: tool calls, tool
results, injected skill bodies, `<system-reminder>` blocks, task
notifications, compaction summaries, and sub-agent traffic.

ccdig reads those transcripts and prints one kind of record at a time:

- **Prompts** you typed, with harness noise dropped and slash commands
  folded back into `/command args`.
- **Replies**: the assistant's text.
- **Tool calls**, with their input as JSON.
- **Tool results**, each paired with the call it answers.

Every kind shares the same filters (date range, project, regular
expression, session, branch) and is masked for common secrets by default.

## Why it is useful

Viewers and converters are good at reading a whole session. ccdig is for
pulling one slice out of all of them: what you asked for this week, every
`Bash` call that failed, or the replies that mention a keyword. It decides
which user lines a person typed, masks secrets by default, writes JSON
Lines that pipe into `jq`, and ships as a single binary.

## Getting started

Requires Rust 1.99 or later.

```sh
cargo install --git https://github.com/tsukuboshi/ccdig
ccdig version
```

To build from a clone and run the tests:

```sh
git clone https://github.com/tsukuboshi/ccdig && cd ccdig
cargo test --locked
cargo install --path . --locked
```

```sh
# Prompts from one week, one JSON object per line
ccdig prompts --since 2026-10-05 --until 2026-10-11

# Replies in the blog project that mention "deploy", ignoring case
ccdig replies --project blog --grep deploy -i

# Failed Bash calls, with sub-agents, as Markdown
ccdig results --tool Bash --errors --subagents --format md

# A size-capped Markdown digest of the prompts to paste into an LLM
ccdig digest --since 2026-10-05 --until 2026-10-11
```

### Commands

| Command | Output |
| --- | --- |
| `prompts` | One record per prompt a person typed |
| `replies` | One record per text block of the assistant |
| `tools` | One record per tool call |
| `results` | One record per tool result, with the call's tool and input |
| `sessions` | One record per session with a selected prompt |
| `digest` | Markdown digest of the prompts |
| `version` | The installed version |

`prompts`, `replies`, `tools`, `results`, and `sessions` take `--format`:
`jsonl` (default), `json`, or `md`. `digest` takes `--max-prompt-chars`
(default 300) and `--max-session-chars` (default 2000).

### Filters

| Flag | Commands | Keeps |
| --- | --- | --- |
| `--since`, `--until` | all | Records from the first to the last day, `YYYY-MM-DD` |
| `--tz` | all | Day boundaries and Markdown times in this IANA zone |
| `--project` | all | Projects whose directory name or cwd contains the value |
| `--grep`, `-i` | all | Records whose text matches the regular expression |
| `--session` | all | Sessions whose ID starts with the value |
| `--branch` | all | Records on this git branch |
| `--limit` | all | The first N records |
| `--skill` | replies, tools, results | Records written while the skill ran |
| `--subagents` | replies, tools, results | Sub-agent transcripts too |
| `--tool` | tools, results | Calls to this tool |
| `--errors` | results | Results the tool reported as failures |

`--project`, `--session`, `--branch`, and `--tool` are repeatable and keep
records that match any value. The shared flags go before or after the
command. `--tz` defaults to the system time zone (`TZ`, else
`/etc/localtime`). `--root` sets the transcript root (default
`~/.claude/projects`), and `--no-redact` turns masking off.

`--grep` searches the text as it will be printed: after masking, so a
secret cannot be found by its value. It reads the prompt or reply text, a
call's input as compact JSON, or a result's text. For `sessions` and
`digest`, the filters apply to the prompts.

### Output

`ccdig prompts` writes one object per line:

```json
{"agent":"claude-code","session_id":"<uuid>","session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T01:00:01Z",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"}}
```

The other kinds start with the same keys, from `agent` to `timestamp`, and
end with `redacted` and `source`:

| Command | Keys in between |
| --- | --- |
| `replies` | `text`, `model`, `skill`, `subagent` |
| `tools` | `tool`, `input`, `tool_use_id`, `skill`, `subagent` |
| `results` | `tool`, `input`, `tool_use_id`, `is_error`, `text`, `subagent` |

Every key is always present. `subagent` is empty for the session itself and
the file name, such as `agent-1a2b`, for a sub-agent. A result whose call
was not found has an empty `tool` and a `null` `input`. `--format json`
wraps the records as `{"schema":1,"filter":"strict-v2","items":[...]}`.
`filter` names the rule set below and changes whenever a rule change can
change which prompts are kept.

## What counts as a typed prompt

A `user` entry is dropped, with the first matching reason, when it:

1. is marked `isMeta`, `isSidechain`, or `isCompactSummary`;
2. has an `origin.kind` other than `human`;
3. carries a `tool_result` block;
4. starts with a harness tag such as `<system-reminder>`,
   `<task-notification>`, `<local-command-…>`, `<bash-input>`,
   `<bash-stdout>`, or `<user-prompt-submit-hook>`;
5. is empty, or is the `[Request interrupted by user` marker.

Rule 4 applies even when `origin.kind` is `human`, because Claude Code
records a `!` shell escape as human input. Sessions recorded before Claude
Code wrote `origin` skip rule 2. Thinking blocks are never printed.

Only regular files are read: `<root>/<project>/<session>.jsonl`, and with
`--subagents` also `<root>/<project>/<session>/subagents/*.jsonl`. Symlinks
are not followed. A resumed session repeats earlier entries; each entry
(by `uuid` and position in the line) is listed once, in the original
session. A directory or file that cannot be read, and lines that cannot be
decoded, have no timestamp, or exceed 64 MiB, are skipped and reported on
stderr while the rest of the output is still written.

## Redaction

On by default. Matches are replaced with `***REDACTED***`:

- AWS credentials: `AWS_SECRET_ACCESS_KEY`-style names with `=` or `:`,
  credentials files, `aws sts` JSON (`SecretAccessKey`, `SessionToken`),
  and bare `AKIA` / `ASIA` access key IDs
- GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), GitLab,
  and npm tokens
- Anthropic and OpenAI `sk-` keys, Google API keys, and Stripe keys
- Slack tokens and webhook URLs, and JWTs
- Passwords in URLs (`postgres://user:…@host`)
- PEM and PGP private key blocks

Session titles, every string in a tool's input, and tool results are
masked too.

The patterns need a distinctive prefix or variable name, so they miss
secrets without one. Tool results hold whatever the tool read: file
contents, environment variables, command output. Treat masking as a safety
net and check the output yourself before you share it.

Markdown and digest output treat transcript fields as untrusted: terminal
control characters and bidi overrides become U+FFFD, metadata is flattened
to one line, titles escape inline Markdown and HTML, and digest prompts sit
in code spans.

## Scope

- Claude Code only.
- ccdig is a Rust rewrite of
  [promptdig](https://github.com/tsukuboshi/promptdig). `prompts`,
  `sessions`, and `digest` print the same output as promptdig 0.1.0.

## Getting help

Open an issue at <https://github.com/tsukuboshi/ccdig/issues>. Never attach
a real transcript; reproduce the problem with a synthetic line. To report a
vulnerability, see [SECURITY.md](SECURITY.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Maintainer

[@tsukuboshi](https://github.com/tsukuboshi)

## License

[MIT](LICENSE)

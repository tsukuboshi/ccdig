# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Pull the logs you want to show someone out of your Claude Code session
transcripts, formatted and masked. Combine filters for time, project,
session, branch, text, kind of record, tool, outcome, and skill to narrow
the history down to the part that matters, and write it as Markdown to
read or JSON Lines to process. Common secrets are masked by default, and
email addresses, IP addresses, your user name, and values you list can be
masked too.

[日本語](README.ja.md)

## Getting started

Requires Rust 1.99.1 or later.

```sh
cargo install --locked --git https://github.com/tsukuboshi/ccdig
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
ccdig records --type prompt --since 2026-10-05 --until 2026-10-11

# Replies in the blog project that mention "deploy", ignoring case
ccdig records --type reply --project blog --igrep deploy

# Failed Bash calls, with sub-agents, as Markdown
ccdig records --type result --tool Bash --status error --subagents --format md

# A size-capped Markdown digest of the prompts to paste into an LLM
ccdig digest --since 2026-10-05 --until 2026-10-11

# One session as a thread to share, with emails, IP addresses, the user
# name, and the values in a file masked too
ccdig records --session 1a2b --format md --mask email,ip,user --mask-file secrets.txt
```

### Commands

| Command | Output |
| --- | --- |
| `records` | One record per prompt, reply, tool call, or tool result, of the kinds `--type` picks, in the order they were written |
| `sessions` | One record per session with a selected prompt |
| `digest` | Markdown digest of the prompts |
| `version` | The installed version |

`--type` takes `prompt` (a prompt a person typed), `reply` (a text block
of the assistant), `call` (a tool call), and `result` (a tool result, with
the call's tool and input), comma-separated or repeated, and defaults to
`prompt,reply,call`. `records` and `sessions` take `--format`: `jsonl`
(default), `json`, or `md`. `digest` takes `--max-prompt-chars` (default
300) and `--max-session-chars` (default 2000).

### Filters

| Flag | Commands | Keeps |
| --- | --- | --- |
| `--since`, `--until` | all | Records from the first to the last day, `YYYY-MM-DD` |
| `--tz` | all | Day boundaries and Markdown times in this IANA zone |
| `--project` | all | Projects whose directory name or cwd contains the value |
| `--session` | all | Sessions whose ID starts with the value |
| `--branch` | all | Records on this git branch |
| `--grep` | all | Records whose text matches the regular expression |
| `--igrep` | all | The same, ignoring case |
| `--limit` | all | The first N records |
| `--tool` | records: `call`, `result` | Calls to this tool and their results |
| `--status` | records: `result` | Results with this status: `error` or `ok` |
| `--skill` | records: `reply`, `call`, `result` | Records written while the skill ran |
| `--subagents` | records: `reply`, `call`, `result` | Sub-agent transcripts too |

`--project`, `--session`, `--branch`, `--grep`, `--igrep`, `--tool`, and
`--skill` are repeatable and keep records that match any value. A value
with a leading `!` leaves out the records that match it instead, so
`--session '!1a2b'` keeps every session but one and `--grep '!WIP'` drops
the records that mention `WIP`; write `\!` for a value that starts with
`!`. Quote such a value, as `!` starts a history expansion in most
shells. `--status` takes a comma-separated list like `--type`.

`--tool`, `--status`, and `--skill` narrow only the kinds that have what
they test and let the other kinds through, so
`ccdig records --type prompt,reply,call --skill tdd` keeps every prompt
along with the replies and calls written while `tdd` ran. Without
`--type`, `records` lists only the kinds every one of them given applies
to: `ccdig records --tool Bash` prints the `Bash` calls and their results.
Each of the last four flags needs a `--type` it applies to, or the
command fails.

The shared flags go before or after the command, and the values of a
repeatable flag on both sides add up. `--tz` defaults to the system time
zone (`TZ`, else `/etc/localtime`). `--root` sets the transcript root
(default `~/.claude/projects`). `--mask` and `--mask-file` add masks, and
`--no-mask` turns all masking off; see Masking.

`CCDIG_ROOT`, `CCDIG_MASK`, and `CCDIG_MASK_FILE` set the defaults of
`--root`, `--mask` (comma-separated), and `--mask-file` (paths separated
as in `PATH`). The flag on the command line replaces its variable,
`--no-mask` ignores the mask variables, and an empty variable counts as
unset.

`--grep` searches the text as it will be printed: after masking, so a
secret cannot be found by its value. It reads the prompt or reply text, a
call's input as compact JSON, or a result's text. For `sessions` and
`digest`, the filters apply to the prompts.

### Output

`ccdig records --type prompt` writes one object per line:

```json
{"type":"prompt","agent":"claude-code","session_id":"<uuid>",
 "session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T01:00:01Z",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"}}
```

Every record starts with `type`, the kind `--type` names. The other kinds
have the same keys from `type` to `timestamp`, and end with `redacted` and
`source`:

| Type | Keys in between |
| --- | --- |
| `reply` | `text`, `model`, `skill`, `subagent` |
| `call` | `tool`, `input`, `tool_use_id`, `skill`, `subagent` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `text`, `subagent` |

Every key is always present. `subagent` is empty for the session itself and
the file name, such as `agent-1a2b`, for a sub-agent, whose record carries
the `session_id` and `session_title` of its session. The title is empty
when the session's own transcript was last written before `--since`.
`input` is the
untruncated input from the line's `wireToolInputs` when Claude Code stored
one, and the `tool_use` block's otherwise. A result whose call
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

## Masking

Always on unless `--no-mask` is given. Matches are replaced with
`***REDACTED***`:

- AWS credentials: `AWS_SECRET_ACCESS_KEY`-style names with `=` or `:`,
  credentials files, `aws sts` JSON (`SecretAccessKey`, `SessionToken`),
  and bare `AKIA` / `ASIA` access key IDs
- GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), GitLab,
  and npm tokens
- Anthropic and OpenAI `sk-` keys, Google API keys, OAuth tokens, and
  client secrets, Stripe keys and webhook secrets, and SendGrid keys
- Slack tokens and webhook URLs, and JWTs
- `Authorization` header credentials and `Bearer` tokens
- Passwords in URLs (`postgres://user:…@host`) and glued to the `-p` of
  `mysql` and its relatives
- PEM and PGP private key blocks
- Values assigned to a credential-like name: `name=value`, `name: value`,
  `name => value`, `--name=value`, `?name=value`, the same inside escaped
  JSON (`\"password\":\"...\"`), and the Japanese words for password and
  token. A name is split at `_`, `.`, `-`, and camelCase; a strong word
  such as `token`, `secret`, or `password` counts alone or at the end of a
  part (`PGPASSWORD`), a weak word such as `key` or `auth` only next to
  another part (`api_key`, not `key`). A quoted value is masked to its
  closing quote, so a passphrase with spaces goes whole. Names about paths,
  URLs, or database keys (`token_path`, `primary_key`), digits behind a
  quantity (`token_limit: 100000`), and values that are references (`$VAR`,
  `ENV[...]`, `<placeholder>`, `%s`, `\u0000`), code (`self.token`,
  `config[`, `last_token`), or type names (`token: string`) are left alone.
  An unquoted value must be printable ASCII, so prose after the separator
  stays
- In a tool's input, a string under a key that names a credential, as in
  `{"password": "..."}`, by the same test

Session titles, every string in a tool's input, and tool results are
masked too.

More masks are off by default and can be added to any command:

| Flag | Masks |
| --- | --- |
| `--mask email` | Email addresses, but not an SSH remote such as `git@github.com:` |
| `--mask ip` | IPv4 addresses other than `127.x.x.x`, `0.0.0.0`, and `255.255.255.255`, and IPv6 addresses of three groups or more |
| `--mask user` | The user name, the last part of `$HOME`, in any case and with any of `.`, `_`, and `-` between its parts, so also in a project directory name such as `-Users-alice-b-blog` |
| `--mask-file FILE` | Each non-empty line of the file, trimmed, wherever it appears, as typed (case matters) |

`--mask` takes a comma-separated list and is repeatable, as in
`--mask email,user`, and so is `--mask-file`. These masks also apply to
the fields that are not text: `project`, `cwd`, `git_branch`, file paths,
`tool`, `model`, `subagent`, `command`, and the warnings on stderr. The
keys of a tool's input stay as they are. An IPv4 rule cannot tell a bare
version number such as `1.2.3.4` from an address, so it masks that too;
one after a letter or a dot, as in `v1.2.3.4`, stays. A part of the user
name alone, such as a first name, stays; add it to a mask file.

`--no-mask` prints everything as it was written, secrets included, for
checking the masks on your own machine. It cannot be combined with
`--mask` or `--mask-file`.

The patterns need a distinctive prefix or a credential-like name, so they
miss secrets without one, such as `--password value` with a space or a
value that looks like a `snake_case` identifier. Tool results hold
whatever the tool read: file contents, environment variables, command
output. Tool inputs, which `records` prints by default, hold what was written,
such as the content of a `Write` call. Treat masking as a safety net and
check the output yourself before you share it.

Markdown and digest output treat transcript fields as untrusted: terminal
control characters and bidi overrides become U+FFFD, metadata is flattened
to one line, titles escape inline Markdown and HTML, and digest prompts sit
in code spans.

## Scope

- Claude Code only.

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

# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Pull the logs you want to share out of your Claude Code session
transcripts, formatted and masked. Combine filters for time, project,
session, branch, text, record type, tool, outcome, and skill to narrow
the history down to the part that matters. `records` and `sessions`
write JSON Lines to process by default, on a terminal and in a pipe
alike; give `--format md` for Markdown to read. Common secrets are
masked by default, and email addresses, IP addresses, your user name,
and values you list can be masked too.

[日本語](README.ja.md)

## Install

Each [release](https://github.com/tsukuboshi/ccdig/releases) has a
prebuilt binary for macOS on Apple silicon. Download it with `curl`
rather than a browser, so macOS does not quarantine the unsigned binary.
This puts `ccdig` in `~/.local/bin`; add that directory to your `PATH` if
it is not there yet:

```sh
base=https://github.com/tsukuboshi/ccdig/releases/latest/download
curl -fsSLO "$base/ccdig-aarch64-apple-darwin.tar.gz"
curl -fsSLO "$base/ccdig-aarch64-apple-darwin.tar.gz.sha256"
shasum -a 256 -c ccdig-aarch64-apple-darwin.tar.gz.sha256
tar -xzf ccdig-aarch64-apple-darwin.tar.gz
install -d ~/.local/bin
install ccdig-aarch64-apple-darwin/ccdig ~/.local/bin/
~/.local/bin/ccdig version
```

On other platforms, build it with Rust 1.100.0 or later:

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

## Quick start

```sh
# Prompts from one week, one JSON object per line
ccdig records --type prompt --since 2026-10-05 --until 2026-10-11

# Replies in the blog project that mention "deploy", ignoring case
ccdig records --type reply --project blog --igrep deploy

# From Claude Code, search the other sessions' prompts and replies, 20 at
# most, leaving out the session doing the search
ccdig records --session '!current' --type prompt,reply --igrep deploy \
  --format md --limit 20

# The sessions that mention "deploy" most, each with the line it first
# matched, without the tokens
ccdig sessions --igrep deploy --sort matches --fields '!usage' --limit 10

# Failed Bash calls, with sub-agents, as Markdown
ccdig records --type result --tool Bash --status error --subagents --format md

# A size-capped Markdown digest of the prompts to paste into an LLM
ccdig digest --since 2026-10-05 --until 2026-10-11

# One session as a thread to share, with emails, IP addresses, the user
# name, and the values in a file masked too
ccdig records --session 1a2b --format md --mask email,ip,user \
  --mask-values @secrets.json
```

## Commands

| Command | Lists | Own options |
| --- | --- | --- |
| `records` | One record per prompt, reply, tool call, tool result, or summary, of the kinds `--type` picks, in the order they were written | `--type`, `--tool`, `--status`, `--denial`, `--skill`, `--subagents`, `--around`, `--context`, `--format`, `--fields`, `--max-message-chars`, `--max-tool-chars` |
| `sessions` | One record per session with a selected prompt | `--format`, `--fields`, `--sort` |
| `digest` | A size-capped Markdown digest of the prompts | `--max-prompt-chars`, `--max-session-chars` |
| `version` | The installed version | `--format` |

Every command also takes the options under Shared options.

### records

`--type` picks the kinds of record, comma-separated or repeated, and
defaults to `prompt,reply,call`:

| Type | Record |
| --- | --- |
| `prompt` | A prompt a person typed |
| `reply` | A text block of the assistant |
| `call` | A tool call |
| `result` | A tool result, with the call's tool and input |
| `summary` | The summary Claude Code writes when it compacts the context |

Five options apply to some kinds only:

| Flag | Applies to | Keeps |
| --- | --- | --- |
| `--tool` | `call`, `result` | Calls to this tool and their results |
| `--status` | `result` | Results with this status: `error` or `ok` |
| `--denial` | `result` | Results of calls that did not run for this reason, such as `user-rejected` |
| `--skill` | `reply`, `call`, `result` | Records written while the skill ran |
| `--subagents` | `reply`, `call`, `result` | Sub-agent transcripts too |

- `--tool`, `--denial`, and `--skill` are repeatable and take a leading
  `!`, as the shared filters do. `--status` takes a comma-separated list
  like `--type`.
- `--tool`, `--status`, `--denial`, and `--skill` narrow only the kinds that have what
  they test and let the other kinds through, so
  `ccdig records --type prompt,reply,call --skill tdd` keeps every prompt
  along with the replies and calls written while `tdd` ran.
- Without `--type`, `records` lists only the kinds that every one of
  `--tool`, `--status`, `--denial`, and `--skill` given applies to:
  `ccdig records --tool Bash` prints the `Bash` calls and their results.
- Each of the five needs a `--type` it applies to, or the command fails.
- A search without `--type` reads the tool calls too, and a call's input
  can be long: give `--type prompt,reply` and `--limit` to keep the output
  small, or cut the texts with `--max-message-chars` and `--max-tool-chars`
  and leave keys out with `--fields`.
- Sub-agent transcripts are left out unless `--subagents` is given. The
  instruction a sub-agent was given is not a prompt a person typed, so it
  is never listed; its replies, calls, and results are.
- `--around N` also lists the N records before and after each record
  `--grep` or `--igrep` matched, such as the prompt a matched reply
  answered: records of the types `--type` picks that pass every other
  filter, from the same file. They have `matched` false, and in Markdown
  `(context)` after their kind. A record a `!` pattern leaves out is not
  listed as context either. It needs a pattern without a leading `!`.

Four options shape the output:

| Flag | Does |
| --- | --- |
| `--context N` | Lists the lines `--grep` or `--igrep` matched in each record, with N lines before and after each, in `matches`. Markdown prints those lines in place of the whole text. Without it, JSON and JSON Lines list the matched lines alone |
| `--format` | `jsonl` (default) writes one JSON object per line, `json` one document, and `md` Markdown to read |
| `--max-message-chars N` | Cuts the text of each prompt, reply, and summary to N characters, followed by `… (M more characters)` |
| `--max-tool-chars N` | Cuts each string in a tool's input, and each tool result's output, to N characters, so a `Write` of a whole file does not fill the output |

### sessions

One record per session that has a selected prompt: the filters apply to
the prompts. `--format` takes `jsonl` (default), `json`, or `md`.

- `--grep` and `--igrep` pick the sessions whose prompts or replies they
  match, and `match_count` says how many of them matched in each, so the
  session that mentions a word most can be read first. The other keys
  come from every prompt the other filters keep, as without `--grep`,
  less those a pattern with a leading `!` leaves out, and Markdown adds a
  `Matches` column. Without a pattern that does not start
  with `!`, `match_count` is `null`.
- `prompt_count` and `started_at` count the selected prompts and give the
  first one's time. `ended_at` is the time of the session's last prompt,
  reply, tool call, or tool result in the range, so it shows when the
  work ended even when a session has one prompt.
- `first_match` holds the first prompt or reply a search matched in the
  session, so it can be judged without reading its records: its `type`,
  `timestamp`, the `line` a pattern matched in its text, and that line,
  masked, on one line and cut to 200 characters, as `text`. When the
  match spans lines, `line` is `null` and `text` starts the whole text.
  Without a pattern that does not start with `!`, `first_match` is
  `null`. Markdown adds a `First match` column next to `Matches`.
- `--sort matches` lists the sessions with the most matches first, and
  those with as many in time order; `--limit` then keeps the first N of
  them. It needs a pattern that does not start with `!`. The default,
  `--sort time`, lists them by their first selected prompt.

A record has these keys, in this order:

| Key | Holds |
| --- | --- |
| `agent` | `claude-code` |
| `session_id` | The session ID |
| `title` | The session's last title, or empty |
| `project` | The project directory's name |
| `cwd`, `git_branch` | Those of the first selected prompt |
| `file` | The transcript file |
| `mtime` | When the file was last written to |
| `started_at`, `ended_at`, `prompt_count`, `match_count`, `first_match` | As above |
| `usage` | The tokens; see Tokens |

### digest

A Markdown digest of the prompts, capped in size. The filters apply to
the prompts.

| Flag | Default | Does |
| --- | --- | --- |
| `--max-prompt-chars N` | 300 | Truncates each prompt to N characters |
| `--max-session-chars N` | 2000 | Lists at most N characters per session |

### version

`ccdig version` prints the installed version. With `--format json` it
prints the version together with `schema` and `filter`, described under
JSON envelope:

```json
{"version":"0.0.3","schema":4,"filter":"strict-v2"}
```

## Shared options

Every command takes these, before or after the command name. The values
of a repeatable flag on both sides add up.

### Filters

| Flag | Keeps |
| --- | --- |
| `--since`, `--until` | Records from the first to the last day, `YYYY-MM-DD` |
| `--tz` | Day boundaries and the times in the output, Markdown and JSON alike, in this IANA zone. Defaults to the system time zone (`TZ`, else `/etc/localtime`) |
| `--project` | Projects whose directory name or cwd contains the value |
| `--session` | Sessions whose ID starts with the value, or with `current`, the session ccdig runs in |
| `--branch` | Records on this git branch |
| `--grep` | Records whose text matches the regular expression |
| `--igrep` | The same, ignoring case |

- `--project`, `--session`, `--branch`, `--grep`, and `--igrep` are
  repeatable and keep records that match any value.
- A value with a leading `!` leaves out the records that match it instead,
  so `--session '!1a2b'` keeps every session but one and `--grep '!WIP'`
  drops the records that mention `WIP`. Write `\!` for a value that starts
  with `!`, and quote such a value, as `!` starts a history expansion in
  most shells.
- `--session current` is the session Claude Code runs ccdig in, read from
  `CLAUDE_CODE_SESSION_ID`, so `--session '!current'` leaves out the
  session that is doing the search. Outside Claude Code, `current` is an
  error.
- `--grep` searches the text as it will be printed: after masking, so a
  secret cannot be found by its value. It reads the prompt or reply text,
  a call's input as compact JSON, or a result's text.

### Input

| Flag | Does |
| --- | --- |
| `--root` | Reads the transcripts under this directory. Defaults to what Environment variables set, else `~/.claude/projects` |
| `--on-skip` | Says what to do when a file or line cannot be read: `warn` (default) reports it and goes on, and `fail` also ends with status 1 after writing the output. See Skipped lines |

### Masking

Common secrets are masked without any flag. These flags change what is
masked; the Masking section lists the rules.

| Flag | Does |
| --- | --- |
| `--mask` | Adds masks, comma-separated or repeated: `aws-account`, `aws-profile`, `email`, `git-user`, `ip`, and `user` |
| `--mask-values` | Masks the values in a JSON object of labels to lists of values, or with a leading `@`, in a file. Repeatable |
| `--mask-report` | Writes on stderr how many times each rule masked something, as `text` or `json` |
| `--no-mask` | Turns all masking off. Cannot be combined with `--mask` or `--mask-values` |

### Output

`--limit N` prints the first N records. With `--around`, it counts the
records that matched and keeps the context after the last of them.
`--format` belongs to each command; see Commands.

`records` and `sessions` take `--fields` too: in JSON and JSON Lines it
writes only the keys it names, comma-separated or repeated, in the order
the records write them. A key with a leading `!` is left out instead, as
with the filters, so
`--fields '!agent,!session_title,!project,!cwd,!git_branch'` drops the keys
every record of a session repeats, and `sessions --fields '!usage'` drops
the tokens. A key the command does not write is an error, and so is
`--fields` with `--format md`.

### Environment variables

| Variable | Sets the default of | Value |
| --- | --- | --- |
| `CCDIG_ROOT` | `--root` | A directory. It comes before `CLAUDE_CONFIG_DIR` |
| `CLAUDE_CONFIG_DIR` | `--root` | Claude Code's own variable; its `projects` directory is the root |
| `CLAUDE_CODE_SESSION_ID` | `--session current` | Claude Code's own variable; the ID of the session it runs ccdig in |
| `CCDIG_ON_SKIP` | `--on-skip` | `warn` or `fail` |
| `CCDIG_MASK` | `--mask` | A comma-separated list |
| `CCDIG_MASK_VALUES` | `--mask-values` | One JSON object, or `@` and a file |

- The flag on the command line replaces its variable.
- `--no-mask` ignores the mask variables.
- An empty variable counts as unset.

## Output format

### Record keys

`ccdig records --type prompt` writes one object per line:

```json
{"type":"prompt","agent":"claude-code","session_id":"<uuid>",
 "session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T10:00:01+09:00",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "permission_mode":"","redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"},
 "matched":true,"matches":[]}
```

Every record starts with `type`, the kind `--type` names. The other kinds
have the same keys from `type` to `timestamp`, and end with `redacted`,
`source`, `matched`, and `matches`:

| Type | Keys in between |
| --- | --- |
| `reply` | `text`, `model`, `skill`, `subagent`, `usage` |
| `call` | `tool`, `input`, `tool_use_id`, `skill`, `subagent`, `usage` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `denial`, `text`, `subagent` |
| `summary` | `text` |

Every key is always present unless `--fields` leaves it out. The `kind` of a prompt is how it was typed,
`slash` or `text`; the record type is `type`, which `--type` picks.

- `matched` is `false` for a record `--around` lists next to a match, and
  `true` for every other record.
- `matches` lists, with `--context`, the lines `--grep` or `--igrep`
  matched and those around them, in order: `field` is the text the line is
  in, `text` for every kind but a call, and for a call where the string is
  in its input, such as `input.command` or `input.edits[0].new_string`,
  with a key other than letters, digits, `_`, and `-` quoted, as in
  `input["a.b"]`; `line` is the line's 1-based
  number in that text, `text` the line, masked, and `hit` whether a
  pattern matched it. Each pattern is tried on each line alone, so `^` and
  `$` match at the start and end of each line, and a match that spans
  lines, is in a call's key names, or needs the escapes of the call's
  compact JSON, such as `\\`, lists no line, and the record is printed
  whole. Without `--context`, JSON and JSON Lines list the matched lines
  alone, and Markdown lists none, so it prints each record whole.
  `matches` is empty without a pattern that does not start with `!`.
- `denial` is Claude Code's name for why a call did not run, such as
  `user-rejected`, `permission-rule`, `automode-blocked`, or `cancelled`,
  and empty when it ran. Before Claude Code recorded it, only
  `user-rejected` is read, from the result's text. `is_error` is `true` for
  each of them too.
- `permission_mode` is the mode Claude Code recorded on the prompt's line,
  such as `default`, `plan`, or `auto`. It is empty when the line does not
  say: Claude Code writes none for a slash command or in older sessions.
- `subagent` is empty for the session itself and the file name, such as
  `agent-1a2b`, for a sub-agent. A sub-agent's record carries the
  `session_id` and `session_title` of its session; the title is empty when
  the session's own transcript was last written before `--since`.
- `input` is the `tool_use` block's input with the untruncated values
  from the line's `wireToolInputs` laid over it when Claude Code stored
  them, so the keys the model did not send, such as the `plan` of
  `ExitPlanMode`, stay. A result
  whose call was not found has an empty `tool` and a `null` `input`.
- `usage` is the tokens of the response; see Tokens.
- `timestamp`, like the times of a `sessions` record, is RFC 3339 with
  the offset of the zone `--tz` names at that time, as in
  `2026-10-13T10:00:01+09:00`, so the times read in the zone the days
  were cut in. UTC is written `Z`.

### Tokens

`usage` holds the tokens of a response:

```json
{"input":12,"output":340,"cache_read":51000,"cache_creation":800}
```

- A response is written over several lines, each with the same tokens, so
  only the first reply or call that the filters keep of a response in a
  file carries them. The others have `null`, as do lines without tokens.
- The records do not always add up to the whole. A response with no reply
  or call, such as one of thinking alone, has no record to carry its
  tokens, and neither has one whose records `--type` or a filter all
  leave out.
- A `sessions` record ends with the same four keys in `usage`: the sum
  over the session's responses in the range, each once. For it,
  `sessions` also reads the assistant lines.

### JSON envelope

`--format json` wraps the records with the versions and the flags of the
run:

```json
{"schema":4,"filter":"strict-v2","ccdig":"0.0.3",
 "run":{"since":"2026-10-05","until":"2026-10-11","tz":"Asia/Tokyo",
        "mask":["email","user"],"mask_values":["repo","person"],
        "no_mask":false},
 "items":[...]}
```

| Key | Holds |
| --- | --- |
| `schema` | A number that goes up whenever a key of the records or the envelope is added, removed, or changes meaning |
| `filter` | The name of the rule set under Typed prompts, which changes whenever a rule change can change which prompts are kept |
| `ccdig` | The version that wrote the document |
| `run` | `--since`, `--until`, and `--tz` as given, `null` when not given, the `--mask` names, the labels of `--mask-values` without their values, and `--no-mask`, so a gap in the records can be told apart from a narrower run |
| `items` | The records |

A script can check `schema` and `filter` with
`ccdig version --format json` before it reads the output.

### Markdown

`records --format md` puts the records of each transcript file under one
heading with the project and session, and each record under a heading
with its time and kind. A sub-agent's record names the sub-agent at the
end of its own heading too, as in ``### 2026-10-13 10:25 JST - Tool call -
`agent-1` ``, so it stands apart from the session's own records.

With `--context`, a record shows its matched lines in place of its text:
each run of adjacent lines is a list item that says which lines it holds,
and which of them matched, followed by the lines in a text block. The
numbers stay out of the block, apart from any numbers in the text, such
as those of a file a tool read:

````markdown
- Lines 41-43, matched 42

```text
...
```
````

Markdown and digest output treat transcript fields as untrusted: terminal
control characters and bidi overrides become U+FFFD, metadata is flattened
to one line, titles escape inline Markdown and HTML, and digest prompts sit
in code spans.

## What is read

### Typed prompts

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

### Files

- Only regular files are read: `<root>/<project>/<session>.jsonl`, and
  with `--subagents` also `<root>/<project>/<session>/subagents/*.jsonl`.
- Symlinks are not followed.
- A resumed session repeats earlier entries; each entry (by `uuid` and
  position in the line) is listed once, in the original session.

### Skipped lines

A directory or file that cannot be read, and lines that cannot be decoded,
have no timestamp, or exceed 64 MiB, are skipped and reported on stderr
while the rest of the output is still written. The run still ends with
status 0 unless `--on-skip fail` (or `CCDIG_ON_SKIP=fail`) is given; then
it ends with status 1 after writing the output, so a script can keep its
previous copy instead of an incomplete one.

## Masking

Masking is on unless `--no-mask` is given. Session titles, every string in
a tool's input, and tool results are masked too.

### Built-in rules

These are always on, and replace what they match with `***REDACTED***`:

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
- Values assigned to a credential-like name:
  - The forms are `name=value`, `name: value`, `name => value`,
    `--name=value`, `?name=value`, the same inside escaped JSON
    (`\"password\":\"...\"`), and the Japanese words for password and
    token. A quoted value is masked to its closing quote, so a passphrase
    with spaces goes whole. An unquoted value must be printable ASCII, so
    prose after the separator stays
  - A name is split at `_`, `.`, `-`, and camelCase. A strong word such as
    `token`, `secret`, or `password` counts alone or at the end of a part
    (`PGPASSWORD`), a weak word such as `key` or `auth` only next to
    another part (`api_key`, not `key`)
  - Left alone are names about paths, URLs, or database keys
    (`token_path`, `primary_key`), digits behind a quantity
    (`token_limit: 100000`), and values that are references (`$VAR`,
    `ENV[...]`, `<placeholder>`, `%s`, `\u0000`), code (`self.token`,
    `config[`, `last_token`), or type names (`token: string`)
- In a tool's input, a string under a key that names a credential, as in
  `{"password": "..."}`, by the same test

### More masks

These are off by default. `--mask` adds them to any command,
comma-separated or repeated, as in `--mask email,user`:

| Value | Masks |
| --- | --- |
| `aws-account` | 12-digit AWS account IDs in context: in an ARN, assigned to a name such as `Account`, `AWS_ACCOUNT_ID`, `OwnerId`, or an `AWS` principal, in an ECR, S3, or console sign-in host, and after `--account-id` or `--owners`; 12 digits on their own stay |
| `aws-profile` | AWS profile names in context: after `--profile`, assigned to `AWS_PROFILE`, `AWS_DEFAULT_PROFILE`, or a `profile` key, in a `[profile name]` or `[sso-session name]` header or a credentials file header, and as a `source_profile`; a name in prose stays |
| `email` | Email addresses, but not an SSH remote such as `git@github.com:` |
| `git-user` | The `user.name` and `user.email` of git config, in any case; the command fails when either is not set |
| `ip` | IPv4 addresses other than `127.x.x.x`, `0.0.0.0`, and `255.255.255.255`, and IPv6 addresses of three groups or more |
| `user` | The user name, the last part of `$HOME`, in any case and with any of `.`, `_`, and `-` between its parts, so also in a project directory name such as `-Users-alice-b-blog` |

These masks and `--mask-values` also apply to the fields that are not
text: `project`, `cwd`, `git_branch`, file paths, `tool`, `model`,
`subagent`, `command`, the keys of a tool's input, and the warnings on
stderr. The keys of a tool's input get the built-in rules too, so a
secret used as a key, such as a token, is masked there.

- A key that masking makes equal to an earlier one, such as two masked
  file paths, gets `#2`, `#3`, and so on, so both values stay.
- The IPv4 rule cannot tell a bare version number such as `1.2.3.4` from
  an address, so it masks that too; one after a letter or a dot, as in
  `v1.2.3.4`, stays.
- A part of the user name alone, such as a first name, stays; add it to
  `--mask-values`.

### Labeled values

`--mask-values` masks the values in a JSON object of labels to lists of
values, wherever they appear, as typed (case matters):

```json
{"repo": ["acme-billing", "acme-portal"],
 "person": [["Taro Yamada", "yamada"], "Hanako Sato"],
 "": ["secret-word"]}
```

- A value under a label becomes the label and a number in full-width
  brackets, numbered per label in the order given: `acme-billing` becomes
  `［repo1］` and `acme-portal` `［repo2］`, so the records of two
  repositories stay apart while their names are hidden.
- A list in place of a value gives each spelling in it the same number, so
  `Taro Yamada` and `yamada` both become `［person1］`.
- A value under `""` becomes `***REDACTED***`.
- With a leading `@`, the object is read from a file, as in
  `--mask-values @secrets.json`.
- The flag is repeatable: the numbers go on across the objects, and a
  value given twice keeps its first label.
- A malformed object stops the command.

### Report

`--mask-report text` writes on stderr, after the output, how many times
each rule masked something in the records printed, most first:

```text
ccdig: masked 3 by github-token, 2 by values/repo, 1 by email
```

`--mask-report json` writes the same as one object:

```json
{"masked":{"github-token":3,"values/repo":2,"email":1}}
```

- Records left out by a filter or `--limit` are not counted, and the
  values themselves are never written.
- Each record counts in all its fields, as `--format jsonl` prints them,
  so Markdown and `digest`, which leave fields out or cut the text, can
  show fewer masks than the report counts.

| Rule name | Counts |
| --- | --- |
| `aws-key-id`, `credential-assignment`, and the like | A built-in rule, named after what it finds |
| `credential-key` | A string masked whole for its key in a tool's input |
| `email`, `ip`, and the other values of `--mask` | That mask |
| `values` | The values of `--mask-values` under `""` |
| `values/` and a label | The values of `--mask-values` under that label |

### Limits

- `--no-mask` prints everything as it was written, secrets included, for
  checking the masks on your own machine.
- The patterns need a distinctive prefix or a credential-like name, so
  they miss secrets without one, such as `--password value` with a space
  or a value that looks like a `snake_case` identifier.
- Tool results hold whatever the tool read: file contents, environment
  variables, command output.
- Tool inputs, which `records` prints by default, hold what was written,
  such as the content of a `Write` call.

Treat masking as a safety net and check the output yourself before you
share it.

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

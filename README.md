# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Pull the logs you need out of your Claude Code session transcripts,
formatted and masked: to pass them to people or an LLM, to search past
sessions, or to count them and look back on how you work. Combine filters
for time, project, session, branch, text, record type, tool, outcome, and
skill to narrow the history down to the part that matters, and count the
records and tokens with `stats`. `records`, `sessions`, and `stats` write
JSON Lines to process by default, on a terminal and in a pipe alike; give
`--format md` for Markdown to read, or with `records`, `--format html` for
one page to pass on. Common secrets are masked by default,
and email addresses, IP addresses, your user name, and values you list
can be masked too.

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

## Use cases

### Share a log with people

A session can go out as Markdown in the order it was written, with
secrets masked by default. List the names of customers and repositories
in a file, and they read as labels such as `［repo1］`, while
`--mask-report` writes what was masked, and how many times, on stderr, so
a log can be checked and shown outside the team as it is. A PR or a
weekly report can be given a digest of the prompts on its branch or in
its week as the story of the work, and someone in another time zone the
times in theirs with `--tz`.

`records --format html` writes the same records as one page, with each
tool call folded under its tool, what it was about, and how it ended. The
page runs no script and loads nothing, so it can be attached to a PR as
the story of the work, handed to a customer as a record of what the agent
ran and changed, printed to a PDF for a report, or shared as an example of
a session that went well. See [HTML](#html) for what it shows and what to
check before it goes out.

```sh
# One session, also masking emails, IPs, the user name, and a file's values
ccdig records --session 1a2b --format md --mask email,ip,user \
  --mask-values @secrets.json --mask-report text

# The prompts on the branch feature/login, as a digest to attach to a PR
ccdig digest --branch feature/login

# A week's prompts for a weekly report, 300 characters a prompt, 2000 a session
ccdig digest --since 2026-10-05 --until 2026-10-11 \
  --max-prompt-chars 300 --max-session-chars 2000

# A log for a teammate on the US west coast, in their local time
ccdig records --session 1a2b --format md --tz America/Los_Angeles

# The sessions on a PR's branch, with the tool calls, as one page
ccdig records --branch feature/login --type prompt,reply,call --format html > pr.html

# For a customer: the shell commands the agent ran, and the calls that touched a directory
ccdig records --session 1a2b --tool Bash --format html --mask-report text > commands.html
ccdig records --session 1a2b --touched src/billing --type call --format html > billing.html

# Each tool's input and output cut to 2000 characters
ccdig records --session 1a2b --type prompt,reply,call --tool-chars 2000 --format html > session.html
```

### Let Claude Code search past sessions

Run ccdig from inside Claude Code, and it finds what was decided or tried
in the other sessions; `current` leaves out the session that is running,
and leaving out the keys every session repeats and cutting the texts
keeps the tokens of reading the results down.
Find the sessions that matched most with `sessions` first, then read
around each match with `records --around` to follow the prompts and
replies of the time as they went.

```sh
# The other sessions' prompts, replies, and tool calls, 20 at most
ccdig records --session '!current' --type prompt,reply,call --igrep deploy \
  --format md --limit 20

# The same search, without repeated keys, texts and tool I/O cut short
ccdig records --session '!current' --type prompt,reply,call --igrep deploy \
  --message-chars 500 --tool-chars 200 \
  --fields '!agent,!session_title,!project,!cwd,!git_branch'

# The sessions that mention "deploy" most, with the line each first matched
ccdig sessions --igrep deploy --sort matches --fields '!usage' --limit 10

# In the session found, the records that matched and 2 before and after
ccdig records --session b799 --igrep deploy --around 2 --format md

# Of a long reply, only the lines that matched and 1 line around each
ccdig records --type reply --igrep deploy --context 1 --format md
```

### Find the sessions that changed a file

Search the edits and writes by path to find the sessions that changed a
file and when, and check later which request a change came from.

```sh
# The calls that changed src/main.rs, with the session, time, and tool
ccdig records --touched src/main.rs --fields session_id,timestamp,tool
```

### Review the calls that went wrong

Count the calls a permission rule stopped and those you turned down, per
tool, as material for tuning the allow and deny lists of
`settings.json`. The failed calls can be read with the sub-agents', so a
command that keeps failing stands out, and narrowed to a skill, they show
where in its steps the skill fails.

```sh
# The calls a permission rule stopped, per tool
ccdig stats --denial permission-rule --by tool --since 2026-10-05 --until 2026-10-11

# The calls you turned down
ccdig records --denial user-rejected --format md

# Failed Bash calls, with the sub-agents'
ccdig records --type result --tool Bash --status error --subagents --format md

# The calls that failed while the skill weekly-report ran
ccdig records --type result --skill weekly-report --status error --format md
```

### Review how you ask

A session where plans were turned down or asked for again and again is a
sign that the first prompt left conditions out. The comment typed when
turning one down is kept as a prompt whose `kind` is `rejection`, so the
conditions added later can be read as they were typed.

```sh
# The sessions that asked to approve a plan twice or more
ccdig stats --by session --type call --tool ExitPlanMode --min-count 2

# The comments typed when turning a plan or a tool call down
ccdig records --kind rejection --fields session_title,text
```

### Count usage

Count the records, sessions, and tokens per project, month, or skill, to
see how much AI each project used or which skills are used most.

```sh
# Records and tokens per project and month
ccdig stats --by project,month --since 2026-10-01 --until 2026-10-31

# One project's usage (blog), month by month
ccdig stats --project blog --by month

# Output tokens per skill, most first
ccdig stats --by skill --type reply,call --since 2026-10-01 --sort output
```

## Commands

| Command | Lists | Own options |
| --- | --- | --- |
| `records` | One record per prompt, reply, tool call, tool result, summary, or thinking block, of the kinds `--type` picks, in the order they were written | `--type`, `--tool`, `--touched`, `--status`, `--denial`, `--skill`, `--kind`, `--superseded`, `--subagents`, `--around`, `--context`, `--format`, `--fields`, `--message-chars`, `--tool-chars` |
| `sessions` | One record per session with a selected prompt | `--type`, `--subagents`, `--format`, `--fields`, `--sort` |
| `stats` | One record per group of the records `records` would list, by the keys `--by` names | `--by`, `--min-count`, `--type`, `--tool`, `--touched`, `--status`, `--denial`, `--skill`, `--kind`, `--superseded`, `--subagents`, `--format`, `--fields`, `--sort` |
| `digest` | A size-capped Markdown digest of the prompts | `--kind`, `--superseded`, `--max-prompt-chars`, `--max-session-chars` |
| `version` | The installed version | `--format` |

Every command also takes the options under Shared options.

### records

`--type` picks the kinds of record, comma-separated or repeated, and
defaults to `prompt,reply`:

| Type | Record |
| --- | --- |
| `prompt` | A prompt a person typed, or what they answered through a dialog |
| `reply` | A text block of the assistant |
| `call` | A tool call, with what its result says |
| `result` | A tool result, with the call's tool and input |
| `summary` | The summary Claude Code writes when it compacts the context |
| `thinking` | A thinking block of the assistant that kept its text |

`thinking` lists the assistant's thinking blocks, masked as the other
kinds are. Claude Code keeps only the signature of many of them, with no
text; those are never listed, and `stats` counts them in `empty`. A
pattern that does not start with `!` never matches one, as it has no
text.

Eight options apply to some kinds only:

| Flag | Applies to | Keeps |
| --- | --- | --- |
| `--tool` | `call`, `result` | Calls to this tool and their results |
| `--touched` | `call`, `result` | Edits and writes of this file, or of a file under this directory, and Bash commands that name it |
| `--status` | `result` | Results with this status: `error` or `ok` |
| `--denial` | `result` | Results of calls that did not run for this reason, such as `user-rejected` |
| `--skill` | `reply`, `thinking`, `call`, `result` | Records written while the skill ran |
| `--kind` | `prompt` | Prompts typed this way: `text`, `slash`, `rejection`, `answer`, `typed-answer`, or `approval` |
| `--superseded` | `prompt` | With `true`, the prompts the user sent another in place of, and with `false`, the others |
| `--subagents` | `reply`, `thinking`, `call`, `result` | Sub-agent transcripts too |

- `--tool`, `--touched`, `--denial`, `--skill`, and `--kind` are
  repeatable and take a leading `!`, as the shared filters do. `--status`
  takes a comma-separated list like `--type`, and `--superseded` takes
  `true` or `false`.
- `--tool`, `--touched`, `--status`, `--denial`, `--skill`, `--kind`, and
  `--superseded` narrow only the kinds that have what
  they test and let the other kinds through, so
  `ccdig records --type prompt,reply,call --skill tdd` keeps every prompt
  along with the replies and calls written while `tdd` ran.
- Without `--type`, `records` lists only the kinds that every one of
  `--tool`, `--status`, `--denial`, and `--skill` given applies to:
  `ccdig records --tool Bash` prints the `Bash` calls and their results.
  `--touched` lists the calls alone, as each result would list its call
  again, and `--kind` and `--superseded` list the prompts.
- Each of the eight needs a `--type` it applies to, or the command fails.
  `--kind` takes only the kinds above, so `--kind call` fails with a
  pointer to `--type`.
- A search with no `--type` and none of the flags above but
  `--subagents` reads prompts and replies only: give
  `--type prompt,reply,call` to read the tool calls too. A call's input
  can be long, so keep the output small with `--limit`, or cut the texts
  with `--message-chars` and `--tool-chars` and leave keys out
  with `--fields`.
- A call carries its result, so `--type call` does not leave out what a
  tool returned, such as the answers to a question; `--type result` lists
  the results on their own. A call whose file has no result for it, as
  when it was interrupted, has a `null` result.
- Sub-agent transcripts are left out unless `--subagents` is given. The
  instruction a sub-agent was given is not a prompt a person typed, so it
  is never listed; its replies, calls, and results are, of the types
  `--type` picks. `--subagents` alone lists their replies: give
  `--type prompt,reply,call` to read their calls too.
- `--touched` reads the `file_path` of `Edit`, `MultiEdit`, and `Write`,
  the `notebook_path` of `NotebookEdit`, and each word of a `Bash`
  command, split at spaces, quotes, and characters such as `;`, `>`, `=`,
  `,`, and `:`. A value matches a path whose parts, between the slashes or
  backslashes, hold its parts in a row, so `src/cli` matches
  `/repo/src/cli/args.rs` but not `/repo/src/client.rs`, and a value that
  starts with `/` matches from the root. A value with a leading `!` leaves
  a path out of the calls that change one, so a read is never kept, and a
  value with no part but `/` and `.` is an error. A command is matched as written, so a relative path in it
  matches only a relative value, and a command that only reads a file,
  such as `cat`, matches too: leave such commands out with
  `--tool '!Bash'`. The paths are matched before masking, as
  `--project` is.
- `--superseded` is known only once a transcript is read to the end, so
  it cannot be used with `--around` above 0.
- `--around N` also lists the N records before and after each record
  `--grep` or `--igrep` matched, such as the prompt a matched reply
  answered: records of the types `--type` picks that pass every other
  filter, from the same file. They have `matched` false, and in Markdown
  `(context)` after their kind. A record a `!` pattern leaves out is not
  listed as context either. It needs a pattern without a leading `!`.

Four options shape the output:

| Flag | Does |
| --- | --- |
| `--context N` | Lists the lines `--grep` or `--igrep` matched in each record, with N lines before and after each, in `matches`. Markdown and HTML print those lines in place of the whole text. Without it, JSON and JSON Lines list the matched lines alone |
| `--format` | `jsonl` (default) writes one JSON object per line, `json` one document, `md` Markdown to read, and `html` one page to pass on, which only `records` writes |
| `--message-chars N` | Keeps the first N characters of the text of each prompt, reply, and summary, followed by `… (M more characters, next: N+1-2N)`. `--message-chars FROM-TO` keeps the characters from FROM to TO, counted from 1 as `cut -c` counts, after `(K characters before) …`, so the range a cut names reads on where it left off |
| `--tool-chars N` | Does the same to each string in a tool's input, and to each tool result's output, the one a call carries too, so a `Write` of a whole file does not fill the output. It also takes `FROM-TO`, which leaves whole a string of an input no longer than TO - FROM + 1 characters, one the cut of that many left whole, such as a file's path |

### sessions

One record per session that has a selected prompt: the filters apply to
the prompts. `--format` takes `jsonl` (default), `json`, or `md`.

- `--grep` and `--igrep` pick the sessions whose prompts or replies they
  match, and `match_count` says how many of them matched in each, so the
  session that mentions a word most can be read first. `--type` picks
  other types of record to search, as `records --type` does, such as
  `--type prompt,reply,call,result`, and `--subagents` adds the matches in
  the replies, calls, and results of each session's sub-agent
  transcripts to the session; the instruction a sub-agent was given is
  not a prompt, so it is not searched. Both need a pattern that does not
  start with `!`. The other keys
  come from every prompt the other filters keep, as without `--grep`,
  less those a pattern with a leading `!` leaves out, and Markdown adds a
  `Matches` column. Without a pattern that does not start
  with `!`, `match_count` is `null`.
- `prompt_count` and `started_at` count the selected prompts and give the
  first one's time. `ended_at` is the time of the session's last prompt,
  reply, tool call, or tool result in the range, so it shows when the
  work ended even when a session has one prompt. A comment the user typed
  when they turned a tool call down counts as a prompt, and so do the
  answers to a question and a plan approved or turned down.
- `first_match` holds the first record a search matched in the
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
- `resume` is the command that resumes the session in the directory its
  transcript starts in, whatever the range and the filters keep, such as
  `cd /Users/alice/blog && claude --resume <session id>`, with each part
  quoted for a POSIX shell when it needs to be. Claude Code keeps a
  session under that directory, so the command runs there. When there is
  no such command, `resume` is `null` and `resume_unavailable` says why:
  `no-cwd` when the transcript has no `cwd`, `cwd-masked` when a mask
  would change it, and `cwd-missing` when it is not a directory now, as
  after its worktree was removed. Markdown leaves both out.

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
| `resume`, `resume_unavailable` | As above |

### stats

Groups the records `records` would list by the keys `--by` names, and
writes one record per group. `--type`, `--tool`, `--touched`,
`--status`, `--denial`, `--skill`, `--kind`, `--superseded`,
`--subagents`, and the filters pick the records as they do for
`records`, so `--type` defaults to `prompt,reply` here too. `--format`
takes `jsonl` (default), `json`, or `md`, a table.

```sh
# Tool calls per tool in a week, with how long their input is
ccdig stats --by tool --type call --since 2026-11-16 --until 2026-11-22

# Output tokens per skill, most first
ccdig stats --by skill --type reply,call --since 2026-11-01 --sort output

# Plans the user turned down, per week
ccdig stats --by week --type result --tool ExitPlanMode --status error
```

| Flag | Does |
| --- | --- |
| `--by KEY` | Groups by these keys, comma-separated or repeated, in that order: `session`, `project`, `branch`, `type`, `tool`, `skill`, `subagent`, `model`, `kind`, `denial`, `status`, `day`, `week`, or `month`. Without it, all the records are one group, and nothing is written when there are none |
| `--min-count N` | Leaves out the groups with fewer than N records, counting the thinking blocks without text with `--type thinking` |
| `--sort SORT` | `count` (default), `chars`, `output`, or `cache_read`, the most first, or `key`, by the values of the keys. Groups that tie are in the order of their keys |
| `--limit N` | Keeps the first N groups after sorting |

- `day` is written as `2026-11-25`, `week` as the ISO 8601 week from
  Monday, `2026-W48`, and `month` as `2026-11`, all in the zone `--tz`
  names.
- `kind` is how a prompt was typed, as in the `kind` key, and `model`
  that of a reply, a thinking block, or a tool call. `denial` and `status` are those of a
  tool result, in the words `--denial` and `--status` take, so
  `--type result --by denial,tool` counts the calls that did not run per
  reason and tool. They need `--type result`, or a flag such as
  `--denial` that picks the results. A record without the key, such as a
  prompt with `--by tool`, or with it empty, such as a reply outside a
  skill with `--by skill`, is in a group whose value is `null`, after the
  others with `--sort key`.
- The keys hold the values after masking, so with `--mask-values` the
  projects are grouped by their labels.
- `--around`, `--context`, `--message-chars`, and `--tool-chars`
  are errors, as they would change what is counted. So is `--by subagent`
  without `--subagents`, which leaves out every sub-agent's record.

A record has these keys, in this order:

| Key | Holds |
| --- | --- |
| The `--by` keys | Their values, in the order `--by` names them, or `null` |
| `session_title`, `project`, `file` | With `--by session`, the session's last title, project, and transcript file, each `null` when empty, so it can be passed on to `records --session` |
| `count` | How many records; with `--type thinking`, the thinking blocks that kept their text alone |
| `empty` | With `--type thinking`, how many thinking blocks kept no text, which `count` and `chars` leave out; not written otherwise |
| `chars` | How many characters of text they have, after masking: a tool call's input as compact JSON, and the text of the other kinds. Tokens depend on the model, so `usage` holds them instead. `--tool-chars` cuts each string in an input, so it measures something else |
| `sessions` | How many sessions the records are in; not written with `--by session` |
| `first`, `last` | The times of the first and the last record, written as `timestamp` is |
| `usage` | The tokens; see Tokens |

### digest

A Markdown digest of the prompts, capped in size. The filters apply to
the prompts, and so do `--kind` and `--superseded`, as for `records`:
`--superseded false` leaves out the prompts the user typed again. A comment the user typed when they turned a tool call down is
listed with `(rejection)` before it, the answers to a question with
`(answer)`, or `(typed answer)` when the user typed one in their own
words, and a plan approved with `(approval)`. A paste and its
`<pasted_content>` tags become `[pasted N chars]`, so the typed words
show, and the answers leave out the preview of each option chosen
(`selected preview:`); `records` keeps both. Under a prompt that follows
a reply, `- after:` shows the first line of that reply, so a short prompt
such as "keep four of them" reads in context; it is cut as the prompt is,
and counts toward `--max-session-chars`.

| Flag | Default | Does |
| --- | --- | --- |
| `--max-prompt-chars N` | 300 | Truncates each prompt to N characters |
| `--max-session-chars N` | 2000 | Lists at most N characters per session |

### version

`ccdig version` prints the installed version. With `--format json` it
prints the version together with `schema` and `filter`, described under
JSON envelope:

```json
{"version":"0.0.6","schema":9,"filter":"strict-v5"}
```

## Shared options

Every command takes these, before or after the command name. The values
of a repeatable flag on both sides add up.

### Filters

| Flag | Keeps |
| --- | --- |
| `--since`, `--until` | Records from the first to the last day, `YYYY-MM-DD`, or minute or second, `YYYY-MM-DDTHH:MM[:SS]`, with a space for the `T` if you like |
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
- Give the whole ID to pick one session, as an ID that starts another
  matches it too. Claude Code names each transcript after its session, so
  `--session` opens only the transcripts whose name starts with a value
  given, and their sub-agents', and one session reads fast. A line is kept
  only when both the name of its transcript and its `sessionId` start with
  a value. With only values that start with `!`, every transcript is read.
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
records that matched and keeps the context after the last of them. With
`stats`, it keeps the first N groups.
`--format` belongs to each command; see Commands.

`records`, `sessions`, and `stats` take `--fields` too: in JSON and JSON
Lines it writes only the keys it names, comma-separated or repeated, in
the order the records write them, and in Markdown and HTML it shows only
the parts and the columns of those keys. A key with a leading `!` is left out instead, as
with the filters, so
`--fields '!agent,!session_title,!project,!cwd,!git_branch'` drops the keys
every record of a session repeats, and `sessions --fields '!usage'` drops
the tokens. A key the command does not write is an error. In Markdown and
HTML, the type in each heading and the session title always show, and a key it does
not show, such as `usage` of `records`, changes nothing.

`--lang ja` writes in Japanese the words Markdown, HTML, and `digest` put
around the records: the headings, the labels, the column names, and the notes of
a cut, such as `… (1900 more characters, next: 101-200)`. The default is
`en`. The records' own text, the tool names, the `input.<key>` names, the
`denial` values, and the type names in the count line, which `--type`
takes, stay as they are. JSON and JSON Lines write the records alone, so
`--lang` with them is an error, while `CCDIG_LANG` is left unused there.
Warnings, errors, the `--mask-report` line, and `--help` stay in English.

### Environment variables

| Variable | Sets the default of | Value |
| --- | --- | --- |
| `CCDIG_ROOT` | `--root` | A directory. It comes before `CLAUDE_CONFIG_DIR` |
| `CLAUDE_CONFIG_DIR` | `--root` | Claude Code's own variable; its `projects` directory is the root |
| `CLAUDE_CODE_SESSION_ID` | `--session current` | Claude Code's own variable; the ID of the session it runs ccdig in |
| `CCDIG_ON_SKIP` | `--on-skip` | `warn` or `fail` |
| `CCDIG_LANG` | `--lang` | `en` or `ja`. Formats that take no `--lang` leave it unused, though another value is an error there too |
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
 "session_title_then":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T10:00:01+09:00","turn":1,
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "permission_mode":"","superseded":false,"redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"},
 "matched":true,"matches":[]}
```

Every record starts with `type`, the kind `--type` names. The other kinds
have the same keys from `type` to `turn`, and end with `redacted`,
`source`, `matched`, and `matches`:

| Type | Keys in between |
| --- | --- |
| `reply`, `thinking` | `text`, `model`, `skill`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id`, `usage` |
| `call` | `tool`, `input`, `tool_use_id`, `result`, `skill`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id`, `usage` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `denial`, `text`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id` |
| `summary` | `text` |

Every key is always present unless `--fields` leaves it out. The `kind` of a prompt is how it was typed,
`slash` or `text`, or `rejection` for what the user typed when they turned
a tool call down, `answer` for the answers to a question, `typed-answer`
when the user typed one of them in their own words, and `approval` for a
plan approved; the record type is `type`, which `--type` picks.

- `result` of a call is what its result in the same file says, masked:
  `is_error`, `denial`, and `text`, as a result has them, or `null` when
  the file has no result for it.
- The result of an `ExitPlanMode` call leaves out the plan when it is the
  one in the call's input, with `(the plan in the call's input)` under
  `## Approved Plan:` in its place, in `result` and in the `text` of the
  result alike. A plan the user edited is not the one in the input, so it
  stays.
- `turn` counts the prompts of kind `text` and `slash` in the transcript
  file up to the record, in the order written, a prompt sent again in
  place of another included, so a record has the number of the prompt it
  follows, as a reply to the second prompt has `turn` 2. An answer to a
  dialog and a comment on a call turned down have the number of the
  prompt before them. It is 0 before the first prompt and in a
  sub-agent's transcript.
- A reply that is only `No response requested.`, which Claude Code writes
  with the model `<synthetic>` when a turn wants no reply, is left out.
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
- `session_title_then` is the title the session had when the line was
  written, empty before Claude Code wrote the first. Claude Code writes a
  title after the prompt it comes from, so the prompt that changed the
  title carries the one before. `session_title` is the last title, on
  every record of the session.
- A prompt of kind `rejection` is the comment the user typed when they
  turned a tool call down, such as a plan they sent back with
  `ExitPlanMode`. It is on the line of the call's result, so its
  `source.uuid` is the result's, and the result's `tool` names the tool.
  Claude Code's own text when the user picks "clarify" in a question
  dialog is not a comment. A plan turned down without a comment is a
  `rejection` too, with the first sentence of Claude Code's text.
- A prompt of kind `answer` is what the user picked in an
  `AskUserQuestion` dialog, and one of kind `typed-answer` what they
  answered when they typed at least one answer in their own words, both
  without Claude Code's words around them. Claude Code writes the two
  apart from version 2.1.220; before it, every answer is an `answer`. A
  prompt of kind `approval` is a plan approved with `ExitPlanMode`, with
  the first sentence of Claude Code's text. Like a `rejection`, each is on the line
  of the call's result.
- `superseded` is `true` for a prompt the user sent another prompt in
  place of, later in the file: one that follows the same line, as when
  they press Esc and edit the prompt, or interrupt the reply and send it
  again. A `rejection` is never superseded.
- `denial` is Claude Code's name for why a call did not run, such as
  `user-rejected`, `permission-rule`, `automode-blocked`, or `cancelled`,
  and empty when it ran. Before Claude Code recorded it, only
  `user-rejected` is read, from the result's text. `is_error` is `true` for
  each of them too.
- A prompt keeps pasted text where Claude Code put it, between
  `<pasted_content id="...">` tags; only `digest` folds it. To read the
  typed words alone, drop the tags with jq:

  ```sh
  ccdig records --type prompt | jq '.text |= gsub(
    "<pasted_content id=\"[0-9a-f]+\">(.|\n)*?</pasted_content id=\"[0-9a-f]+\">"; "")'
  ```

- `permission_mode` is the mode Claude Code recorded on the prompt's line,
  such as `default`, `plan`, or `auto`. It is empty when the line does not
  say: Claude Code writes none for a slash command or in older sessions.
- `subagent` is empty for the session itself and the file name, such as
  `agent-1a2b`, for a sub-agent. A sub-agent's record carries the
  `session_id` and `session_title` of its session; the title is empty when
  the session's own transcript was last written before `--since`.
- `subagent_type`, `subagent_description`, and `parent_tool_use_id` say
  how a sub-agent was started: its kind, such as `Explore`, what the
  caller wrote it was for, masked as text is, and the `tool_use_id` of the
  call that started it. That call is in the session's transcript, or for a
  sub-agent another one started, in that sub-agent's, so the record with
  that `tool_use_id` outside the sub-agent's own file is its parent. They
  come from the `.meta.json` next to the sub-agent's transcript, as
  Claude Code has been observed to write it, and are empty for the session
  itself, when that file is missing, and when it does not say, as for a
  skill run in a forked sub-agent, which no call starts.
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
  only the first reply, thinking block, or call that the filters keep of a
  response in a file carries them. The others have `null`, as do lines without tokens.
- The records do not always add up to the whole. A response with no reply
  or call, such as one of thinking alone, has no record to carry its
  tokens unless `--type thinking` lists a block of it, and neither has
  one whose records `--type` or a filter all leave out.
- A `sessions` record ends with the same four keys in `usage`: the sum
  over the session's responses in the range, each once. For it,
  `sessions` also reads the assistant lines.
- A `stats` record adds up in `usage` the tokens its records carry, so
  each response counts once, and only when a record of it is kept; the
  sum is not that of `sessions`. With `--by tool`, `type`, `kind`,
  `denial`, or `status`, the
  records of one response are in different groups, so `usage` is `null`,
  and `--sort output` and `--sort cache_read` are errors. It is `null`
  too when no record carries tokens, as with `--type prompt`.

### JSON envelope

`--format json` wraps the records with the versions and the flags of the
run:

```json
{"schema":9,"filter":"strict-v5","ccdig":"0.0.6",
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

`records --format md` starts with how many records it lists, by type, and
how many `--around` added as context, as in
`- Records: 7 (prompt 4, call 3) + 2 context`, so an empty list reads
`- Records: 0`. `sessions` and `stats` start with how many sessions or
groups they list, and a search of `sessions` with how many matches, by
type. The sessions table has a `Session` and a `Cwd` column, which
`records --session` and `--project` take.

`records --format md` puts the records of each transcript file under one
heading with the project and session, and each record under a heading
with its time to the second, its turn when it follows a prompt, and its
kind. A string in a call's input that runs over lines, such as the plan
of `ExitPlanMode`, follows the rest of the input in a text block named
for its key, such as `input.plan`, so it reads as written. A call shows
its result under its input, and the
result is not shown again on its own unless `--context` lists its matched
lines, or a search matched it and not its call, which then shows as
context. `--limit` counts a result shown under its call, as JSON lists
it, so Markdown and HTML can show fewer records than the limit. A tool's output that is a JSON object or array is indented, with
its escapes such as `\u65e5` decoded; JSON and JSON Lines keep it as the
tool wrote it. A sub-agent's record names the sub-agent at the
end of its own heading too, as in ``### 2026-10-13 10:25:01 JST - Tool call -
`agent-1` ``, so it stands apart from the session's own records. The
heading of a sub-agent's transcript also says its type and, on one line,
its description, as in ``- Subagent type: `Explore` ``.

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

### HTML

`records --format html` writes one page on stdout, which a redirect saves.
It starts with the count line of Markdown and how many times each rule
masked something, or `Not masked (--no-mask)`. Each transcript file gets a
section with the project, the session, the branch, and when the earliest
and the latest record shown were written: a filter can leave out records
at either end, so this is not when the session started and ended. Each
record is under its time, its turn, and its kind, with an `id` made of its
type, its line's ID, and its block in the line, such as
`reply-3f2a9c1e-…-0`, so a link to it stays the same whatever the filters
keep. A record whose line has no ID gets its place on the page, such as
`r7`.

- A call folds under one line: its tool, the first line of the first of
  `command`, `file_path`, `pattern`, `url`, `query`, `description`,
  `skill`, and `path` in its input, cut to 80 characters, and a badge
  that says `ok`, `error`, `denied`, or `no result`. Its result folds
  inside it. A result that stands on its own, as in Markdown, links to
  its call, and the call back to it.
- A sub-agent's records show in the call that started it when the page
  has that call, as with `--type prompt,reply,call --subagents`, and in a
  section of their own after the session's otherwise, as in Markdown.
- A summary marks where the context was compacted, with its text folded.
- Replies, thinking, and summaries are Markdown and are rendered, with
  tables and struck-through text. Prompts, inputs, and outputs show as
  written. A record `--around` added is faded, and the lines `--context`
  lists have the matched ones marked.
- A long output is not cut but scrolls in its box; `--tool-chars` cuts it.
- The page follows the system's dark mode. Printing it, or saving it as a
  PDF, opens every fold, in Chrome and Edge 131, Firefox 143, Safari
  18.4, and later; in an older browser, open the folds first.

The page treats transcript fields as untrusted, as Markdown does, and runs
no script and loads nothing: its `Content-Security-Policy` allows only the
styles written in it. Text is escaped, raw HTML in a reply shows as text,
a link goes only to an `http` or `https` address, and an image shows only
its description, so opening the page sends no request.

Before a page goes outside the team:

1. Make it with the masks its content needs, such as
   `--mask email,ip,user --mask-values @secrets.json`, and with
   `--mask-report text`.
2. Check that the count on stderr is the one at the top of the page.
3. Open the page, open the folds or the print preview, and read it through
   for names, paths, and values no rule knows. Masking finds the patterns
   it knows, not what a customer would rather keep to themselves.

## What is read

### Typed prompts

A `user` entry is dropped, with the first matching reason, when it:

1. is marked `isMeta`, `isSidechain`, or `isCompactSummary`;
2. has an `origin.kind` other than `human`;
3. carries a `tool_result` block, unless the user turned the call down
   with a comment: the comment is kept, as a prompt of kind `rejection`;
   or the block answers an `AskUserQuestion` or `ExitPlanMode` call: the
   answer is kept, as a prompt of kind `answer`, `typed-answer`,
   `approval`, or `rejection`;
4. starts with a harness tag such as `<system-reminder>`,
   `<task-notification>`, `<local-command-…>`, `<bash-input>`,
   `<bash-stdout>`, or `<user-prompt-submit-hook>`;
5. is empty, or is the `[Request interrupted by user` marker.

Rule 4 applies even when `origin.kind` is `human`, because Claude Code
records a `!` shell escape as human input. Sessions recorded before Claude
Code wrote `origin` skip rule 2. Thinking blocks are printed only with
`--type thinking`, and only those that kept their text.

### Files

- Only regular files are read: `<root>/<project>/<session>.jsonl`, and
  with `--subagents` also `<root>/<project>/<session>/subagents/*.jsonl`.
  `records` also reads the `.meta.json` of the same name next to each
  sub-agent transcript it reads, up to 1 MiB. A missing one, or one that
  is not a regular file, says nothing; one that cannot be read or decoded
  is a warning, which `--on-skip fail` counts.
- Symlinks are not followed.
- A resumed session repeats earlier entries; each entry (by `uuid` and
  position in the line) is listed once, in the original session, when the
  run reads both transcripts. Claude Code writes the new session's ID on
  the copies, so `--session` with the new ID lists them too.

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
| `aws-account` | 12-digit AWS account IDs in context: in an ARN, assigned to a name such as `Account`, `AWS_ACCOUNT_ID`, `OwnerId`, or an `AWS` principal, in an ECR, S3, or console sign-in host, after `--account-id` or `--owners`, in brackets in a `PS1` or `PROMPT`, and after the Japanese word for account, directly or as a comma-separated list, or in brackets on its line or on the line after it when it is a heading, with the profile or account name beside an ID in brackets; 12 digits on their own stay |
| `aws-profile` | AWS profile names in context: after `--profile`, assigned to `AWS_PROFILE`, `AWS_DEFAULT_PROFILE`, or a `profile` key, in a `[profile name]` or `[sso-session name]` header or a credentials file header, and as a `source_profile`; a name in prose stays |
| `email` | Email addresses, but not an SSH remote such as `git@github.com:` |
| `git-user` | The `user.name` and `user.email` of git config, in any case; the command fails when either is not set |
| `ip` | IPv4 addresses other than `127.x.x.x`, `0.0.0.0`, and `255.255.255.255`, and IPv6 addresses of three groups or more |
| `user` | The user name, the last part of `$HOME`, in any case and with any of `.`, `_`, and `-` between its parts, so also in a project directory name such as `-Users-alice-b-blog` |

These masks and `--mask-values` also apply to the fields that are not
text: `project`, `cwd`, `git_branch`, file paths, `tool`, `model`,
`skill`, `subagent`, `subagent_type`, `command`, the keys of a tool's input, and the warnings on
stderr. The keys of a tool's input get the built-in rules too, so a
secret used as a key, such as a token, is masked there.

- A key that masking makes equal to an earlier one, such as two masked
  file paths, gets `#2`, `#3`, and so on, so both values stay.
- The IPv4 rule cannot tell a bare version number such as `1.2.3.4` from
  an address, so it masks that too; one after a letter or a dot, as in
  `v1.2.3.4`, stays.
- A part of the user name alone, such as a first name, stays; add it to
  `--mask-values`.
- An AWS account name on its own, away from an ID, stays; add it to
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
- Each record counts in all its fields, those a format leaves out too.
  Markdown and HTML indent a tool's output that is JSON and mask the
  strings decoding brings out, so they can count more than
  `--format jsonl` for the same records; the top of an HTML page says the
  same count as the report. Markdown, HTML, and `digest` leave fields out
  or cut the text, so they can show fewer masks than they count.

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
- Tool inputs, which `records` prints in calls and results, hold what was written,
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

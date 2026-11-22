# Changelog

All notable changes to this project will be documented in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- A Japanese name for a credential, such as the word for password, now has
  its value masked when it follows a value or an ASCII word directly, as in
  `token=...` or `ID：user1` followed by the name and a colon. Its value was
  left as it was.

## [0.0.1] - 2026-11-21

### Added

- Each release has a prebuilt binary for macOS on Apple silicon,
  `ccdig-aarch64-apple-darwin.tar.gz`, with its SHA-256 checksum.
- `--mask email`, `--mask ip`, and `--mask user` mask email addresses, IP
  addresses, and the user name from `$HOME`; `--mask aws-account` and
  `--mask aws-profile` mask AWS account IDs and profile names in context,
  and `--mask git-user` the `user.name` and `user.email` of git config;
  `--mask-values` masks the values in a JSON object of labels to lists of
  values, each as its label and a number, such as `［repo1］`, so that
  records stay apart by repository or person while the names are hidden.
  They are off by default and
  cover the fields that are not text, such as `project`, `git_branch`,
  `tool`, and the keys of a tool's input.
- `--mask-report text|json` writes on stderr how many times each masking
  rule masked something in the records printed, without the values, so
  the masking can be checked before the output is shared.
- Tool calls and results carry the untruncated tool input that Claude
  Code stores in `wireToolInputs`, instead of the shortened copy in the
  `tool_use` block.
- Masking of `Authorization` header credentials, `Bearer` tokens, `mysql`
  `-p` passwords, SendGrid keys, Google OAuth tokens and client secrets,
  Stripe webhook secrets, values assigned to credential-like names such as
  `password=`, `--api-token=`, `PGPASSWORD=`, or `client_secret:` (also
  inside escaped JSON), and strings under such keys in a tool's input.
- `ccdig records`: the prompts a person typed, the assistant's text
  blocks, tool calls with their input as JSON, and tool results with the
  tool and input of the call they answer, as JSON Lines, JSON, or
  Markdown. `--type prompt,reply,call,result` picks the kinds, by default
  `prompt,reply,call`, and they are listed in the order they were
  written, each with a `type` key. Harness traffic is dropped by the
  `strict-v2` rules, and slash command expansions are folded back into
  `/command args`. `--type summary` lists the summaries Claude Code
  writes when it compacts the context.
- A prompt carries the `permission_mode` Claude Code recorded on its line,
  such as `plan` or `auto`, or an empty one when the line has none.
- Replies and tool calls carry the `usage` tokens of their response, on
  the first record of each response, and `sessions` records their sum.
- `ccdig sessions`: sessions with at least one selected prompt.
- `ccdig digest`: a size-capped Markdown digest of the prompts for LLM
  input.
- `ccdig records --format md --max-input-chars N` cuts each string in a
  tool call's input to N characters; JSON and JSON Lines are not cut.
- `ccdig version`, and the help when no command is given.
  `ccdig version --format json` prints the version with the `schema` and
  `filter` of the output, so a script can check them before it reads it.
- `--format json` wraps the records with `schema` 2, the `filter`, the
  `ccdig` version, and a `run` object holding `--since`, `--until`, `--tz`,
  the `--mask` names, the `--mask-values` labels, and `--no-mask`.
- Filters for every command: `--since` and `--until` in the system time
  zone or one given with `--tz`, `--project`, `--grep` or `--igrep`
  (ignoring case; both matched after masking), `--session` (ID prefix),
  `--branch`, and `--limit`. For `records`: `--skill` and `--subagents`
  for replies, calls, and results, `--tool` for calls and results, and
  `--status error|ok` for results. These narrow only the kinds they apply
  to and need one of them in `--type`; without `--type`, `records` lists
  only those kinds. `--grep`, `--igrep`, and the flags that take a name
  are repeatable, and a value with a leading `!` leaves out what matches
  it.
- `CCDIG_ROOT`, `CCDIG_MASK`, and `CCDIG_MASK_VALUES` set the defaults of
  `--root`, `--mask`, and `--mask-values`.
- Without `--root` or `CCDIG_ROOT`, the root is
  `$CLAUDE_CONFIG_DIR/projects` when Claude Code's `CLAUDE_CONFIG_DIR` is
  set, so a moved config directory is read instead of a stale
  `~/.claude`.
- Help lists the flags under Records, Filters, Input, Masking, and
  Output, with the values `--type`, `--status`, `--format`, and `--mask`
  take.
- Masking on by default (`--no-mask` to turn it off) of AWS credentials,
  GitHub, GitLab, npm, Anthropic, OpenAI, Google, Stripe, and Slack tokens,
  Slack webhooks, JWTs, URL passwords, and PEM / PGP private keys, in
  prompts, replies, tool inputs, tool results, and session titles.
- Entries copied into resumed sessions are listed once, in the original.
- Unreadable files and undecodable, timestamp-less, or over-64-MiB lines
  are reported on stderr while the rest of the output is still written.
  `--on-skip fail` (or `CCDIG_ON_SKIP=fail`) then ends the run with status
  1, after the output.
  Symlinks and other non-regular files under `--root` are not read.
- Markdown and digest output neutralize terminal control characters, bidi
  overrides, and Markdown or HTML in transcript fields.

### Fixed

- With `--subagents`, a sub-agent's records carry the title of their
  session instead of an empty `session_title`, unless the session's own
  transcript was last written before `--since`.
- Slack workflow and trigger webhook URLs longer than 56 characters are
  masked to the end.

[Unreleased]: https://github.com/tsukuboshi/ccdig/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/tsukuboshi/ccdig/releases/tag/v0.0.1

# Changelog

All notable changes to this project will be documented in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `ccdig conversation`: the prompts, replies, and tool calls of sessions
  in the order they were written, each with a `type` key, as JSON Lines,
  JSON, or Markdown.
- `--mask email`, `--mask home`, and `--mask ip` mask email addresses,
  shorten the home directory to `~` (also in `cwd`, `project`, and file
  paths), and mask IP addresses; `--mask-file` masks each line of a file.
  They are off by default and also apply with `--no-redact`.
- `ccdig tools` and `ccdig results` print the untruncated tool input that
  Claude Code stores in `wireToolInputs`, instead of the shortened copy in
  the `tool_use` block.
- Masking of `Authorization` header credentials, `Bearer` tokens, `mysql`
  `-p` passwords, SendGrid keys, Google OAuth tokens and client secrets,
  Stripe webhook secrets, values assigned to credential-like names such as
  `password=`, `--api-token=`, `PGPASSWORD=`, or `client_secret:` (also
  inside escaped JSON), and strings under such keys in a tool's input.
- `ccdig prompts`: the prompts a person typed in Claude Code sessions, as
  JSON Lines, JSON, or Markdown. Harness traffic is dropped by the
  `strict-v2` rules, and slash command expansions are folded back into
  `/command args`.
- `ccdig replies`: the assistant's text blocks.
- `ccdig tools`: tool calls with their input as JSON.
- `ccdig results`: tool results, each with the tool and input of the call
  it answers.
- `ccdig sessions`: sessions with at least one selected prompt.
- `ccdig digest`: a size-capped Markdown digest of the prompts for LLM
  input.
- `ccdig version`, and the help when no command is given.
- Filters for every command: `--since` and `--until` in the system time
  zone or one given with `--tz`, `--project`, `--grep` with `-i` (matched
  after masking), `--session` (ID prefix), `--branch`, and `--limit`. For
  replies, tools, and results: `--skill` and `--subagents`. For tools and
  results: `--tool`. For results: `--errors`.
- Masking on by default (`--no-redact` to turn it off) of AWS credentials,
  GitHub, GitLab, npm, Anthropic, OpenAI, Google, Stripe, and Slack tokens,
  Slack webhooks, JWTs, URL passwords, and PEM / PGP private keys, in
  prompts, replies, tool inputs, tool results, and session titles.
- Entries copied into resumed sessions are listed once, in the original.
- Unreadable files and undecodable, timestamp-less, or over-64-MiB lines
  are reported on stderr while the rest of the output is still written.
  Symlinks and other non-regular files under `--root` are not read.
- Markdown and digest output neutralize terminal control characters, bidi
  overrides, and Markdown or HTML in transcript fields.

### Fixed

- With `--subagents`, a sub-agent's records carry the title of their
  session instead of an empty `session_title`, unless the session's own
  transcript was last written before `--since`.
- Slack workflow and trigger webhook URLs longer than 56 characters are
  masked to the end.

[Unreleased]: https://github.com/tsukuboshi/ccdig/commits/main

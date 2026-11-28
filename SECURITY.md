# Security Policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report privately through GitHub:
<https://github.com/tsukuboshi/ccdig/security/advisories/new>

Include what you found, how to reproduce it, and its impact. Use synthetic
data only; do not attach real session transcripts.

You can expect an initial response within 7 days.

## Scope

ccdig reads local transcript files and writes to standard output. Issues of
particular interest:

- A secret that the default masking should catch but does not, in a
  prompt, a reply, a tool's input, or a tool result.
- Crafted transcript input that causes a crash, hang, or excessive memory
  use.
- Output that discloses file contents outside the requested transcript
  root, such as through a symlink.
- Transcript text that reaches the terminal as control sequences, or
  changes the structure of Markdown output.
- Transcript text that changes the structure of the HTML page, runs a
  script in it, or makes it load anything.

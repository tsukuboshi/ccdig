#!/usr/bin/env bash
# Checks that README.ja.md follows README.md, the source.
#
# Translation is not checked; the structure and every literal are. Both
# files are split at their headings, and each pair of sections must have the
# same code blocks, table sizes, list item counts, inline code, and link
# targets, each as often, and the Japanese section must hold every number
# of the English one. Comments in shell blocks may be translated.
#
# Not read: setext headings, reference-style links, HTML links, `1)` items,
# HTML comments, and link targets with parentheses in them.
#
# Usage: .github/scripts/check-readme-sync.sh [README.md README.ja.md]

set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
en=${1:-$root/README.md}
ja=${2:-$root/README.ja.md}

# Prints one "<section>\t<item>" line per item a translation must keep.
# Only POSIX awk is used, so it runs the same on macOS and on the CI runner.
program=$(
	cat <<'AWK'
function emit(item) { printf "%03d\t%s\n", sec, item }

# A link target without its title. In README.ja.md, a link to either README
# is named as README.md names it, so the language link and anchors match.
function target(value) {
  sub(/[ \t].*$/, "", value)
  if (!swap) return value
  if (match(value, /^README\.md(#|$)/)) return "README.ja.md" substr(value, 10)
  if (match(value, /^README\.ja\.md(#|$)/)) return "README.md" substr(value, 13)
  return value
}

# Takes the text of `text` from `opener` to `closer` out, one span at a time,
# and returns what is left with each span replaced by a space.
function spans(text, opener, closer, kind, skip,    out, i, j, rest, value) {
  out = ""
  while ((i = index(text, opener)) > 0) {
    rest = substr(text, i + length(opener) - skip)
    j = index(rest, closer)
    if (j == 0) break
    value = substr(rest, 1, j - 1)
    if (kind == "link") value = target(value)
    emit(kind ": " value)
    out = out substr(text, 1, i - 1) " "
    text = substr(rest, j + length(closer))
  }
  return out text
}

function flush(    t, text) {
  for (t = 1; t <= tables; t++)
    emit(sprintf("table %02d: %d rows, %d columns", t, rows[t], cols[t]))
  emit("list: " bullets " bulleted, " numbered " numbered")
  text = spans(prose, "`", "`", "inline code", 0)
  text = spans(text, "](", ")", "link", 0)
  # Keep "http" of "<http" in the target.
  text = spans(text, "<http", ">", "link", 4)
  while (match(text, /[0-9]+(\.[0-9]+)*/)) {
    emit("number: " substr(text, RSTART, RLENGTH))
    text = substr(text, RSTART + RLENGTH)
  }
  tables = bullets = numbered = blocks = 0
  prose = ""
}

BEGIN { emit("title: (before the first heading)") }

# A fence closes on a line of at least as many of its own characters.
fence != "" {
  text = $0
  sub(/^[ \t]+/, "", text)
  if (index(text, fence) == 1 && text ~ /^(`+|~+)[ \t]*$/) {
    fence = ""
    next
  }
  line++
  shell = lang ~ /^(sh|bash|shell|zsh)$/
  code = $0
  if (shell && text ~ /^#/) code = "#"
  else if (shell) sub(/[ \t]+#.*$/, " #", code)
  emit(sprintf("code %02d line %04d: %s", blocks, line, code))
  next
}
{
  text = $0
  sub(/^[ \t]+/, "", text)
}
match(text, /^(```+|~~~+)/) {
  fence = substr(text, 1, RLENGTH)
  blocks++
  line = 0
  lang = substr(text, RLENGTH + 1)
  sub(/^[ \t]+/, "", lang)
  sub(/[ \t].*$/, "", lang)
  emit(sprintf("code %02d: language %s", blocks, lang == "" ? "(none)" : lang))
  in_table = 0
  next
}

/^#+ / {
  flush()
  sec++
  match($0, /^#+/)
  emit("heading: " RLENGTH)
  emit("title: " $0)
  in_table = 0
  next
}

{
  if (text ~ /^\|/) {
    if (!in_table) { tables++; rows[tables] = 0 }
    rows[tables]++
    cells = text
    sub(/^\|/, "", cells)
    sub(/\|[ \t]*$/, "", cells)
    cols[tables] = gsub(/\|/, "|", cells) + 1
    in_table = 1
  } else {
    in_table = 0
  }
  if (text ~ /^[-*+] /) {
    bullets++
    text = substr(text, 3)
  } else if (match(text, /^[0-9]+\. /)) {
    numbered++
    text = substr(text, RLENGTH + 1)
  }
  prose = prose " " text
}

END { flush() }
AWK
)

outline() {
	LC_ALL=C awk -v swap="$2" "$program" "$1"
}

# The outline in order, with an item that repeats in a section numbered
# from its second time on, so a translation must keep it as often.
skeleton() {
	outline "$1" "$2" |
		LC_ALL=C awk '{ n = ++seen[$0]; print (n > 1 ? $0 " (#" n ")" : $0) }' |
		LC_ALL=C sort
}

# The items of a skeleton with the section titles, which are translated,
# left out.
items() {
	skeleton "$1" "$2" | awk -F '\t' '$2 !~ /^title: /'
}

levels() {
	outline "$1" "$2" | awk -F '\t' '
    $2 ~ /^heading: / { sub(/^heading: /, "", $2); printf "%s ", $2 }'
}

en_name=$(basename "$en")
ja_name=$(basename "$ja")
en_levels=$(levels "$en" 0)
ja_levels=$(levels "$ja" 1)
if [[ $en_levels != "$ja_levels" ]]; then
	echo "$ja_name: heading levels differ: $en_name has [ $en_levels], $ja_name has [ $ja_levels]" >&2
	echo "$en_name is the source; update $ja_name to match it." >&2
	exit 1
fi

# Lines only in the English skeleton, then those only in the Japanese one.
# A number the Japanese text adds, as in "1 つ", is not a drift.
drifts=$(
	{
		LC_ALL=C comm -23 <(items "$en" 0) <(items "$ja" 1) |
			awk -v name="$en_name" '{ print name "\t" $0 }'
		LC_ALL=C comm -13 <(items "$en" 0) <(items "$ja" 1) |
			awk -F '\t' -v name="$ja_name" '$2 !~ /^number: / { print name "\t" $0 }'
	} | LC_ALL=C sort -t "$(printf '\t')" -k2,2 -s
)

if [[ -z $drifts ]]; then
	exit 0
fi
awk -F '\t' -v ja="$ja_name" '
  NR == FNR {
    if ($2 ~ /^title: /) title[$1 + 0] = substr($2, 8)
    next
  }
  {
    item = $0
    sub(/^[^\t]*\t[^\t]*\t/, "", item)
    printf "%s: section %d `%s`: only in %s: %s\n", ja, $2 + 0, title[$2 + 0], $1, item
  }
' <(outline "$en" 0) <(printf '%s\n' "$drifts") >&2
echo "$en_name is the source; update $ja_name to match it." >&2
exit 1

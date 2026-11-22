#!/usr/bin/env bash
# Checks that ARCHITECTURE.md agrees with the code it describes.
#
# - The module table under "## Code map" lists the modules src/lib.rs
#   declares, the test-only ones aside.
# - Each module has a .ast-grep/rules/layer-<module>.yml rule that forbids
#   every module but itself and the ones it may use: those before it in the
#   table, or the ones its row names after "uses only". The rule's message
#   names the ones it may use. A module that may use all the others has no
#   rule, and there is no rule for a module the table does not list.
# - The .rs files the "I/O happens only at the ends." invariant names are
#   the files outside the tests that expect clippy::disallowed_methods.
#
# Usage: .github/scripts/check-architecture.sh

set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
doc=$root/ARCHITECTURE.md
rules=$root/.ast-grep/rules
failed=0

# Reports a drift and goes on, so one run lists them all.
drift() {
	echo "ARCHITECTURE.md: $*" >&2
	failed=1
}

# Prints the words of the lines on stdin sorted, one line, space-separated.
sorted() {
	tr ' ' '\n' | sed '/^$/d' | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//'
}

# Prints "<module>\t<the modules it may use>" for each row of the module
# table, in order; the second field is "-" when the row does not say
# "uses only". Only POSIX awk is used, so it runs the same on macOS and on
# the CI runner.
table() {
	awk '
	  /^## / { inside = ($0 == "## Code map"); next }
	  inside && /^\| `[a-z_]+` \|/ {
	    split($0, cell, "|")
	    name = cell[2]
	    gsub(/[ `]/, "", name)
	    allowed = "-"
	    if ((i = index(cell[3], "uses only")) > 0) {
	      # The names up to the end of the sentence.
	      rest = substr(cell[3], i + 9)
	      sub(/\..*$/, "", rest)
	      allowed = ""
	      while (match(rest, /`[a-z_]+`/)) {
	        allowed = allowed (allowed == "" ? "" : " ") substr(rest, RSTART + 1, RLENGTH - 2)
	        rest = substr(rest, RSTART + RLENGTH)
	      }
	    }
	    printf "%s\t%s\n", name, allowed
	  }
	' "$doc"
}

# Prints the modules src/lib.rs declares, without those under #[cfg(test)].
declared() {
	awk '
	  /^#\[cfg\(test\)\]/ { test = 1; next }
	  /^mod [a-z_]+;/ && !test { name = $2; sub(/;$/, "", name); print name }
	  { test = 0 }
	' "$root/src/lib.rs"
}

# Prints the alternatives of the rule's regex that starts with `prefix`,
# as in `^crate::(a|b)$`.
alternatives() {
	awk -v prefix="$2" '
	  $1 == "regex:" && index($2, prefix) == 1 {
	    value = substr($2, length(prefix) + 1)
	    sub(/\).*$/, "", value)
	    gsub(/\|/, " ", value)
	    print value
	  }
	' "$1"
}

# Prints the modules the rule's message says the module may use.
message_names() {
	awk '
	  $1 == "message:" && match($0, /may use only [^"]*/) {
	    value = substr($0, RSTART + 13, RLENGTH - 13)
	    if (value != "no other module") { gsub(/,/, "", value); print value }
	  }
	' "$1"
}

rows=$(table)
if [[ -z $rows ]]; then
	drift "no module table under \"## Code map\""
	exit 1
fi
modules=$(cut -f 1 <<<"$rows" | tr '\n' ' ')

while read -r name; do
	drift "src/lib.rs declares \`$name\`, which the module table does not list"
done < <(LC_ALL=C comm -23 <(declared | LC_ALL=C sort) <(tr ' ' '\n' <<<"$modules" | sed '/^$/d' | LC_ALL=C sort))
while read -r name; do
	drift "the module table lists \`$name\`, which src/lib.rs does not declare"
done < <(LC_ALL=C comm -13 <(declared | LC_ALL=C sort) <(tr ' ' '\n' <<<"$modules" | sed '/^$/d' | LC_ALL=C sort))

earlier=""
expected_rules=""
while IFS=$'\t' read -r name allowed; do
	if [[ $allowed == - ]]; then
		allowed=$earlier
	fi
	earlier="$earlier $name"
	forbidden=""
	for other in $modules; do
		case " $name $allowed " in
		*" $other "*) ;;
		*) forbidden="$forbidden $other" ;;
		esac
	done
	forbidden=$(sorted <<<"$forbidden")
	allowed=$(sorted <<<"$allowed")
	rule=$rules/layer-$name.yml
	if [[ -z $forbidden ]]; then
		if [[ -e $rule ]]; then
			drift "\`$name\` may use every module, so .ast-grep/rules/layer-$name.yml should not exist"
		fi
		continue
	fi
	expected_rules="$expected_rules layer-$name.yml"
	if [[ ! -e $rule ]]; then
		drift "\`$name\` needs .ast-grep/rules/layer-$name.yml, forbidding: $forbidden"
		continue
	fi
	for prefix in '^crate::(' '^('; do
		found=$(alternatives "$rule" "$prefix" | sorted)
		if [[ $found != "$forbidden" ]]; then
			drift "layer-$name.yml: the regex starting with $prefix forbids \"$found\", but the table forbids \"$forbidden\""
		fi
	done
	said=$(message_names "$rule" | sorted)
	if [[ $said != "$allowed" ]]; then
		drift "layer-$name.yml: the message allows \"$said\", but the table allows \"$allowed\""
	fi
	for glob in "src/$name.rs" "src/$name/**/*.rs"; do
		if ! grep -qxF "  - $glob" "$rule"; then
			drift "layer-$name.yml: files should list $glob"
		fi
	done
done <<<"$rows"

for rule in "$rules"/layer-*.yml; do
	[[ -e $rule ]] || continue
	case " $expected_rules " in
	*" ${rule##*/} "*) ;;
	*) drift ".ast-grep/rules/${rule##*/} checks a module the table does not list or one that may use every module" ;;
	esac
done

# The files the I/O invariant names, and the ones that expect the lint.
named_io=$(awk '
  /^## / { inside = 0 }
  /^- \*\*/ { inside = index($0, "**I/O happens only at the ends.**") > 0 }
  inside {
    line = $0
    while (match(line, /`[^`]*\.rs`/)) {
      print "src/" substr(line, RSTART + 1, RLENGTH - 2)
      line = substr(line, RSTART + RLENGTH)
    }
  }
' "$doc" | LC_ALL=C sort -u)
expecting=$(
	cd "$root" &&
		find src -name '*.rs' ! -name tests.rs ! -path '*/tests/*' ! -path src/testing.rs -print |
		LC_ALL=C sort |
			while read -r file; do
				awk -v file="$file" '
				  /^#!\[expect\(/ { open = 1 }
				  open && /clippy::disallowed_methods/ { print file; exit }
				  open && /\)\]/ { open = 0 }
				' "$file"
			done
)
while read -r file; do
	[[ -n $file ]] && drift "the I/O invariant names $file, which does not expect clippy::disallowed_methods"
done < <(LC_ALL=C comm -23 <(printf '%s\n' "$named_io") <(printf '%s\n' "$expecting"))
while read -r file; do
	[[ -n $file ]] && drift "$file expects clippy::disallowed_methods, but the I/O invariant does not name it"
done < <(LC_ALL=C comm -13 <(printf '%s\n' "$named_io") <(printf '%s\n' "$expecting"))

if [[ $failed -ne 0 ]]; then
	echo "Update ARCHITECTURE.md and the code together." >&2
fi
exit "$failed"

#!/usr/bin/env bash
# Checks that ARCHITECTURE.md agrees with the code it describes.
#
# - The module table under "## Code map" lists the modules src/lib.rs
#   declares, the test-only ones aside.
# - Each .ast-grep/rules/layer-<module>.yml is exactly the rule this script
#   writes from the table: it forbids every module but itself and the ones
#   it may use, which are those before it in the table, or the ones its row
#   names after "uses only". A module that may use all the others has no
#   rule, and there is no rule for a module the table does not list.
# - The .rs files the "I/O happens only at the ends." invariant names are
#   the files outside the tests that expect clippy::disallowed_methods.
# - The .rs files the "Properties hold for any input." invariant names are
#   the tests/properties.rs files under src, and each runs its properties
#   through testing::for_all or for_all_cases.
#
# With --write, the layer rules are written from the table instead of
# compared; a rule that should not exist is reported, not deleted.
#
# Not read: a table cell with an escaped `\|`, and "uses only" anywhere but
# in the sentence that lists what a module may use.
#
# Usage: .github/scripts/check-architecture.sh [--write]

set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
doc=$root/ARCHITECTURE.md
rules=$root/.ast-grep/rules
write=0
if [[ ${1-} == --write ]]; then
	write=1
fi
failed=0

# Reports a drift and goes on, so one run lists them all.
drift() {
	echo "ARCHITECTURE.md: $*" >&2
	failed=1
}

# Prints "<module>\t<the modules it may use>" for each row of the module
# table, in order; the second field is "-" when the row does not say
# "uses only". Only POSIX awk is used, so it runs the same on macOS and on
# the CI runner.
table() {
	awk '
	  /^## / { inside = ($0 == "## Code map"); next }
	  inside && /^\| `[a-z0-9_]+` \|/ {
	    split($0, cell, "|")
	    name = cell[2]
	    gsub(/[ `]/, "", name)
	    allowed = "-"
	    if ((i = index(cell[3], "uses only")) > 0) {
	      # The names up to the end of the sentence.
	      rest = substr(cell[3], i + 9)
	      sub(/\..*$/, "", rest)
	      allowed = ""
	      while (match(rest, /`[a-z0-9_]+`/)) {
	        allowed = allowed (allowed == "" ? "" : " ") substr(rest, RSTART + 1, RLENGTH - 2)
	        rest = substr(rest, RSTART + RLENGTH)
	      }
	    }
	    printf "%s\t%s\n", name, allowed
	  }
	' "$doc"
}

# Prints the modules src/lib.rs declares, without those under #[cfg(test)],
# whatever their visibility.
declared() {
	awk '
	  /^#\[cfg\(test\)\]/ { test = 1; next }
	  /^(pub(\([a-z: ]+\))? )?mod [a-z0-9_]+;/ && !test {
	    name = $0
	    sub(/^.*mod /, "", name)
	    sub(/;.*$/, "", name)
	    print name
	  }
	  { test = 0 }
	' "$root/src/lib.rs"
}

# Prints the layer rule for module $1, which may use the modules in $2 and
# no others in $3, both lists space-separated in table order.
layer_rule() {
	local name=$1 may=$2 alt
	alt=$(tr ' ' '|' <<<"$3")
	if [[ -z $may ]]; then
		may="no other module"
	else
		may=${may// /, }
	fi
	cat <<EOF
# Written by .github/scripts/check-architecture.sh --write from the module
# table in ARCHITECTURE.md. Change the table and run it again.
id: layer-$name
language: rust
severity: error
message: "$name may use only $may"
note: |
  A module uses only those before it in the module table of
  ARCHITECTURE.md, or the ones its row names after "uses only".
files:
  - src/$name.rs
  - src/$name/**/*.rs
ignores:
  - src/**/tests.rs
  - src/**/tests/**/*.rs
rule:
  any:
    # crate::module::item for a module this one may not use, in a use or
    # a path
    - kind: scoped_identifier
      regex: ^crate::($alt)\$
    # use crate::{module, module::item, module::{..}, module as x}
    - any:
        - kind: identifier
        - kind: scoped_identifier
        - kind: scoped_use_list
        - kind: use_as_clause
      regex: ^($alt)\\b
      inside:
        kind: use_list
        inside:
          kind: scoped_use_list
          has:
            field: path
            kind: crate
    # crate::module in a macro call or an attribute, which ast-grep sees
    # as tokens
    - kind: identifier
      regex: ^($alt)\$
      inside:
        kind: token_tree
      follows:
        regex: ^::\$
        follows:
          kind: crate
EOF
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
		allowed=${earlier# }
	fi
	earlier="$earlier $name"
	forbidden=""
	for other in $modules; do
		case " $name $allowed " in
		*" $other "*) ;;
		*) forbidden="$forbidden $other" ;;
		esac
	done
	forbidden=${forbidden# }
	rule=$rules/layer-$name.yml
	if [[ -z $forbidden ]]; then
		if [[ -e $rule ]]; then
			drift "\`$name\` may use every module, so delete .ast-grep/rules/layer-$name.yml"
		fi
		continue
	fi
	expected_rules="$expected_rules layer-$name.yml"
	if [[ $write -eq 1 ]]; then
		layer_rule "$name" "$allowed" "$forbidden" >"$rule"
	elif [[ ! -e $rule ]]; then
		drift "\`$name\` needs .ast-grep/rules/layer-$name.yml; run with --write"
	elif ! layer_rule "$name" "$allowed" "$forbidden" | diff -u "$rule" - >&2; then
		drift "layer-$name.yml is not the rule the table gives (diff above); run with --write"
	fi
done <<<"$rows"

for rule in "$rules"/layer-*.yml; do
	[[ -e $rule ]] || continue
	case " $expected_rules " in
	*" ${rule##*/} "*) ;;
	*) drift "delete .ast-grep/rules/${rule##*/}: the table does not list its module, or the module may use every other one" ;;
	esac
done

# Prints, under src/ and sorted, the .rs files that the invariant whose
# bold title is $1 names.
named_files() {
	awk -v title="**$1**" '
	  /^## / { inside = 0 }
	  /^- \*\*/ { inside = index($0, title) > 0 }
	  inside {
	    line = $0
	    while (match(line, /`[^`]*\.rs`/)) {
	      print "src/" substr(line, RSTART + 1, RLENGTH - 2)
	      line = substr(line, RSTART + RLENGTH)
	    }
	  }
	' "$doc" | LC_ALL=C sort -u
}

# The files the I/O invariant names, and the ones that expect the lint.
named_io=$(named_files "I/O happens only at the ends.")
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

# The property files the invariant names, the ones there are, and whether
# each runs its properties through for_all.
named_properties=$(named_files "Properties hold for any input.")
properties=$(cd "$root" && find src -path '*/tests/properties.rs' -print | LC_ALL=C sort)
while read -r file; do
	[[ -n $file ]] && drift "the property invariant names $file, which does not exist"
done < <(LC_ALL=C comm -23 <(printf '%s\n' "$named_properties") <(printf '%s\n' "$properties"))
while read -r file; do
	[[ -n $file ]] && drift "$file holds properties, but the property invariant does not name it"
done < <(LC_ALL=C comm -13 <(printf '%s\n' "$named_properties") <(printf '%s\n' "$properties"))
while read -r file; do
	[[ -n $file ]] || continue
	grep -Eq 'for_all(_cases)?\(' "$root/$file" ||
		drift "$file runs no property through testing::for_all"
done <<<"$properties"

if [[ $failed -ne 0 ]]; then
	echo "Update ARCHITECTURE.md and the code together." >&2
fi
exit "$failed"

#!/usr/bin/env bash
# Runs the tests with coverage and fails when line coverage is below
# min_lines.
#
# The tests themselves are left out of the count: tests.rs, the files under
# a tests directory, and src/testing.rs. The binary the integration tests
# run is measured too, as it inherits the profile settings.
#
# On GitHub Actions the table of files goes to the job summary as well.
#
# Usage: .github/scripts/coverage.sh

set -euo pipefail

# The floor, in percent. Raise it when coverage goes up.
min_lines=96
ignore='(^|/)(tests\.rs|tests/.*|testing\.rs)$'

cd "$(dirname "$0")/../.."

cargo llvm-cov --locked --no-report
# The report prints the table either way and fails below the floor, without
# saying why.
status=0
table=$(cargo llvm-cov report --ignore-filename-regex "$ignore" \
	--fail-under-lines "$min_lines") || status=$?
echo "$table"
if [[ -n ${GITHUB_STEP_SUMMARY-} ]]; then
	{
		echo "## Coverage"
		echo
		echo "~~~text"
		echo "$table"
		echo "~~~"
	} >>"$GITHUB_STEP_SUMMARY"
fi
if [[ $status -ne 0 ]]; then
	echo "coverage.sh: line coverage is below the floor of $min_lines%; see TOTAL above" >&2
	exit "$status"
fi

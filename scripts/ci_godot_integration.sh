#!/usr/bin/env bash
# Runs gdtest-runner against a real headless Godot + GUT on the minimal
# project in tests/fixtures/gutproject, and checks its verdicts, Run
# Summary and exit codes against a serial GUT run of the same scripts.
#
#   scripts/ci_godot_integration.sh RUNNER GODOT GUT_DIR
#
# RUNNER  the gdtest-runner binary
# GODOT   a Godot 4 binary
# GUT_DIR a GUT checkout or release (the folder that holds addons/gut)
#
# It works on a copy of the project in a temp dir, so the tree stays clean,
# and gives Godot a throwaway XDG_DATA_HOME, so it never touches the real
# user://. Nothing here uses the network.
set -euo pipefail

if [[ $# -ne 3 ]]; then
	echo "usage: $0 RUNNER GODOT GUT_DIR" >&2
	exit 2
fi
runner=$(realpath "$1")
godot=$(realpath "$2")
gut_dir=$(realpath "$3")
repo=$(cd "$(dirname "$0")/.." && pwd)

# What the fixture holds (tests/fixtures/gutproject/tests/unit): 4 script
# files, 9 tests, 14 asserts, all passing. tests/cli.rs checks the static
# count of the same files.
expected_files=4
expected_tests=9
expected_asserts=14

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
project="$work/project"
cp -R "$repo/tests/fixtures/gutproject" "$project"
mkdir -p "$project/addons" "$project/tools/gdtest"
cp -R "$gut_dir/addons/gut" "$project/addons/gut"
cp "$repo/reporter/gdtest_reporter.gd" "$project/tools/gdtest/"

# The config names the binary "godot"; put it first on PATH.
mkdir -p "$work/bin"
ln -s "$godot" "$work/bin/godot"
export PATH="$work/bin:$PATH"
export XDG_DATA_HOME="$work/xdg"
mkdir -p "$XDG_DATA_HOME"

failures=0
fail() {
	echo "FAILED: $*" >&2
	failures=$((failures + 1))
}

strip_colours() { sed 's/\x1b\[[0-9;]*m//g' "$1"; }

# The value of a Totals line ("Passing Tests    9") in the last Run Summary.
total() {
	strip_colours "$1" | awk -v label="$2" '
		/^= Run Summary/ { value = "" }
		index($0, label) == 1 {
			rest = substr($0, length(label) + 1)
			if (rest ~ /^ +[^ ]+$/) { gsub(/ /, "", rest); value = rest }
		}
		END { print value }'
}

# --- 1. import: GUT's classes and the reporter's .uid ---------------------
echo "== godot --headless --import"
(cd "$project" && godot --headless --import) >"$work/import.log" 2>&1 || {
	cat "$work/import.log"
	echo "FAILED: godot --import" >&2
	exit 1
}

# --- 2. GUT alone, serially: the reference totals --------------------------
echo "== GUT alone, serial"
serial_log="$work/gut-serial.log"
serial_xdg=$(mktemp -d)
set +e
(cd "$project" && XDG_DATA_HOME="$serial_xdg" GDTEST_PORT_BASE=20000 \
	godot --headless -s addons/gut/gut_cmdln.gd -gconfig= \
	-gdir=res://tests/unit -gprefix=test_ -gsuffix=.gd -gexit) >"$serial_log" 2>&1
serial_exit=$?
set -e
rm -rf "$serial_xdg"
strip_colours "$serial_log" | sed -n '/^= Run Summary/,$p'
[[ $serial_exit -eq 0 ]] || fail "serial GUT exited $serial_exit"

labels=("Scripts" "Tests" "Passing Tests" "Asserts")
declare -A reference
for label in "${labels[@]}"; do
	reference[$label]=$(total "$serial_log" "$label")
done
[[ ${reference[Tests]} == "$expected_tests" ]] ||
	fail "serial GUT: Tests ${reference[Tests]}, expected $expected_tests"
[[ ${reference[Passing Tests]} == "$expected_tests" ]] ||
	fail "serial GUT: Passing Tests ${reference[Passing Tests]}, expected $expected_tests"
[[ ${reference[Asserts]} == "$expected_asserts" ]] ||
	fail "serial GUT: Asserts ${reference[Asserts]}, expected $expected_asserts"

# --- 3. the runner, per script and batched ---------------------------------
check_passing_run() { # mode
	local mode=$1 out="$work/runner-$1.log" code
	echo "== gdtest-runner --mode $mode"
	rm -rf "$project/.gdtest"
	set +e
	"$runner" --config "$project/gdtest.toml" --mode "$mode" --plain >"$out" 2>&1
	code=$?
	set -e
	cat "$out"
	[[ $code -eq 0 ]] || fail "--mode $mode exited $code, expected 0"
	grep -q "ran $expected_files of $expected_files scripts" "$out" ||
		fail "--mode $mode: no 'ran $expected_files of $expected_files scripts' line"
	grep -q -- "---- All tests passed! ----" "$out" ||
		fail "--mode $mode: no 'All tests passed!' line"
	for label in "${labels[@]}"; do
		local got
		got=$(total "$out" "$label")
		[[ $got == "${reference[$label]}" ]] ||
			fail "--mode $mode: $label $got, serial GUT has ${reference[$label]}"
	done
	local rows
	rows=$(($(wc -l <"$project/.gdtest/timings.tsv") - 1))
	[[ $rows -eq $expected_files ]] ||
		fail "--mode $mode: timings.tsv has $rows rows, expected $expected_files"
}
check_passing_run script
check_passing_run batch

# --- 4. a failing test: verdict FAIL, exit 1 -------------------------------
echo "== gdtest-runner, a failing test"
out="$work/runner-failing.log"
set +e
"$runner" --config "$project/gdtest-failing.toml" --plain >"$out" 2>&1
code=$?
set -e
cat "$out"
[[ $code -eq 1 ]] || fail "failing run exited $code, expected 1"
grep -q "FAIL" "$out" || fail "failing run: no FAIL verdict"
grep -q -- "---- 1 failing tests ----" "$out" ||
	fail "failing run: no '1 failing tests' line"
[[ $(total "$out" "Passing Tests") == 1 ]] || fail "failing run: Passing Tests is not 1"
[[ $(total "$out" "Failing Tests") == 1 ]] || fail "failing run: Failing Tests is not 1"

if [[ $failures -gt 0 ]]; then
	echo "$failures check(s) failed" >&2
	exit 1
fi
echo "All integration checks passed."

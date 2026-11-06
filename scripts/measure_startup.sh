#!/usr/bin/env bash
# Measures the fixed cost of a fresh Godot process per test script, the input
# to the choice between per-script and batch mode (docs/measurements.md).
#
#   scripts/measure_startup.sh <project dir> <res://path/to/fast_test.gd> [runs=5] [godot=godot4] [extra GUT args...]
#
# Extra GUT arguments (a project's -gpre_run_script=..., say) go after godot.
#
# Each run starts a GUT process for the one script (the command line
# gdtest-runner uses, minus the reporter) under its own throwaway
# XDG_DATA_HOME, and prints its wall time next to the time GUT itself
# reports for the tests: the difference is the per-process overhead
# (engine start, project and autoload load, GUT setup, shutdown).
set -uo pipefail
project=$1 script=$2 runs=${3:-5} godot=${4:-godot4}
shift $(($# < 4 ? $# : 4))
cd "$project" || exit 2
now() { date +%s.%N; }
for i in $(seq 1 "$runs"); do
	data=$(mktemp -d)
	t0=$(now)
	XDG_DATA_HOME=$data timeout 300 "$godot" --headless --fixed-fps 60 -s addons/gut/gut_cmdln.gd \
		-gconfig= -glog=1 -gexit "$@" -gtest="$script" >"$data/gut.log" 2>&1
	status=$?
	t1=$(now)
	tests=$(sed 's/\x1b\[[0-9;]*m//g' "$data/gut.log" | awk '$1 == "Tests" { print $2 }')
	gut_time=$(sed 's/\x1b\[[0-9;]*m//g' "$data/gut.log" | awk '$1 == "Time" { sub("s", "", $2); print $2 }')
	rm -rf "$data"
	awk -v i="$i" -v a="$t0" -v b="$t1" -v s="$status" -v t="$tests" -v g="${gut_time:-0}" \
		'BEGIN { printf "run %d: process wall %.2f s, GUT test time %.2f s, overhead %.2f s (exit %s, tests %s)\n", i, b - a, g, b - a - g, s, t }'
done

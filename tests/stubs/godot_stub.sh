#!/usr/bin/env bash
# A stand-in for Godot + GUT + the reporter in gdtest-runner's tests. It runs
# the scripts of -gtest=res://a.gd[,res://b.gd...] in order, like GUT, and
# each script's behaviour comes from its file name:
#   test_pass*     two passing tests
#   test_fail*     one passing, one failing test (exit 1 at the end)
#   test_crash*    prints a start, then the process dies by SIGSEGV
#   test_hang*     prints a start, then sleeps (a child sleeps too)
#   test_mismatch* events show 2 tests, the summary counts 3
#   test_noreport* counted in the summary without any reporter event (alone:
#                  no run_end either, as without the reporter)
# After the last script: GUT's summary over all of them, then run_end.
# STUB_DELAY sleeps once at the start, STUB_SCRIPT_DELAY before each script.
# It also prints the environment the runner set, for the tests to check.
# STUB_WARN="test_pass_a=2,test_pass_b=1" makes those scripts print that many
# [GUT WARNING] lines after their header; STUB_ORPHAN="test_pass_c=3" makes
# them leave that many orphans, listed (by file name) in the "= N Orphans"
# section before the summary, which also gets Warnings / Orphans lines.
tests=""
for a in "$@"; do
	case "$a" in -gtest=*) tests=${a#-gtest=} ;; esac
done
echo "STUB args: $*"
echo "STUB pid=$$ data=${XDG_DATA_HOME:-unset} port=${GUT_PORT_BASE:-unset} pre=${GDTEST_PRE_RUN:-unset} extra=${EXTRA:-unset} cwd=$PWD"
[ -n "${XDG_DATA_HOME:-}" ] && [ -d "$XDG_DATA_HOME" ] && echo "STUB data dir exists"
[ -n "${STUB_DELAY:-}" ] && sleep "$STUB_DELAY"

ev() { printf '\033[0m@@gdtest %s\n' "$1"; }
start() { ev "{\"event\":\"script_start\",\"script\":\"$script\",\"inner\":\"\",\"tests\":$1}"; }
test_end() { # name status asserts message
	local msgs="[]"
	[ -n "${4:-}" ] && msgs="[\"$4\"]"
	ev "{\"event\":\"test_start\",\"script\":\"$script\",\"inner\":\"\",\"test\":\"$1\"}"
	ev "{\"event\":\"test_end\",\"script\":\"$script\",\"inner\":\"\",\"test\":\"$1\",\"status\":\"$2\",\"asserts\":$3,\"seconds\":0.01,\"messages\":$msgs}"
}
script_end() { # tests passing failing asserts_passed asserts_failed
	ev "{\"event\":\"script_end\",\"script\":\"$script\",\"inner\":\"\",\"tests\":$1,\"passing\":$2,\"failing\":$3,\"pending\":0,\"risky\":0,\"asserts_passed\":$4,\"asserts_failed\":$5,\"seconds\":0.5,\"setup_failures\":[]}"
}

pick() { # "a=1,b=2" name -> the number for name, or 0
	local pair
	IFS=',' read -r -a pairs <<<"$1"
	for pair in "${pairs[@]}"; do
		[ "${pair%%=*}" = "$2" ] && { echo "${pair#*=}"; return; }
	done
	echo 0
}

# Totals for the summary.
S=0 T=0 P=0 F=0 A=0 FA=0 W=0 O=0
ORPHAN_LINES=""
IFS=',' read -r -a list <<<"$tests"
for script in "${list[@]}"; do
	name=$(basename "$script" .gd)
	echo "STUB running $name"
	echo "$script"
	w=$(pick "${STUB_WARN:-}" "$name")
	for ((i = 0; i < w; i++)); do
		echo "[GUT WARNING]:  Test script has 3 unfreed children."
	done
	W=$((W + w))
	o=$(pick "${STUB_ORPHAN:-}" "$name")
	if [ "$o" != 0 ]; then
		ORPHAN_LINES="$ORPHAN_LINES$name.gd"$'\n'"    - test_one"$'\n'
		for ((i = 0; i < o; i++)); do
			ORPHAN_LINES="$ORPHAN_LINES        * [<Node#$i>]"$'\n'
		done
		echo "    $o Orphans"
	fi
	O=$((O + o))
	[ -n "${STUB_SCRIPT_DELAY:-}" ] && sleep "$STUB_SCRIPT_DELAY"
	case "$name" in
	test_pass*)
		start 2
		test_end test_one passed 1
		test_end test_two passed 2
		script_end 2 2 0 3 0
		S=$((S + 1)) T=$((T + 2)) P=$((P + 2)) A=$((A + 3))
		;;
	test_fail*)
		start 2
		test_end test_one passed 1
		test_end test_two failed 1 "expected 1 got 2"
		script_end 2 1 1 1 1
		S=$((S + 1)) T=$((T + 2)) P=$((P + 1)) F=$((F + 1)) A=$((A + 2)) FA=$((FA + 1))
		;;
	test_crash*)
		start 2
		test_end test_one passed 1
		kill -SEGV $$
		;;
	test_hang*)
		start 2
		sleep 600 &
		sleep 600
		;;
	test_mismatch*)
		start 2
		test_end test_one passed 1
		test_end test_two passed 1
		script_end 2 2 0 2 0
		S=$((S + 1)) T=$((T + 3)) P=$((P + 3)) A=$((A + 3))
		;;
	test_noreport*)
		S=$((S + 1)) T=$((T + 2)) P=$((P + 2)) A=$((A + 2))
		;;
	*)
		echo "stub: unknown mode $name" >&2
		exit 3
		;;
	esac
done

if [ "$O" != 0 ]; then
	echo "= $O Orphans"
	printf '%s' "$ORPHAN_LINES"
fi
echo "= Run Summary"
echo "Totals"
[ "$W" != 0 ] && echo "Warnings $W"
echo "Scripts $S"
echo "Tests $T"
echo "Passing Tests $P"
[ "$F" != 0 ] && echo "Failing Tests $F"
if [ "$FA" != 0 ]; then
	echo "Asserts $((A - FA))/$A"
else
	echo "Asserts $A"
fi
[ "$O" != 0 ] && echo "Orphans $O"
echo "Time 0.5s"
if [ "${#list[@]}" = 1 ] && [[ "${list[0]}" == *test_noreport* ]]; then
	exit 0
fi
ev '{"event":"run_end"}'
[ "$F" != 0 ] && exit 1
exit 0

extends GutHookScript

## gdtest-runner's live reporter: a GUT pre-run hook that prints one JSON line
## per event, each starting with "@@gdtest ", for the runner to read from
## stdout. Vendor this file into the project (any res:// path) and point the
## runner's `reporter` setting at it; the runner passes it as
## -gpre_run_script. The project's own pre-run hook, if any, comes in the
## GDTEST_PRE_RUN environment variable (a res:// path) and runs right after
## the reporter attaches, with its abort and exit code carried over.
##
## Events (field "event"):
##   script_start   script, inner, tests (collected)
##   test_start     script, inner, test
##   test_end       script, inner, test, status (passed|failed|pending|risky),
##                  asserts, seconds, messages (fail and pending texts)
##   script_end     script, inner, tests, passing, failing, pending, risky,
##                  asserts_passed, asserts_failed, seconds, setup_failures
##   script_skipped script, inner, reason
##   run_end        (no fields)

const PREFIX := "@@gdtest "
const META_KEY := "gdtest_reporter"


class Listener:
	extends RefCounted

	var _gut: Node
	var _open: Object = null
	var _open_start_usec := 0

	func _init(gut_node: Node) -> void:
		_gut = gut_node
		_gut.start_script.connect(_on_start_script)
		_gut.start_test.connect(_on_start_test)
		_gut.end_test.connect(_on_end_test)
		_gut.end_script.connect(_on_end_script)
		_gut.end_run.connect(_on_end_run)

	func _on_start_script(collected: Object) -> void:
		_close_open()
		_open = collected
		_open_start_usec = Time.get_ticks_usec()
		_emit(
			{
				"event": "script_start",
				"script": collected.path,
				"inner": String(collected.inner_class_name),
				"tests": collected.tests.size(),
			}
		)

	func _on_start_test(test_name: Variant) -> void:
		if _open == null:
			return
		_emit(
			{
				"event": "test_start",
				"script": _open.path,
				"inner": String(_open.inner_class_name),
				"test": str(test_name),
			}
		)

	func _on_end_test() -> void:
		var test: Object = _gut.get_current_test_object()
		if _open == null or test == null:
			return
		var messages: Array = []
		messages.append_array(test.fail_texts)
		messages.append_array(test.pending_texts)
		_emit(
			{
				"event": "test_end",
				"script": _open.path,
				"inner": String(_open.inner_class_name),
				"test": str(test.name),
				"status": _status(test),
				"asserts": test.assert_count,
				"seconds": test.time_taken,
				"messages": messages,
			}
		)

	func _on_end_script() -> void:
		_close_open()

	func _on_end_run() -> void:
		_close_open()
		_emit({"event": "run_end"})

	## A script GUT skips never emits end_script: it closes here, when the
	## next one starts or the run ends.
	func _close_open() -> void:
		if _open == null:
			return
		var collected: Object = _open
		_open = null
		if collected.was_skipped:
			_emit(
				{
					"event": "script_skipped",
					"script": collected.path,
					"inner": String(collected.inner_class_name),
					"reason": str(collected.skip_reason),
				}
			)
			return
		var setup_failures: Array = []
		for setup_test in collected.setup_teardown_tests:
			setup_failures.append_array(setup_test.fail_texts)
		_emit(
			{
				"event": "script_end",
				"script": collected.path,
				"inner": String(collected.inner_class_name),
				"tests": collected.get_ran_test_count(),
				"passing": collected.get_passing_test_count(),
				"failing": collected.get_failing_test_count(),
				"pending": collected.get_pending_count(),
				"risky": collected.get_risky_count(),
				"asserts_passed": collected.get_pass_count(),
				"asserts_failed": collected.get_fail_count(),
				"seconds": (Time.get_ticks_usec() - _open_start_usec) / 1000000.0,
				"setup_failures": setup_failures,
			}
		)

	static func _status(test: Object) -> String:
		if test.is_failing():
			return "failed"
		if test.is_pending():
			return "pending"
		if test.is_passing():
			return "passed"
		return "risky"

	static func _emit(payload: Dictionary) -> void:
		print(PREFIX + JSON.stringify(payload))


func run() -> void:
	# The listener hangs on the run object as meta, which keeps it alive.
	gut.set_meta(META_KEY, Listener.new(gut))
	var chained := OS.get_environment("GDTEST_PRE_RUN")
	if chained.is_empty():
		return
	var script: Script = load(chained)
	if script == null:
		push_error("gdtest_reporter: cannot load GDTEST_PRE_RUN %s" % chained)
		set_exit_code(1)
		abort()
		return
	var hook: Object = script.new()
	hook.gut = gut
	await hook.run()
	if hook.get_exit_code() != null:
		set_exit_code(hook.get_exit_code())
	if hook.should_abort():
		abort()

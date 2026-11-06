# Reporter protocol

The reporter, [`reporter/gdtest_reporter.gd`](../reporter/gdtest_reporter.gd),
is a GUT pre-run hook (`GutHookScript`). It connects to GUT's
`start_script`, `start_test`, `end_test`, `end_script` and `end_run` signals
and prints one line per event on stdout:

```text
@@gdtest {"event":"test_end","script":"res://tests/unit/test_math.gd",...}
```

A line is the prefix `@@gdtest` and a space, followed by one JSON object.
GUT's logger sometimes leaves a colour reset (`ESC[0m`) at the start of
the line; the runner strips ANSI escape codes before it looks for the
prefix. A line with the prefix whose JSON does not parse is a protocol
error and makes the script a `MISMATCH`; it is never skipped.

## Events

Every event has an `event` field. `script` is the `res://` path of the
test file; `inner` is the inner test class name, or `""` for the file's
own tests. GUT treats each inner class as a separate collected script, so
one file can produce several `script_start` / `script_end` pairs.

| `event` | Other fields | When |
| --- | --- | --- |
| `script_start` | `script`, `inner`, `tests` | GUT starts a collected script. `tests` is how many tests GUT collected in it. |
| `test_start` | `script`, `inner`, `test` | A test starts. |
| `test_end` | `script`, `inner`, `test`, `status`, `asserts`, `seconds`, `messages` | A test ends. |
| `script_end` | `script`, `inner`, `tests`, `passing`, `failing`, `pending`, `risky`, `asserts_passed`, `asserts_failed`, `seconds`, `setup_failures` | A collected script ends. |
| `script_skipped` | `script`, `inner`, `reason` | GUT skipped a collected script (`should_skip_script`). Sent instead of `script_end`. |
| `run_end` | none | GUT's run is over. |

Field types:

- `status`: `"passed"`, `"failed"`, `"pending"` or `"risky"` (a test with
  no assert).
- `asserts`, `tests`, `passing`, `failing`, `pending`, `risky`,
  `asserts_passed`, `asserts_failed`: non-negative integers.
- `seconds`: a number. In `test_end` it is GUT's own time for the test; in
  `script_end` it is the wall time from `script_start`.
- `messages`: list of strings, the failure and pending texts of the test.
- `setup_failures`: list of strings, the failure texts of
  `before_all`/`after_all` asserts.

## What the runner checks

For each process, the runner compares:

1. The events with GUT's own Run Summary at the end of the log (scripts,
   tests, passing, failing, risky/pending, asserts, failed asserts).
2. The number of `test_end` events and their statuses with the same
   summary.
3. Each script's `script_end` test counts, per inner class, with the
   static discovery of that file.

Any disagreement is a `MISMATCH`. No events at all, or no `run_end`, is a
`MISMATCH` that points at the reporter.

## Chaining a project hook

GUT accepts one pre-run hook. The runner always passes the reporter, so a
project hook goes in the config's `pre_run_script`; the runner sets
`GDTEST_PRE_RUN` to it, and the reporter loads it, runs it (awaiting its
`run()`), and copies its exit code and abort flag.

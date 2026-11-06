# Architecture

gdtest-runner is a single binary (`src/main.rs`) over a small library
(`src/lib.rs`). This page follows one run from start to end and names the
module that owns each step.

```text
config ─> discovery ─> queue ─> workers ─> Godot + GUT + reporter
                                  │               │
                                  │        events on stdout, log
                                  ▼               │
                          verdict per script <────┘
                                  │
                         UI (TUI or plain) ─> summary, timings
```

## 1. Config (`config.rs`)

`Config::load` parses the TOML file with `deny_unknown_fields`, resolves
`project` against the config file's folder, canonicalises it, and validates
the values (at least one worker, a `res://` reporter, a non-empty
`test_dirs`, a sane `[batch]` table). See
[configuration.md](configuration.md).

## 2. Discovery (`discover.rs`)

Discovery reads the test files without starting Godot. It walks
`test_dirs` for files named `<script_prefix>*.gd` and, in each file,
collects:

- top-level `func <test_prefix>*(` lines;
- inner classes named `<inner_class_prefix>*` that extend `GutTest` (or
  GUT's `test.gd`), and their `func <test_prefix>*(` lines one indentation
  level in.

The result, `ScriptInfo`, holds each file's path and its tests per class.
Its count is the "ran N of N" guard: whatever GUT reports must match it.

## 3. Queue (`timings.rs`, `run.rs`)

`queue_order` sorts the scripts: those with no row in the timings first
(by path), then the known ones slowest first. One shared `VecDeque` holds
the order; there are no shards fixed up front.

`take_batch` hands a worker its next unit of work: the queue's front
script, plus, when that script is batchable, the following batchable
scripts in queue order, up to `max_scripts` and within the `max_seconds`
budget. Scripts it skips keep their places.

## 4. Workers (`run.rs`, `worker.rs`)

`run_all` starts one thread per worker slot. Each thread loops: take work,
run it, report, until the queue is empty or a signal stops the run.

`run_script` and `run_batch` start Godot (`run_process`):

- from the project folder, in a new process group;
- with `[env]`, `GDTEST_PRE_RUN`, a fresh data dir in `data_dir_var` and
  the slot's port base in `port_base_var`;
- with stdout and stderr read line by line on two threads into one channel,
  written to the log, and parsed for reporter events as they arrive.

The timeout is per script: it restarts when a new script's first event
arrives. On a timeout or a signal the whole process group gets SIGKILL. If
the process exits but something it started still holds the pipes, the
group is killed after a 2 s grace period. The data dir is removed after
the process ends.

## 5. Batches (`batch.rs`)

A batch is one process over several scripts (`-gtest=a,b,c`). Events carry
their script, so `judge_batch` splits them per script and judges each one
on its own events and its own discovery. GUT's Run Summary covers the
whole process, so it is checked against all the batch's events; a
disagreement makes every script of the batch a `MISMATCH`.

When the process ends without a summary (a crash, a timeout, a failed
start), the script whose event came last takes the `CRASH` or `TIMEOUT`,
the scripts that finished before it keep their verdicts, and the scripts
that never started are re-queued at the front for a fresh process.

The batch log is cut into per-script logs at each script's first event,
and each script's wall time is its segment of the process.

## 6. Events and GUT's summary (`events.rs`)

`parse_event_line` turns an `@@gdtest` line into an `Event` (see
[reporter-protocol.md](reporter-protocol.md)). `parse_summary` reads the
last `= Run Summary` block of the log, colours stripped. `parse_log_notes`
attributes `[GUT WARNING]` lines to the last `res://...gd` header before
them, and orphans to the file names in GUT's `= N Orphans` section.

## 7. Verdicts (`verdict.rs`)

`judge` turns one process's `ProcessRun` (exit, events, protocol errors,
summary, notes, wall time) into a `Verdict`. The order of the checks:

1. Timed out: `TIMEOUT`.
2. Could not start, no summary, a signal, or a non-zero exit with no
   failing test in the summary: `CRASH`.
3. Protocol errors, events against summary, events against discovery: any
   problem is a `MISMATCH`.
4. A failing test, a failed assert, or a `before_all`/`after_all` failure:
   `FAIL`.
5. Otherwise `ok`.

## 8. Output (`ui.rs`, `summary.rs`)

The calling thread receives every `Started`, `Event` and `Finished`
message in arrival order and hands it to the UI:

- `Tui` redraws a block of header plus one pane per worker (at most every
  80 ms, and every 100 ms while idle) and scrolls failures above it. Lines
  are cut to the terminal width so a redraw never wraps.
- `Plain` writes one line per finished script and the details of each
  problem.

`RunReport::render` prints the slowest scripts, the `ran N of M` line and a
Run Summary in GUT's layout. The run is green only when every discovered
script ran and passed.

## 9. Timings (`timings.rs`)

After the run, `write_script_timings` writes this run's rows plus the old
rows of scripts that still exist but did not run, slowest first, through a
temp file and a rename. `write_test_timings` writes one row per test.

## Tests

`tests/` drives every layer without Godot:

- `tests/stubs/godot_stub.sh` stands in for Godot + GUT + the reporter. A
  script's behaviour comes from its file name (`test_pass*`, `test_fail*`,
  `test_crash*`, `test_hang*`, `test_mismatch*`, `test_noreport*`), one
  script or a batch per process.
- `tests/fixtures/gut/*.log` are captured from real GUT 9.7.1 runs with the
  reporter, with project-specific names replaced.
- `tests/cli.rs` runs the built binary end to end against the stub.
- `tests/fixtures/gutproject/` is a minimal real GUT project. CI runs it
  under a real headless Godot with `scripts/ci_godot_integration.sh`; see
  [ci.md](ci.md).

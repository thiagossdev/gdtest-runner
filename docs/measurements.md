# Measurements

The numbers in the README come from one real project's suite, run while
the runner was developed. The project is not public; its script names are
replaced in the demo image and the test fixtures.

## Machine and suite

- 6 physical cores / 12 threads, Linux under WSL2.
- Godot 4.8 pre-release, headless, `--fixed-fps 60` (each frame advances
  exactly 1/60 s of game time, as fast as the CPU allows).
- GUT 9.7.1.
- 149 test scripts, 1956 tests: 94 small unit scripts (1212 tests) and 55
  integration scripts, some of which simulate several seconds of gameplay.
- Every process ran with its own throwaway `XDG_DATA_HOME`.

## Method

- **Serial baseline**: GUT alone, one process over the whole suite, the
  command line a project would use without the runner.
- **Runner runs**: the same commit, the same Godot flags, from a cold
  start, with the timings of an earlier run as the queue order.
- **Agreement**: for each comparison, the per-script test and assert counts
  of the runner's run were compared with the serial run's; they matched.
- **Startup cost**: `scripts/measure_startup.sh` runs one 4-test script
  (tests take about 0.01 s) five times in fresh processes and prints wall
  time against GUT's own test time.

Wall times are single runs or small sets of runs; expect a few seconds of
spread on a machine that is doing other work.

## Results

| Run | Wall time |
| --- | --- |
| GUT alone, one process, serial | about 111 to 113 s |
| One Godot per script, 6 workers | about 73 to 84 s |
| Every script batched, `max_seconds = 5`, 6 workers | about 36 to 38 s |

Batched with `max_seconds = 5`, by worker count:

| Workers | Wall time |
| --- | --- |
| 4 | 46 s |
| 6 | 38 s |
| 8 | 34 s |
| 10 | 30 s |

The 94 unit scripts alone:

| Run | Wall time |
| --- | --- |
| GUT alone, one process, serial | about 8.4 s |
| One Godot per script, 6 workers | 34.6 s |
| Batched, 6 workers | about 8.5 s |

## Reading the numbers

- **Startup dominates small scripts.** A fresh Godot costs about 1.2 s
  (engine start, project and autoloads, GUT setup, GUT's own final wait,
  shutdown) before and after the tests. With 6 processes on 6 physical
  cores it grows to about 2 s. That is why one-process-per-script was
  slower than serial GUT for the unit scripts.
- **A time budget keeps batches balanced.** With no budget, the slowest
  integration scripts can pile into one or two processes and that run
  ended later than per-script mode. `max_seconds` caps the previous-timings
  seconds per process so the long scripts spread over the workers.
- **More workers help until the cores run out.** The suite kept getting
  faster up to 10 workers on 12 hardware threads; the integration
  scripts are CPU-bound simulations.

## The demo image

[`assets/tui-demo.svg`](assets/tui-demo.svg) is drawn from a terminal
recording (`script -T timing ...`) of a per-script run of this suite with
a development build before batch mode: one redraw part way through and the last screen. The
recorded project's script and test names are replaced with neutral ones;
the counts, times and layout are the recording's.

To record your own run:

```sh
script -q -T timing -c 'gdtest-runner' tui.typescript
scriptreplay -t timing tui.typescript
```

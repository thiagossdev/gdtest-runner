# gdtest-runner

A vitest-like parallel runner for Godot 4 + GUT test suites, in Rust. Read
`README.md` for what it does and `docs/architecture.md` for how.

## Stack

- Rust, edition 2024, MSRV 1.88 (`rust-version` in `Cargo.toml`).
- Dependencies: `libc`, `serde`, `serde_json`, `toml`. Add one only when
  the standard library cannot do the job.
- `reporter/gdtest_reporter.gd`: GDScript, a GUT 9 pre-run hook. Tab
  indentation, GUT's API only.
- Unix only (process groups, signals): CI tests Linux and macOS; real
  Godot runs on Linux.

## Layout

- `src/main.rs`: command line, wiring, exit codes.
- `src/config.rs`: TOML config. `src/discover.rs`: static test discovery.
- `src/timings.rs`: timings files and queue order.
- `src/run.rs`: scheduler and batch selection. `src/worker.rs`: one Godot
  process (spawn, isolation, timeout, logs).
- `src/batch.rs`: per-script verdicts of a batch. `src/verdict.rs`:
  verdict of one process. `src/events.rs`: reporter events, GUT summary,
  warnings and orphans.
- `src/ui.rs`: TUI and plain output. `src/summary.rs`: the end-of-run
  report.
- `tests/`: integration tests per module, `tests/stubs/godot_stub.sh`,
  `tests/fixtures/`.
- `docs/`: architecture, reporter protocol, configuration, measurements,
  CI, versioning.
- `scripts/ci_godot_integration.sh`: the real-Godot check CI runs on
  `tests/fixtures/gutproject/`.
- `.github/workflows/`: `ci.yml` (fmt, clippy, test on Linux and macOS,
  msrv, integration) and `deny.yml` (cargo-deny with `deny.toml`).
  `.github/dependabot.yml`: weekly crate and action updates.
- `examples/`: a generic config and setup steps.

## Commands

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo package --list      # what a package would contain
cargo deny check          # licences, advisories, sources (deny.toml)
actionlint                # after touching .github/workflows/
```

The first four must pass before a change is done. CI also runs the tests
with `--locked` on the MSRV, on macOS, and against a real Godot; see
`docs/ci.md`.

## How tests work

- No test starts Godot. `tests/stubs/godot_stub.sh` plays Godot + GUT +
  the reporter: it reads `-gtest=res://a.gd[,res://b.gd]` and behaves by
  file name (`test_pass*`, `test_fail*`, `test_crash*`, `test_hang*`,
  `test_mismatch*`, `test_noreport*`). Env knobs: `STUB_DELAY`,
  `STUB_SCRIPT_DELAY`, `STUB_WARN`, `STUB_ORPHAN`. Extend the stub for a
  new behaviour instead of adding a mock in Rust.
- `tests/fixtures/stubproject/` holds the stub's scripts;
  `tests/fixtures/project/` holds real-looking GDScript for discovery.
- `tests/fixtures/gutproject/` is a minimal real GUT project for CI's
  integration job. Its counts are pinned in `tests/cli.rs` and in
  `scripts/ci_godot_integration.sh`; change all three together.
- `tests/fixtures/gut/*.log` are captured from real GUT runs with the
  reporter. A parser change needs a captured log, not a hand-written one.
  Replace project-specific names before committing a capture.
- Expected values come from the captures or the spec, never from the
  code's own formula.

## Rules

- **Nothing project specific.** No game names, paths, script names, ports
  or environment variables from any one project in code, tests, fixtures
  or docs. Everything a project needs comes in through the config or flags.
- **No silent fallbacks.** A bad config, an unreadable event line or a
  count that does not add up is an error or a `MISMATCH`, never skipped.
- **Docs follow behaviour.** A change to flags, config keys, events,
  verdicts or output updates `README.md`, the matching `docs/` page and
  `CHANGELOG.md` (under `Unreleased`).
- **No absolute paths** from your machine in tracked files.
- **Semantic Versioning** (`docs/versioning.md`). Flags, config keys,
  exit codes, verdicts, the Run Summary, written files and the reporter
  protocol are public API. Each user-visible change gets a changelog line
  under `## Unreleased` in a Keep a Changelog subsection, marked
  **Breaking:** when it breaks that API. Never bump `version` or add a
  version section on your own; `tests/versioning.rs` checks the
  changelog against `Cargo.toml`.
- **CI pins.** Actions are pinned to a commit SHA with the version in a
  comment; downloads have a fixed version and checksum. Workflows keep
  `permissions: contents: read`. No release or publish workflow.

## Verifying against a real Godot project

The stub proves the runner's logic, not that it reads real GUT output. A
change to how Godot is started, to the reporter, or to parsing GUT output
also needs a real run:

1. Pick a Godot 4 + GUT project and vendor the reporter (see
   `examples/README.md`); use a throwaway `XDG_DATA_HOME` so the run never
   touches your real `user://`.
2. Run GUT alone, serially, and keep its Run Summary.
3. Run `cargo run --release -- --config <its gdtest.toml> --plain` in
   per-script and in batch mode.
4. Compare: the same Scripts / Tests / Passing / Asserts totals and the
   same per-script counts (from the timings file) as the serial run.

Say in the commit or pull request which Godot and GUT versions you used.

## Git

- Agents never add a remote, push, tag, publish to crates.io, rewrite
  history, or call the GitHub API. The owner does those.
- Small commits with a message that says what changed and why.

# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/): what counts as a breaking
change, and how versions move before 1.0, is in
[docs/versioning.md](docs/versioning.md).

## Unreleased

### Added

- `[batch] separate_dirs` (default `false`): a batch takes scripts of one
  `dirs` entry only, so scripts of two folders never share a Godot
  process.

## 0.1.0 - 2026-11-06

The first public version.

### Added

- Static discovery of GUT test scripts, top-level tests and inner test
  classes, without starting Godot.
- A dynamic queue (unknown scripts first, then slowest first by the
  previous run's timings) feeding N workers, each a fresh headless Godot
  process.
- Per-script mode (one Godot per script) and batch mode (several scripts
  per process, with a verdict per script): `--mode auto|script|batch` and a
  `[batch]` config table (`dirs`, `max_scripts`, `max_seconds`).
- A per-script timeout, also inside a batch. A crash or timeout in a batch
  charges the script that was running; the scripts that never started go
  back to the front of the queue and run in a fresh process.
- Per-process isolation: a fresh data dir and a port base per worker slot,
  through configurable environment variables.
- The GDScript reporter (a GUT pre-run hook) that streams JSON-line events
  and chains the project's own pre-run hook.
- Verdicts `ok`, `FAIL`, `CRASH`, `TIMEOUT`, `MISMATCH`, cross-checked
  against GUT's own summary and the static discovery.
- A live TUI with one pane per worker, plain output when there is no
  terminal, and a Run Summary in GUT's layout that always prints
  `Warnings` and `Orphans`, with an `Orphans by script` list; `--warnings`
  adds a `Warnings by script` list. In batch mode both are attributed per
  script from the process log.
- Per-script logs (cut from the batch log in batch mode).
- Script and test timings files; `--timings-from` as a read-only second
  queue source.
- Docs (`docs/`: architecture, reporter protocol, configuration,
  measurements, CI, versioning), `examples/`, `CONTRIBUTING.md`; licensed
  `MIT OR Apache-2.0`; MSRV Rust 1.88.
- CI on GitHub Actions: fmt, clippy, tests on Linux and macOS, the MSRV,
  and a real headless Godot 4.7.2 + GUT 9.7.1 run per script and batched;
  cargo-deny and Dependabot.
- Published on crates.io (`cargo install --locked gdtest-runner`), and as
  binaries for x86_64 Linux (static) and macOS (Apple Silicon and Intel)
  on each GitHub release.

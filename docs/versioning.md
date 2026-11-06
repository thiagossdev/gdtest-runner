# Versioning

gdtest-runner follows [Semantic Versioning 2.0.0](https://semver.org/).
The version lives in one place, `version` in `Cargo.toml`;
`gdtest-runner --version` prints it, and `CHANGELOG.md` has a section for
it.

## What the public API is

A change to any of these is a change to the public API:

- **Command line**: flags, their values and defaults, the `SCRIPT`
  arguments, `--list` output.
- **Config**: keys, their types, defaults and meaning, and which keys are
  required ([configuration.md](configuration.md)).
- **Exit codes**: `0`, `1`, `2`, `130` and when each is used.
- **Verdicts**: the names `ok`, `FAIL`, `CRASH`, `TIMEOUT`, `MISMATCH` and
  what makes a script get each one.
- **Run Summary**: the `ran N of M scripts` line and the Run Summary block
  in GUT's layout (labels, order, the final `----` line), because tools
  parse it.
- **Files the runner writes**: the timings and test-timings columns, and
  the log file names under `log_dir`.
- **Environment the runner sets** for Godot: `GDTEST_PRE_RUN`, and the
  isolation variables named in the config.
- **Reporter protocol**: the `@@gdtest` events and their fields
  ([reporter-protocol.md](reporter-protocol.md)), and the command line the
  runner gives Godot and GUT. A project vendors the reporter, so a runner
  must keep working with a reporter from the same release line.
- **Supported versions**: the minimum Rust (`rust-version`), and the Godot
  and GUT versions the README says are supported.

Not public: the TUI's layout and colours, the plain output's per-script
progress lines, the wording of error messages, the Rust library in
`src/lib.rs` (it exists for the binary and its tests), and anything under
`docs/` that describes internals.

## Which number goes up

From 1.0.0 on:

| Change | Bump |
| --- | --- |
| Removes or renames a flag, a config key, a verdict, an event or a field; changes a default, an exit code, a file format or the Run Summary layout; needs a newer reporter; drops a supported Godot or GUT version | major |
| Adds a flag, a config key, an event field, a verdict reason or a supported version; raises `rust-version`; deprecates something | minor |
| Fixes a bug without changing anything above; docs, tests, CI, internal refactors | patch |

Before 1.0.0 (now), the same table moves one place to the right, as Cargo
reads `0.y.z` versions: a breaking change bumps the minor (`0.1.3` to
`0.2.0`), anything else bumps the patch (`0.1.3` to `0.1.4`). So
`cargo install --git ... --tag v0.1.x` users get fixes and additions, never
a break.

A bug fix that changes a verdict (a script that was wrongly `ok` becomes a
`MISMATCH`) is a fix, not a break: the documented meaning of the verdict
did not change. Say it plainly in the changelog.

Pre-release versions (`0.2.0-rc.1`) are fine for trying a breaking
change before it lands.

## The changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/):

- Every change a user can see adds a line under `## Unreleased`, in one of
  `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`,
  `### Security`. A breaking change says so: **Breaking:** at the start of
  its line.
- The next version follows from those lines: anything breaking means a
  major (minor before 1.0), anything `Added`, `Changed` or `Deprecated`
  means at least a minor (patch before 1.0), otherwise a patch.

`tests/versioning.rs` checks, on every `cargo test`, that:

- `Cargo.toml`'s version is valid SemVer;
- the changelog has `## Unreleased` first, then version sections
  `## X.Y.Z - YYYY-MM-DD` (the newest may say `- Unreleased` until it is
  tagged), newest first, with no duplicates;
- the newest version section is `Cargo.toml`'s version;
- subsections are only the six names above.

## Making a release

The owner decides when to release; pushing the tag is that decision.

1. Pick the version from the `Unreleased` lines, as above.
2. In one commit: set `version` in `Cargo.toml` (and `cargo update -p
   gdtest-runner` for the lock file), move the `Unreleased` lines under
   `## X.Y.Z - YYYY-MM-DD`, and leave an empty `## Unreleased` above it.
3. Run the checks (`cargo test` covers the version rules,
   `scripts/release_notes.sh vX.Y.Z` prints the notes) and let CI pass
   on `main`.
4. Tag that commit and push the tag:
   `git tag -a vX.Y.Z -m "gdtest-runner X.Y.Z" && git push origin vX.Y.Z`.
   The `Release` workflow checks it again, builds the binaries and
   publishes the GitHub release ([ci.md](ci.md#releaseyml)).

A tag is never moved or reused. A broken release gets a new patch version.

# Continuous integration

Two GitHub Actions workflows run on every push to `main` and every pull
request against it; a third, `release.yml`, runs on version tags. Every
action is pinned to a commit SHA, with its version in a comment, and every
job has only `contents: read`, except the job that publishes a release.

## `ci.yml`

| Job | What it runs |
| --- | --- |
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --locked --all-targets -- -D warnings` |
| `test (ubuntu-latest)`, `test (macos-latest)` | `cargo test --locked` |
| `msrv` | `cargo test --locked` on the `rust-version` read from `Cargo.toml` |
| `integration (Godot 4.7.2-stable, GUT 9.7.1)` | the runner against a real headless Godot, below |

The test matrix has Linux and macOS: the runner uses Unix process groups
and signals (`std::os::unix`, `libc`), so it does not build on Windows.

### The integration job

It downloads two pinned files, and nothing else besides the Rust
toolchain and crates:

- `Godot_v4.7.2-stable_linux.x86_64.zip` from the official Godot release,
  checked against the SHA-512 in that release's `SHA512-SUMS.txt`;
- GUT's `v9.7.1` tag archive, checked against a SHA-256.

Then `scripts/ci_godot_integration.sh` works on a temp copy of
`tests/fixtures/gutproject/`: it adds GUT and the current reporter, runs
`godot --headless --import`, and checks that

1. GUT alone, serially, reports as many tests (all passing) as the
   runner's `--list` finds, 9 today, and 13 asserts. The assert count is
   the only number written in the script: asserts exist only at run time,
   so it is the one check that GUT and the runner cannot both get wrong
   together. GUT also reports 5 scripts, counting the inner class as one;
2. the runner in `--mode script` and in `--mode batch` exits 0, prints
   `ran N of N scripts` (4 script files) and `All tests passed!`, has
   the same Scripts, Tests, Passing Tests and Asserts totals as the
   serial GUT run, and writes a timings row per script;
3. the runner on `tests/failing/` reports a `FAIL`, `1 failing tests`, and
   exits 1.

The fixture's scripts also check isolation from inside Godot: `user://`
lives under the runner's `XDG_DATA_HOME` and starts empty, and the port
base variable is set.

To move to a new Godot or GUT, change the `matrix.include` entry: the
version and the checksum together.

Locally, with a Godot binary and an unpacked GUT release:

```sh
cargo build
scripts/ci_godot_integration.sh target/debug/gdtest-runner /path/to/godot /path/to/Gut-9.7.1
```

## `deny.yml`

`cargo deny check advisories bans licenses sources` with `deny.toml`:
licences must be compatible with `MIT OR Apache-2.0` (`MIT`,
`Apache-2.0`, `Unicode-3.0`), crates come only from crates.io, and any
RustSec advisory fails. It also runs weekly, because a new advisory can
land on an unchanged `Cargo.lock`. Locally: `cargo install --locked
cargo-deny`, then `cargo deny check`.

## Dependabot

`.github/dependabot.yml` opens weekly pull requests for crates and for
the pinned actions. A crate update that needs a newer Rust than
`rust-version` fails the `msrv` job; raise `rust-version` on purpose, or
hold the update.

## Linting the workflows

```sh
actionlint      # checks .github/workflows/*.yml (and shellcheck, if installed, on run: blocks)
```

## `release.yml`

Runs when a tag `vX.Y.Z` is pushed, or by hand (`workflow_dispatch`) with
the name of a tag that already exists (the tag's own commit may predate
the workflow: the checks use the workflow's copy of
`scripts/release_notes.sh` on the tag's `Cargo.toml` and `CHANGELOG.md`).
Three jobs:

1. **check the tag**: the tag is `vX.Y.Z` (a pre-release suffix is
   allowed) and its commit is on `main`; `scripts/release_notes.sh` checks
   that it is `v` + `Cargo.toml`'s version and that `CHANGELOG.md` has a
   dated, non-empty section for it; then `cargo test --locked`.
2. **build**, from scratch (no cache), for `x86_64-unknown-linux-musl`
   (a static binary for any x86_64 Linux), `aarch64-apple-darwin` and
   `x86_64-apple-darwin`. Each archive,
   `gdtest-runner-vX.Y.Z-<target>.tar.gz`, holds the binary, the
   reporter, `README.md`, `CHANGELOG.md` and both licences, with a
   `.sha256` beside it.
3. **publish the GitHub release**: the only job with `contents: write`.
   `gh release create` with the changelog section as notes, the archives
   and a `SHA256SUMS`. A tag with a `-` suffix makes a pre-release. If
   the release already exists, the job fails instead of overwriting it.

Nothing is published to crates.io (`publish = false`).

Check a tag's notes locally before pushing it:

```sh
scripts/release_notes.sh v0.1.0
```

The steps for making a release are in [versioning.md](versioning.md).

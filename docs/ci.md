# Continuous integration

Two GitHub Actions workflows run on every push to `main` and every pull
request against it. Every action is pinned to a commit SHA, with its
version in a comment, and both workflows have only `contents: read`.

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

1. GUT alone, serially, reports 9 tests, 9 passing, 14 asserts;
2. the runner in `--mode script` and in `--mode batch` exits 0, prints
   `ran 4 of 4 scripts` and `All tests passed!`, has the same Scripts,
   Tests, Passing Tests and Asserts totals as the serial GUT run, and
   writes a timings row per script;
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

## Releases: a proposal, not set up

There is no release or publish workflow; releases are the owner's call.
If one is wanted later, a small shape would be:

1. Update `CHANGELOG.md` (move `Unreleased` under the new version) and
   `version` in `Cargo.toml`, in one commit.
2. Tag it `vX.Y.Z` by hand.
3. A workflow on `push: tags: ['v*']` builds release binaries for
   `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` and
   `x86_64-apple-darwin`, and attaches them to a GitHub release with
   `contents: write` scoped to that job only.
4. crates.io stays off (`publish = false`) until the owner decides
   otherwise.

# Contributing

Thanks for taking the time. Bug reports, fixes and small features are
welcome.

## Before you open a pull request

1. Open an issue first for anything larger than a fix, so we can agree on
   the shape before you write it.
2. Keep the runner generic. Nothing in it may assume one Godot project:
   paths, binaries, environment variables and limits come from the config
   or the command line.
3. Add a test. Most behaviour can be tested with the stub Godot in
   `tests/stubs/godot_stub.sh`; parsing changes get a captured GUT log
   in `tests/fixtures/gut/`.
4. Run the checks:

   ```sh
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo fmt --check
   cargo deny check          # if you changed dependencies
   ```

   CI runs the same checks on every pull request, plus the tests on
   macOS, on the minimum Rust version (`rust-version` in `Cargo.toml`)
   and against a real headless Godot + GUT project. A pull request needs
   all of them green. What each job does: [docs/ci.md](docs/ci.md).

5. If the change touches how Godot is started, the reporter or the GUT
   output it reads, also run it against a real Godot + GUT project and say
   in the pull request which Godot and GUT versions you used.
6. If you touch `.github/workflows/`, run `actionlint`, pin new actions to
   a commit SHA with the version in a comment, and keep downloads at a
   fixed version with a checksum.
7. Update `README.md`, `docs/` and `CHANGELOG.md` when behaviour changes.
   In the changelog, add a line under `## Unreleased`, in `### Added`,
   `### Changed`, `### Deprecated`, `### Removed`, `### Fixed` or
   `### Security`, and start it with **Breaking:** if it breaks the public
   API as [docs/versioning.md](docs/versioning.md) defines it. Leave the
   version number alone; the maintainer sets it at release time.

## Licence

By contributing, you agree that your contribution is dual-licensed under
the MIT and Apache-2.0 licences, as the rest of the project, without any
additional terms or conditions.

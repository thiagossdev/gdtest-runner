# Setting up a GUT project

These steps work for any Godot 4 project that runs its tests with GUT 9.

## 1. Install the runner

```sh
cargo install --locked --git https://github.com/thiagossdev/gdtest-runner
gdtest-runner --version
```

## 2. Vendor the reporter

Copy `reporter/gdtest_reporter.gd` from this repository into the project,
anywhere under `res://`. This example uses `res://tools/gdtest/`:

```sh
mkdir -p tools/gdtest
cp /path/to/gdtest-runner/reporter/gdtest_reporter.gd tools/gdtest/
godot --headless --import    # Godot writes the script's .uid file
```

Commit both files. The reporter only prints event lines; it changes
nothing in how GUT runs your tests.

If the project already uses a GUT pre-run hook (`-gpre_run_script`, or
`pre_run_script` in `.gutconfig.json`), set the config's
`pre_run_script` to it. The runner passes the reporter as GUT's pre-run
hook, and the reporter runs yours right after.

## 3. Add the config

Copy [`gdtest.toml`](gdtest.toml) next to `project.godot` and adjust:

- `reporter`: where you put the reporter in step 2.
- `test_dirs`: the folders GUT reads tests from.
- `godot`: your Godot binary.
- `[batch] dirs`: start empty (`dirs = []`) and add folders once you know
  their scripts are safe to share a process.

Add the runner's output folder to `.gitignore`:

```gitignore
.gdtest/
```

## 4. Check discovery, then run

```sh
gdtest-runner --list      # every script with its test count
gdtest-runner             # the whole suite
```

The first run has no timings, so it runs scripts in path order and
writes `.gdtest/timings.tsv`. From the second run on, the slowest scripts
start first.

## 5. In CI

Without a terminal the runner prints plain lines with no escape codes; it
exits 0 only when every discovered script ran and passed. Keep the timings
file between runs (a cache or a committed copy passed with
`--timings-from`) to keep the queue order.

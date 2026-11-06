# Configuration reference

gdtest-runner reads one TOML file, `./gdtest.toml` unless `--config FILE`
names another. Unknown keys, at the top level or in any table, are an
error.

Paths:

- `project` is relative to the config file's own folder.
- Every other path in the file (`test_dirs`, `timings`, `test_timings`,
  `log_dir`, `[batch] dirs`) is relative to the project.
- Paths given on the command line (`--timings`, `--timings-from`,
  `--test-timings`, `--log-dir`) are relative to the current directory and
  override the config's.

## Top level

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `project` | path | `"."` | Folder holding `project.godot`. Godot runs from here. |
| `godot` | string | `"godot"` | The Godot binary: a name on `PATH` or a path. |
| `godot_args` | list of strings | `["--headless"]` | Arguments before `-s <gut_cmdln>`, for example `["--headless", "--fixed-fps", "60"]`. |
| `gut_cmdln` | string | `"addons/gut/gut_cmdln.gd"` | GUT's command-line script. |
| `gut_args` | list of strings | `[]` | Extra GUT options, for example `["-glog=1"]`. |
| `reporter` | `res://` path | required | Where the vendored `gdtest_reporter.gd` lives. Must start with `res://`. |
| `pre_run_script` | `res://` path | none | The project's own GUT pre-run hook. The reporter runs it after attaching. |
| `test_dirs` | list of paths | required, not empty | Folders searched recursively for test scripts. |
| `script_prefix` | string | `"test_"` | File-name prefix of a test script (the suffix is always `.gd`). |
| `test_prefix` | string | `"test_"` | Name prefix of a test function. |
| `inner_class_prefix` | string | `"Test"` | Name prefix of an inner test class. |
| `workers` | integer, at least 1 | `6` | Parallel Godot processes. `--workers` overrides it. |
| `script_timeout` | seconds, at least 1 | `600` | How long one script may run. In a batch the clock restarts at each script's first event. `--timeout` overrides it. |
| `timings` | path | none | Script timings file, read for the queue order and rewritten after the run. |
| `test_timings` | path | none | Per-test timings file, written after the run. |
| `log_dir` | path | `".gdtest/logs"` | One log per script (and one per batch). The previous run's `*.log` files in it are removed at the start. |

## `[env]`

Extra environment variables for every Godot process, as `NAME = "value"`
pairs.

```toml
[env]
MY_GAME_TEST_MODE = "1"
```

## `[isolation]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `data_dir_var` | string | none | This variable gets a fresh, empty temp dir per process, removed after it exits. With `"XDG_DATA_HOME"`, a Linux Godot puts `user://` there. |
| `port_base_var` | string | none | This variable gets the worker slot's port base. |
| `port_base` | integer | `20000` | Port base of slot 0. |
| `port_stride` | integer | `10` | Ports between two slots: slot `n` gets `port_base + n * port_stride`. |

Slots are numbered from 0 to `workers - 1`, and a slot keeps its number for
the whole run, so two processes running at the same time never share a
port base.

## `[batch]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `dirs` | list of paths | `[]` | Scripts under these folders may share a process (`--mode auto`). |
| `max_scripts` | integer, at least 1 | `20` | At most this many scripts per process. |
| `max_seconds` | number, 0 or more | `0.0` | At most this many seconds of previous timings per process. A script with no timing counts 0. `0` means no budget. A batch always takes at least one script. |

`--mode script` ignores `dirs` and runs every script alone; `--mode batch`
treats every script as batchable.

## The command each process runs

```text
<godot> <godot_args> -s <gut_cmdln> -gconfig= -gexit <gut_args>
        -gpre_run_script=<reporter> -gtest=res://<script>[,res://<script>...]
```

`-gconfig=` keeps a `.gutconfig.json` in the project from adding its own
test dirs. The process runs from `project`, in its own process group, with
`GDTEST_PRE_RUN` set to `pre_run_script` when there is one.

## A complete example

See [`examples/gdtest.toml`](../examples/gdtest.toml).

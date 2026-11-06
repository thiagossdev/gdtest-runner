//! gdtest-runner command line. See README.md.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gdtest_runner::config::Config;
use gdtest_runner::discover::{Naming, ScriptInfo, discover_dirs, discover_files};
use gdtest_runner::events::Event;
use gdtest_runner::run::{Batching, ScriptResult, run_all};
use gdtest_runner::summary::{RunReport, display_path};
use gdtest_runner::timings::{
    ScriptTiming, TestTiming, queue_order, read_timings, write_script_timings, write_test_timings,
};
use gdtest_runner::ui::{Plain, Progress, Tui, Ui, terminal_width};
use gdtest_runner::worker::{RunSpec, STOP, absolute};

const USAGE: &str = "usage: gdtest-runner [--config FILE] [--workers N] [--timeout SECONDS]
                     [--mode auto|script|batch] [--plain | --tui] [--warnings]
                     [--timings FILE] [--timings-from FILE]
                     [--test-timings FILE] [--log-dir DIR] [--list] [SCRIPT ...]

Runs every test script under the config's test_dirs (or only the named
SCRIPTs, project-relative or res:// paths) on a pool of workers, one fresh
Godot process per script, or per batch of scripts for the scripts under
the config's [batch] dirs (--mode auto, the default); --mode script runs
every script alone, --mode batch batches every script. --timeout is per
script, also inside a batch. --config defaults to ./gdtest.toml. The
--timings, --test-timings and --log-dir paths are relative to the current
directory and override the config's (which are relative to the project).
--timings is read for the queue order and rewritten at the end;
--timings-from is a read-only second source for scripts --timings has no
row for (a fresh checkout, a scoped run).
The Run Summary always has the Warnings and Orphans totals and names each
script with orphans; --warnings also lists the scripts with warnings.
Exit status: 0 all passed, 1 any failure, crash, timeout, mismatch or
fewer scripts run than discovered, 2 a usage or config error, 130
interrupted.";

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    workers: Option<usize>,
    timeout: Option<u64>,
    plain: bool,
    tui: bool,
    warnings: bool,
    timings: Option<PathBuf>,
    timings_from: Option<PathBuf>,
    test_timings: Option<PathBuf>,
    log_dir: Option<PathBuf>,
    list: bool,
    mode: Mode,
    scripts: Vec<String>,
}

/// Which scripts share a process.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The config's [batch] dirs.
    #[default]
    Auto,
    Script,
    Batch,
}

fn parse_args(raw: Vec<String>) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--version" => {
                println!("gdtest-runner {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--config" => a.config = Some(PathBuf::from(value("--config")?)),
            "--workers" => {
                let v = value("--workers")?;
                a.workers = Some(
                    v.parse()
                        .map_err(|_| format!("--workers {v}: not a number"))?,
                );
            }
            "--timeout" => {
                let v = value("--timeout")?;
                a.timeout = Some(
                    v.parse()
                        .map_err(|_| format!("--timeout {v}: not a number"))?,
                );
            }
            "--mode" => {
                a.mode = match value("--mode")?.as_str() {
                    "auto" => Mode::Auto,
                    "script" => Mode::Script,
                    "batch" => Mode::Batch,
                    v => return Err(format!("--mode {v}: auto, script or batch")),
                }
            }
            "--plain" => a.plain = true,
            "--tui" => a.tui = true,
            "--warnings" => a.warnings = true,
            "--timings" => a.timings = Some(PathBuf::from(value("--timings")?)),
            "--timings-from" => a.timings_from = Some(PathBuf::from(value("--timings-from")?)),
            "--test-timings" => a.test_timings = Some(PathBuf::from(value("--test-timings")?)),
            "--log-dir" => a.log_dir = Some(PathBuf::from(value("--log-dir")?)),
            "--list" => a.list = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.scripts.push(s.to_string()),
        }
    }
    if a.plain && a.tui {
        return Err("--plain and --tui exclude each other".to_string());
    }
    Ok(Some(a))
}

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    // SAFETY: the handler only stores to an atomic, which is signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1).collect()) {
        Ok(Some(a)) => a,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("gdtest-runner: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match real_main(args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("gdtest-runner: {e}");
            ExitCode::from(2)
        }
    }
}

fn real_main(args: Args) -> Result<u8, String> {
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("gdtest.toml"));
    let config = Config::load(&config_path)?;
    let naming = Naming::from_config(&config);
    let scripts: Vec<ScriptInfo> = if args.scripts.is_empty() {
        discover_dirs(&config.project, &config.test_dirs, &naming)?
    } else {
        discover_files(&config.project, &args.scripts, &naming)?
    };
    let discovered_tests: usize = scripts.iter().map(ScriptInfo::test_count).sum();
    if args.list {
        for s in &scripts {
            println!("{}\t{}", s.path, s.test_count());
        }
        println!("{} scripts, {} tests", scripts.len(), discovered_tests);
        return Ok(0);
    }
    if scripts.is_empty() {
        return Err("no test scripts found".to_string());
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let from_cli = |p: &Option<PathBuf>| p.as_ref().map(|p| cwd.join(p));
    let from_config = |p: &Option<String>| p.as_ref().map(|p| absolute(&config.project, p));
    let timings_path = from_cli(&args.timings).or_else(|| from_config(&config.timings));
    let test_timings_path =
        from_cli(&args.test_timings).or_else(|| from_config(&config.test_timings));
    let log_dir =
        from_cli(&args.log_dir).unwrap_or_else(|| absolute(&config.project, &config.log_dir));
    let workers = args.workers.unwrap_or(config.workers).max(1);
    let timeout = Duration::from_secs(args.timeout.unwrap_or(config.script_timeout).max(1));

    let old_timings = match &timings_path {
        Some(p) => read_timings(p)?,
        None => Default::default(),
    };
    let mut queue_timings = match from_cli(&args.timings_from) {
        Some(p) => read_timings(&p)?,
        None => Default::default(),
    };
    queue_timings.extend(old_timings.iter().map(|(k, v)| (k.clone(), v.clone())));
    let order = queue_order(&scripts, &queue_timings);
    clear_logs(&log_dir)?;
    let spec = RunSpec::from_config(&config, log_dir.clone(), timeout);

    let use_tui = args.tui || (!args.plain && std::io::stdout().is_terminal() && !dumb_term());
    let workers = workers.min(scripts.len());
    let progress = Progress::new(scripts.len(), discovered_tests, workers);
    let batching = batching(&config, args.mode, &scripts, &queue_timings);
    let processes = if batching.batchable.is_empty() {
        format!("one {} process per script", config.godot)
    } else {
        let budget = if batching.max_seconds > 0.0 {
            format!(" and {} s of timings", batching.max_seconds)
        } else {
            String::new()
        };
        format!(
            "{} of them in batches of up to {} scripts{budget} per {} process, the rest one per process",
            batching.batchable.len(),
            batching.max_scripts,
            config.godot
        )
    };
    println!(
        "gdtest-runner {}: {} scripts, {} tests discovered; {} workers, {processes}, timeout {} s per script; logs in {}",
        env!("CARGO_PKG_VERSION"),
        scripts.len(),
        discovered_tests,
        workers,
        timeout.as_secs(),
        display_path(&log_dir)
    );
    install_signal_handlers();
    let start = Instant::now();
    let stdout = std::io::stdout();
    let mut ui: Box<dyn Ui> = if use_tui {
        Box::new(Tui::new(
            stdout.lock(),
            progress,
            terminal_width().unwrap_or(100),
        ))
    } else {
        Box::new(Plain::new(stdout.lock(), progress))
    };
    let results = {
        let ui_cell = std::cell::RefCell::new(&mut ui);
        run_all(
            &spec,
            &scripts,
            order,
            workers,
            &batching,
            &mut |m| ui_cell.borrow_mut().handle(m),
            &mut || ui_cell.borrow_mut().tick(),
        )
    };
    ui.finish();
    drop(ui);
    let wall = start.elapsed().as_secs_f64();
    let interrupted = STOP.load(Ordering::SeqCst);

    let report = RunReport {
        results: &results,
        discovered_scripts: scripts.len(),
        discovered_tests,
        workers,
        wall_seconds: wall,
        interrupted,
        warnings_by_script: args.warnings,
    };
    print!("{}", report.render());

    let mut write_error = None;
    if let Some(p) = &timings_path {
        let existing: Vec<String> = all_existing(&config, &naming, &scripts);
        if let Err(e) = write_script_timings(p, &script_rows(&results), &old_timings, &existing) {
            write_error = Some(e);
        }
    }
    if let Some(p) = &test_timings_path
        && let Err(e) = write_test_timings(p, &test_rows(&results))
    {
        write_error = Some(e);
    }
    if let Some(e) = write_error {
        eprintln!("gdtest-runner: {e}");
        return Ok(1);
    }
    if interrupted {
        return Ok(130);
    }
    Ok(if report.ok() { 0 } else { 1 })
}

/// The scripts that may share a process (by `--mode` and the config's
/// [batch] dirs) and the batch limits.
fn batching(
    config: &Config,
    mode: Mode,
    scripts: &[ScriptInfo],
    timings: &HashMap<String, ScriptTiming>,
) -> Batching {
    let in_dirs = |path: &str| {
        config.batch.dirs.iter().any(|d| {
            let d = d.trim_end_matches('/');
            path.strip_prefix(d)
                .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let batchable = scripts
        .iter()
        .map(|s| s.path.clone())
        .filter(|p| match mode {
            Mode::Script => false,
            Mode::Batch => true,
            Mode::Auto => in_dirs(p),
        })
        .collect();
    Batching {
        batchable,
        max_scripts: config.batch.max_scripts,
        max_seconds: config.batch.max_seconds,
        seconds: timings
            .iter()
            .map(|(k, v)| (k.clone(), v.seconds))
            .collect(),
    }
}

fn dumb_term() -> bool {
    std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false)
}

/// Removes the previous run's *.log files (only those) from the log dir.
fn clear_logs(dir: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "log") {
            std::fs::remove_file(&p).map_err(|e| format!("cannot remove {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

/// The scripts that still exist in the test dirs (a partial run keeps their
/// old timings); for a full run, the discovered ones.
fn all_existing(config: &Config, naming: &Naming, ran: &[ScriptInfo]) -> Vec<String> {
    match discover_dirs(&config.project, &config.test_dirs, naming) {
        Ok(all) => all.into_iter().map(|s| s.path).collect(),
        Err(_) => ran.iter().map(|s| s.path.clone()).collect(),
    }
}

fn script_rows(results: &[ScriptResult]) -> Vec<ScriptTiming> {
    results
        .iter()
        .map(|r| {
            let mut seconds = 0.0;
            let mut ends = 0;
            for e in &r.run.events {
                if let Event::ScriptEnd(s) = e {
                    seconds += s.seconds;
                    ends += 1;
                }
            }
            if ends == 0 {
                seconds = r.run.seconds;
            }
            ScriptTiming {
                script: r.script.clone(),
                seconds,
                tests: r.verdict.counts.tests,
                asserts: r.verdict.counts.asserts,
            }
        })
        .collect()
}

fn test_rows(results: &[ScriptResult]) -> Vec<TestTiming> {
    let mut rows = Vec::new();
    for r in results {
        for e in &r.run.events {
            if let Event::TestEnd(t) = e {
                rows.push(TestTiming {
                    script: r.script.clone(),
                    test: if t.inner.is_empty() {
                        t.test.clone()
                    } else {
                        format!("{}.{}", t.inner, t.test)
                    },
                    seconds: t.seconds,
                });
            }
        }
    }
    rows
}

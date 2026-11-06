//! One Godot process per test script, or per batch of scripts.
//!
//! The process runs in its own process group (a timeout kills the group, so
//! a child it started cannot keep the pipes open), from the project dir,
//! with a fresh data dir and the worker slot's port base in the configured
//! environment variables. Its stdout and stderr go line by line to the
//! script's log (a batch: to the batch's log, then split per script);
//! reporter events on stdout are handed to the caller as they arrive.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub use crate::batch::ScriptMark;
use crate::batch::event_script;
use crate::config::{Config, Isolation};
use crate::events::{Event, LogNotes, parse_event_line, parse_log_notes, parse_summary};
use crate::verdict::{ExitKind, ProcessRun};

/// Everything a worker needs to start one script's process.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub godot: String,
    pub project: PathBuf,
    pub godot_args: Vec<String>,
    pub gut_cmdln: String,
    pub gut_args: Vec<String>,
    pub reporter: String,
    pub pre_run_script: Option<String>,
    pub env: BTreeMap<String, String>,
    pub isolation: Isolation,
    /// Per script: in a batch it restarts at each script's first event.
    pub timeout: Duration,
    /// Absolute.
    pub log_dir: PathBuf,
}

impl RunSpec {
    pub fn from_config(config: &Config, log_dir: PathBuf, timeout: Duration) -> RunSpec {
        RunSpec {
            godot: config.godot.clone(),
            project: config.project.clone(),
            godot_args: config.godot_args.clone(),
            gut_cmdln: config.gut_cmdln.clone(),
            gut_args: config.gut_args.clone(),
            reporter: config.reporter.clone(),
            pre_run_script: config.pre_run_script.clone(),
            env: config.env.clone(),
            isolation: config.isolation.clone(),
            timeout,
            log_dir,
        }
    }
}

/// The env variable the reporter reads to chain the project's pre-run hook.
pub const PRE_RUN_ENV: &str = "GDTEST_PRE_RUN";

/// Grace period for the pipes to close after the process exited.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// The full argument list after the Godot binary.
pub fn command_args(spec: &RunSpec, script: &str) -> Vec<String> {
    command_args_many(spec, &[script.to_string()])
}

/// The argument list for a process over several scripts, run in that order
/// (GUT's `-gtest` takes a comma-separated list).
pub fn command_args_many(spec: &RunSpec, scripts: &[String]) -> Vec<String> {
    let mut args = spec.godot_args.clone();
    args.push("-s".to_string());
    args.push(spec.gut_cmdln.clone());
    args.push("-gconfig=".to_string());
    args.push("-gexit".to_string());
    args.extend(spec.gut_args.iter().cloned());
    args.push(format!("-gpre_run_script={}", spec.reporter));
    let list: Vec<String> = scripts.iter().map(|s| format!("res://{s}")).collect();
    args.push(format!("-gtest={}", list.join(",")));
    args
}

/// The log of one script: its project path with `/` turned into `__`.
pub fn log_path(spec: &RunSpec, script: &str) -> PathBuf {
    let name = script.trim_end_matches(".gd").replace('/', "__");
    spec.log_dir.join(format!("{name}.log"))
}

static DATA_DIR_SEQ: AtomicU64 = AtomicU64::new(0);
static BATCH_SEQ: AtomicU64 = AtomicU64::new(0);

/// Set on SIGINT/SIGTERM: running processes are killed, no new one starts.
pub static STOP: AtomicBool = AtomicBool::new(false);

fn fresh_data_dir() -> std::io::Result<PathBuf> {
    let n = DATA_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gdtest-data-{}-{n}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn kill_group(child: &Child) {
    let pid = child.id() as i32;
    // SAFETY: kill(2) on our own child's process group; no memory involved.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn reader<R: Read + Send + 'static>(
    stream: R,
    is_stdout: bool,
    tx: mpsc::Sender<(bool, String)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut r = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();
                    if tx.send((is_stdout, line)).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// Runs one script in its own process and returns what it left behind.
/// `slot` is the worker's index (its port base); `on_event` sees every
/// reporter event as it arrives.
pub fn run_script(
    spec: &RunSpec,
    slot: usize,
    script: &str,
    on_event: &mut dyn FnMut(&Event),
) -> ProcessRun {
    let scripts = [script.to_string()];
    let out = run_process(spec, slot, &scripts, &log_path(spec, script), on_event);
    if let Some(mut log) = out.log {
        let _ = writeln!(
            log,
            "gdtest-runner: {script}: {} after {:.1} s",
            out.run.exit, out.run.seconds
        );
        let _ = log.flush();
    }
    out.run
}

/// The whole log of one batch's process (a fresh name per batch).
fn batch_log_path(spec: &RunSpec, first: &str) -> PathBuf {
    let n = BATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = first.trim_end_matches(".gd").replace('/', "__");
    spec.log_dir.join(format!("batch-{n:03}__{name}.log"))
}

/// Runs several scripts in one process (GUT's `-gtest=a,b,...`, in that
/// order). The whole output goes to a batch log; each script also gets its
/// own log at `log_path` with its share of the lines (the first script's
/// starts with the engine start, the last script's ends with GUT's summary)
/// and a pointer to the batch log; a script that never started gets a log
/// that says so. Returns the run and where each script started.
pub fn run_batch(
    spec: &RunSpec,
    slot: usize,
    scripts: &[String],
    on_event: &mut dyn FnMut(&Event),
) -> (ProcessRun, Vec<ScriptMark>) {
    let first = scripts.first().map(String::as_str).unwrap_or("empty");
    let batch_log = batch_log_path(spec, first);
    let out = run_process(spec, slot, scripts, &batch_log, on_event);
    let mut lines = out.lines;
    let last = format!(
        "gdtest-runner: batch of {} scripts ({}): {} after {:.1} s",
        scripts.len(),
        scripts.join(", "),
        out.run.exit,
        out.run.seconds
    );
    if let Some(mut log) = out.log {
        let _ = writeln!(log, "{last}");
        let _ = log.flush();
    }
    lines.push(last);
    let pointer = format!(
        "gdtest-runner: ran in a batch, whole log: {}",
        batch_log.display()
    );
    let marks: Vec<&ScriptMark> = out
        .marks
        .iter()
        .filter(|m| scripts.contains(&m.script))
        .collect();
    for script in scripts {
        let text = match marks.iter().position(|m| &m.script == script) {
            Some(i) => {
                let from = if i == 0 { 0 } else { marks[i].line };
                let to = marks.get(i + 1).map_or(lines.len(), |m| m.line);
                let mut t = lines[from..to].join("\n");
                t.push('\n');
                t
            }
            None => format!("gdtest-runner: {script} did not start in its batch's process\n"),
        };
        let _ = std::fs::write(log_path(spec, script), format!("{text}{pointer}\n"));
    }
    (out.run, out.marks)
}

struct ProcessOutput {
    run: ProcessRun,
    marks: Vec<ScriptMark>,
    /// Every output line, stdout and stderr in arrival order.
    lines: Vec<String>,
    /// Still open, for the caller's last line; None if it could not open.
    log: Option<BufWriter<File>>,
}

/// One Godot process over `scripts`, its output in `log_file`. The timeout
/// counts per script: it restarts when a script's first event arrives.
fn run_process(
    spec: &RunSpec,
    slot: usize,
    scripts: &[String],
    log_file: &Path,
    on_event: &mut dyn FnMut(&Event),
) -> ProcessOutput {
    let start = Instant::now();
    let failed = |e: String, log: Option<BufWriter<File>>| ProcessOutput {
        run: ProcessRun {
            exit: ExitKind::SpawnFailed(e),
            events: Vec::new(),
            protocol_errors: Vec::new(),
            summary: None,
            notes: LogNotes::default(),
            seconds: start.elapsed().as_secs_f64(),
        },
        marks: Vec::new(),
        lines: Vec::new(),
        log,
    };
    if let Err(e) = std::fs::create_dir_all(&spec.log_dir) {
        return failed(
            format!("cannot create {}: {e}", spec.log_dir.display()),
            None,
        );
    }
    let mut log = match File::create(log_file) {
        Ok(f) => BufWriter::new(f),
        Err(e) => return failed(format!("cannot write {}: {e}", log_file.display()), None),
    };

    let mut cmd = Command::new(&spec.godot);
    cmd.args(command_args_many(spec, scripts))
        .current_dir(&spec.project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(pre) = &spec.pre_run_script {
        cmd.env(PRE_RUN_ENV, pre);
    }
    if let Some(var) = &spec.isolation.port_base_var {
        let port = spec.isolation.port_base + slot as u32 * spec.isolation.port_stride;
        cmd.env(var, port.to_string());
    }
    let data_dir = match &spec.isolation.data_dir_var {
        Some(var) => match fresh_data_dir() {
            Ok(dir) => {
                cmd.env(var, &dir);
                Some(dir)
            }
            Err(e) => return failed(format!("cannot create a data dir: {e}"), Some(log)),
        },
        None => None,
    };

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            if let Some(dir) = &data_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            let _ = writeln!(log, "gdtest-runner: could not start {}: {e}", spec.godot);
            return failed(format!("{}: {e}", spec.godot), Some(log));
        }
    };
    let (tx, rx) = mpsc::channel();
    let out = reader(child.stdout.take().expect("piped"), true, tx.clone());
    let err = reader(child.stderr.take().expect("piped"), false, tx);

    let mut deadline = start + spec.timeout;
    let mut timed_out = false;
    let mut exited_at: Option<Instant> = None;
    let mut grace_kill = false;
    let mut interrupted = false;
    let mut events = Vec::new();
    let mut protocol_errors = Vec::new();
    let mut stdout_text = String::new();
    let mut lines: Vec<String> = Vec::new();
    let mut marks: Vec<ScriptMark> = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((is_stdout, line)) => {
                let _ = writeln!(log, "{line}");
                if is_stdout {
                    stdout_text.push_str(&line);
                    stdout_text.push('\n');
                    match parse_event_line(&line) {
                        Some(Ok(event)) => {
                            if let Some(s) = event_script(&event)
                                && !marks.iter().any(|m| m.script == s)
                            {
                                let now = Instant::now();
                                marks.push(ScriptMark {
                                    script: s.to_string(),
                                    line: lines.len(),
                                    at: now.duration_since(start).as_secs_f64(),
                                });
                                // A new script: its own timeout starts now.
                                deadline = now + spec.timeout;
                            }
                            on_event(&event);
                            events.push(event);
                        }
                        Some(Err(e)) => protocol_errors.push(e),
                        None => {}
                    }
                }
                lines.push(line);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if !timed_out && exited_at.is_none() && now >= deadline {
            timed_out = true;
            kill_group(&child);
        }
        if !interrupted && exited_at.is_none() && STOP.load(Ordering::SeqCst) {
            interrupted = true;
            kill_group(&child);
        }
        if exited_at.is_none() && matches!(child.try_wait(), Ok(Some(_))) {
            exited_at = Some(now);
        }
        if let Some(t) = exited_at
            && !grace_kill
            && now.duration_since(t) > PIPE_GRACE
        {
            // Something the process started still holds the pipes.
            grace_kill = true;
            kill_group(&child);
        }
    }
    let _ = out.join();
    let _ = err.join();
    let status = child.wait();
    let seconds = start.elapsed().as_secs_f64();
    if let Some(dir) = &data_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
    let exit = if timed_out {
        ExitKind::TimedOut
    } else {
        match status {
            Ok(s) => match (s.code(), s.signal()) {
                (Some(c), _) => ExitKind::Code(c),
                (None, Some(sig)) => ExitKind::Signal(sig),
                _ => ExitKind::Code(-1),
            },
            Err(e) => ExitKind::SpawnFailed(format!("wait failed: {e}")),
        }
    };
    ProcessOutput {
        run: ProcessRun {
            exit,
            events,
            protocol_errors,
            summary: parse_summary(&stdout_text),
            notes: parse_log_notes(&stdout_text),
            seconds,
        },
        marks,
        lines,
        log: Some(log),
    }
}

/// Makes a path absolute against `base` unless it already is.
pub fn absolute(base: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

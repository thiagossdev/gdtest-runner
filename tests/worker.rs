use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gdtest_runner::config::Isolation;
use gdtest_runner::events::Event;
use gdtest_runner::verdict::ExitKind;
use gdtest_runner::worker::{RunSpec, log_path, run_batch, run_script};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn spec(name: &str, timeout: u64) -> RunSpec {
    let log_dir = std::env::temp_dir().join(format!("gdtest-worker-{}-{name}", std::process::id()));
    RunSpec {
        godot: root()
            .join("tests/stubs/godot_stub.sh")
            .display()
            .to_string(),
        project: root().join("tests/fixtures/stubproject"),
        godot_args: vec![
            "--headless".to_string(),
            "--fixed-fps".to_string(),
            "60".to_string(),
        ],
        gut_cmdln: "addons/gut/gut_cmdln.gd".to_string(),
        gut_args: vec!["-glog=1".to_string()],
        reporter: "res://tools/gdtest_reporter.gd".to_string(),
        pre_run_script: Some("res://tests/pre_run.gd".to_string()),
        env: [("EXTRA".to_string(), "yes".to_string())]
            .into_iter()
            .collect(),
        isolation: Isolation {
            data_dir_var: Some("XDG_DATA_HOME".to_string()),
            port_base_var: Some("GUT_PORT_BASE".to_string()),
            port_base: 20000,
            port_stride: 10,
        },
        timeout: Duration::from_secs(timeout),
        log_dir,
    }
}

fn log_of(spec: &RunSpec, script: &str) -> String {
    std::fs::read_to_string(log_path(spec, script)).unwrap()
}

#[test]
fn a_passing_script_streams_events_and_writes_its_log() {
    let spec = spec("pass", 30);
    let mut seen = Vec::new();
    let run = run_script(&spec, 2, "tests/unit/test_pass_a.gd", &mut |e: &Event| {
        seen.push(e.clone())
    });
    assert_eq!(run.exit, ExitKind::Code(0));
    assert!(run.summary.is_some());
    assert!(run.protocol_errors.is_empty());
    assert_eq!(seen.len(), run.events.len());
    assert!(matches!(seen.last(), Some(Event::RunEnd)));
    let log = log_of(&spec, "tests/unit/test_pass_a.gd");
    assert!(log.contains("= Run Summary"));
    // The command line: godot args, gut_cmdln, the fixed GUT options.
    assert!(log.contains(
        "STUB args: --headless --fixed-fps 60 -s addons/gut/gut_cmdln.gd -gconfig= -gexit -glog=1 -gpre_run_script=res://tools/gdtest_reporter.gd -gtest=res://tests/unit/test_pass_a.gd"
    ), "{log}");
}

#[test]
fn each_process_gets_its_own_data_dir_port_and_env_and_the_dir_is_removed() {
    let spec = spec("env", 30);
    let run = run_script(&spec, 3, "tests/unit/test_pass_b.gd", &mut |_| {});
    assert_eq!(run.exit, ExitKind::Code(0));
    let log = log_of(&spec, "tests/unit/test_pass_b.gd");
    assert!(log.contains("port=20030"), "{log}");
    assert!(log.contains("pre=res://tests/pre_run.gd"), "{log}");
    assert!(log.contains("extra=yes"), "{log}");
    assert!(log.contains("STUB data dir exists"), "{log}");
    let data = log
        .split("data=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert!(!Path::new(data).exists(), "data dir {data} was not removed");
    let cwd = log.split("cwd=").nth(1).unwrap().lines().next().unwrap();
    assert_eq!(
        Path::new(cwd).canonicalize().unwrap(),
        spec.project.canonicalize().unwrap()
    );
}

#[test]
fn a_crash_has_no_summary_and_a_signal() {
    let spec = spec("crash", 30);
    let run = run_script(&spec, 0, "tests/unit/test_crash.gd", &mut |_| {});
    assert_eq!(run.exit, ExitKind::Signal(11));
    assert!(run.summary.is_none());
}

#[test]
fn a_hang_is_killed_at_the_timeout_with_its_children() {
    let spec = spec("hang", 1);
    let start = Instant::now();
    let run = run_script(&spec, 0, "tests/unit/test_hang.gd", &mut |_| {});
    assert_eq!(run.exit, ExitKind::TimedOut);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

const BATCH: [&str; 3] = [
    "tests/unit/test_pass_a.gd",
    "tests/unit/test_pass_b.gd",
    "tests/unit/test_pass_c.gd",
];

fn batch() -> Vec<String> {
    BATCH.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_batch_runs_in_one_process_with_a_log_per_script() {
    let spec = spec("batch", 30);
    let mut seen = 0;
    let (run, marks) = run_batch(&spec, 1, &batch(), &mut |_| seen += 1);
    assert_eq!(run.exit, ExitKind::Code(0));
    assert_eq!(run.summary.unwrap().scripts, 3);
    assert_eq!(seen, run.events.len());
    let order: Vec<&str> = marks.iter().map(|m| m.script.as_str()).collect();
    assert_eq!(order, BATCH);
    assert!(marks[0].line < marks[1].line && marks[1].line < marks[2].line);
    assert!(marks[0].at <= marks[1].at && marks[1].at <= marks[2].at);
    // One process: the first script's log has the command line with all
    // three scripts, in order; no other log has a command line.
    let a = log_of(&spec, BATCH[0]);
    assert!(
        a.contains(
            "-gtest=res://tests/unit/test_pass_a.gd,res://tests/unit/test_pass_b.gd,res://tests/unit/test_pass_c.gd"
        ),
        "{a}"
    );
    let b = log_of(&spec, BATCH[1]);
    let c = log_of(&spec, BATCH[2]);
    assert!(
        !b.contains("STUB args") && !c.contains("STUB args"),
        "{b}{c}"
    );
    assert!(b.contains("res://tests/unit/test_pass_b.gd"), "{b}");
    assert!(
        !b.contains("res://tests/unit/test_pass_a.gd\",\"inner"),
        "{b}"
    );
    assert!(c.contains("= Run Summary"), "{c}");
    // Each points at the batch's whole log, which exists.
    let batch_log = b
        .lines()
        .find_map(|l| l.strip_prefix("gdtest-runner: ran in a batch, whole log: "))
        .unwrap_or_else(|| panic!("no batch log line in {b}"));
    let whole = std::fs::read_to_string(batch_log).unwrap();
    assert_eq!(whole.matches("STUB args").count(), 1);
    assert!(whole.contains("test_pass_c"));
}

#[test]
fn the_timeout_is_per_script_inside_a_batch() {
    let mut spec = spec("batch-timeout", 1);
    spec.env
        .insert("STUB_SCRIPT_DELAY".to_string(), "0.6".to_string());
    let (run, _) = run_batch(&spec, 0, &batch(), &mut |_| {});
    // 1.8 s in all, each script under 1 s: no timeout.
    assert_eq!(run.exit, ExitKind::Code(0), "{:?}", run.exit);
}

#[test]
fn a_hang_inside_a_batch_is_a_timeout_after_the_scripts_before_it() {
    let spec = spec("batch-hang", 1);
    let scripts: Vec<String> = ["tests/unit/test_pass_a.gd", "tests/unit/test_hang.gd"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (run, marks) = run_batch(&spec, 0, &scripts, &mut |_| {});
    assert_eq!(run.exit, ExitKind::TimedOut);
    assert_eq!(marks.len(), 2);
    assert!(log_of(&spec, "tests/unit/test_hang.gd").contains("timed out"));
}

#[test]
fn a_missing_binary_is_a_spawn_failure() {
    let mut spec = spec("missing", 5);
    spec.godot = "/nonexistent/godot".to_string();
    let run = run_script(&spec, 0, "tests/unit/test_pass_a.gd", &mut |_| {});
    assert!(
        matches!(run.exit, ExitKind::SpawnFailed(_)),
        "{:?}",
        run.exit
    );
}

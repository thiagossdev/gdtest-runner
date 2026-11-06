use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// A temp dir holding a config that points at the stub project and stub
/// Godot; logs and timings go into the temp dir.
fn setup(name: &str, timeout: u64) -> PathBuf {
    setup_with(name, timeout, "")
}

/// `setup` with extra config lines at the end (a `[batch]` table).
fn setup_with(name: &str, timeout: u64, extra: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gdtest-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = format!(
        r#"project = "{project}"
godot = "{godot}"
godot_args = ["--headless"]
reporter = "res://tools/gdtest_reporter.gd"
test_dirs = ["tests/unit"]
workers = 3
script_timeout = {timeout}
timings = "{dir}/timings.tsv"
test_timings = "{dir}/test_timings.tsv"
log_dir = "{dir}/logs"

[isolation]
data_dir_var = "XDG_DATA_HOME"
port_base_var = "GUT_PORT_BASE"
{extra}"#,
        project = root().join("tests/fixtures/stubproject").display(),
        godot = root().join("tests/stubs/godot_stub.sh").display(),
        dir = dir.display(),
    );
    std::fs::write(dir.join("gdtest.toml"), config).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gdtest-runner"));
    cmd.arg("--config").arg(dir.join("gdtest.toml")).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

const PASSING: [&str; 3] = [
    "tests/unit/test_pass_a.gd",
    "tests/unit/test_pass_b.gd",
    "tests/unit/test_pass_c.gd",
];

#[test]
fn a_green_run_exits_0_with_a_gut_style_summary_and_timings() {
    let dir = setup("green", 30);
    let o = run(&dir, &PASSING, &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(
        !out.contains('\x1b'),
        "plain output has escape codes: {out:?}"
    );
    let block = out.split("= Run Summary").nth(1).expect("summary block");
    let words: Vec<Vec<&str>> = block
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    assert!(words.contains(&vec!["Scripts", "3"]), "{block}");
    assert!(words.contains(&vec!["Tests", "6"]), "{block}");
    assert!(words.contains(&vec!["Passing", "Tests", "6"]), "{block}");
    assert!(words.contains(&vec!["Asserts", "9"]), "{block}");
    let timings = std::fs::read_to_string(dir.join("timings.tsv")).unwrap();
    assert_eq!(timings.lines().count(), 4, "{timings}");
    assert!(timings.starts_with("script\tseconds\ttests\tasserts\n"));
    let tt = std::fs::read_to_string(dir.join("test_timings.tsv")).unwrap();
    assert_eq!(tt.lines().count(), 7, "{tt}");
    assert!(dir.join("logs/tests__unit__test_pass_a.log").is_file());
}

#[test]
fn every_kind_of_failure_is_reported_and_fails_the_run() {
    let dir = setup("red", 2);
    let o = run(&dir, &[], &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    let block = out.split("= Run Summary").nth(1).expect("summary block");
    assert!(
        block.contains("FAIL tests/unit/test_fail.gd > test_two"),
        "{block}"
    );
    assert!(block.contains("expected 1 got 2"), "{block}");
    assert!(block.contains("CRASH tests/unit/test_crash.gd"), "{block}");
    assert!(block.contains("TIMEOUT tests/unit/test_hang.gd"), "{block}");
    assert!(
        block.contains("MISMATCH tests/unit/test_mismatch.gd"),
        "{block}"
    );
    assert!(
        block.contains("MISMATCH tests/unit/test_noreport.gd"),
        "{block}"
    );
    assert!(block.contains("tests__unit__test_crash.log"), "{block}");
    // The passing scripts still ran.
    assert!(
        out.contains("ok") && out.contains("test_pass_c.gd"),
        "{out}"
    );
}

#[test]
fn the_queue_runs_unknown_scripts_first_then_slowest_first() {
    let dir = setup("order", 30);
    std::fs::write(
        dir.join("timings.tsv"),
        "script\tseconds\ttests\tasserts\ntests/unit/test_pass_a.gd\t1.000\t2\t3\ntests/unit/test_pass_c.gd\t9.000\t2\t3\n",
    )
    .unwrap();
    // b has no timing: first; then c (9 s) before a (1 s).
    let o = run(
        &dir,
        &["--workers", "1", PASSING[0], PASSING[1], PASSING[2]],
        &[],
    );
    let out = stdout(&o);
    let pos = |name: &str| {
        out.lines()
            .position(|l| l.starts_with("ok") && l.contains(name))
            .unwrap_or_else(|| panic!("{name} not in {out}"))
    };
    assert!(pos("test_pass_b") < pos("test_pass_c"), "{out}");
    assert!(pos("test_pass_c") < pos("test_pass_a"), "{out}");
}

#[test]
fn timings_from_orders_the_queue_when_the_target_has_no_row() {
    let dir = setup("from", 30);
    // The target knows only a; the extra source says c is the slowest and
    // b slower than a. The target's own row wins for a.
    std::fs::write(
        dir.join("timings.tsv"),
        "script\tseconds\ttests\tasserts\ntests/unit/test_pass_a.gd\t50.000\t2\t3\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("from.tsv"),
        "script\tseconds\ttests\tasserts\ntests/unit/test_pass_a.gd\t1.000\t2\t3\ntests/unit/test_pass_b.gd\t5.000\t2\t3\ntests/unit/test_pass_c.gd\t9.000\t2\t3\n",
    )
    .unwrap();
    let from = dir.join("from.tsv");
    let o = run(
        &dir,
        &[
            "--workers",
            "1",
            "--timings-from",
            from.to_str().unwrap(),
            PASSING[0],
            PASSING[1],
            PASSING[2],
        ],
        &[],
    );
    let out = stdout(&o);
    let pos = |name: &str| {
        out.lines()
            .position(|l| l.starts_with("ok") && l.contains(name))
            .unwrap_or_else(|| panic!("{name} not in {out}"))
    };
    assert!(pos("test_pass_a") < pos("test_pass_c"), "{out}");
    assert!(pos("test_pass_c") < pos("test_pass_b"), "{out}");
    // The source is read, never written.
    assert!(
        std::fs::read_to_string(&from)
            .unwrap()
            .contains("\t9.000\t")
    );
}

#[test]
fn workers_run_in_parallel() {
    let dir = setup("parallel", 30);
    let start = Instant::now();
    let o = run(
        &dir,
        &["--workers", "3", PASSING[0], PASSING[1], PASSING[2]],
        &[("STUB_DELAY", "1")],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert!(
        start.elapsed() < Duration::from_millis(2500),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn tui_mode_draws_worker_panes_with_escape_codes() {
    let dir = setup("tui", 30);
    let o = run(
        &dir,
        &["--tui", "--workers", "2", PASSING[0], PASSING[1]],
        &[("STUB_DELAY", "0.3")],
    );
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("\x1b["), "{out:?}");
    assert!(out.contains("w1 ") && out.contains("w2 "), "{out:?}");
    assert!(out.contains("= Run Summary"), "{out:?}");
}

#[test]
fn list_prints_the_discovery_without_running() {
    let dir = setup("list", 30);
    let o = run(&dir, &["--list"], &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("tests/unit/test_pass_a.gd\t2"), "{out}");
    assert!(out.contains("8 scripts, 16 tests"), "{out}");
    assert!(!dir.join("logs").exists());
}

/// The CI integration project (tests/fixtures/gutproject) holds the counts
/// that scripts/ci_godot_integration.sh expects from a real GUT run.
#[test]
fn the_ci_gut_project_has_the_counts_the_integration_job_expects() {
    let list = |config: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_gdtest-runner"))
            .arg("--config")
            .arg(root().join("tests/fixtures/gutproject").join(config))
            .arg("--list")
            .output()
            .unwrap();
        let out = stdout(&o);
        assert_eq!(o.status.code(), Some(0), "{out}");
        out
    };
    let out = list("gdtest.toml");
    assert!(out.contains("tests/unit/test_strings.gd\t3"), "{out}");
    assert!(out.contains("4 scripts, 9 tests"), "{out}");
    let out = list("gdtest-failing.toml");
    assert!(out.contains("1 scripts, 2 tests"), "{out}");
}

#[test]
fn a_bad_config_or_script_exits_2() {
    let dir = setup("bad", 30);
    let o = Command::new(env!("CARGO_BIN_EXE_gdtest-runner"))
        .arg("--config")
        .arg(dir.join("nope.toml"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = run(&dir, &["tests/unit/test_nope.gd"], &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("test_nope.gd"));
}

// --- batch mode ---------------------------------------------------------------

const BATCH_UNIT: &str = "\n[batch]\ndirs = [\"tests/unit\"]\n";

/// The batch logs a run left (one per Godot process of a batch).
fn batch_logs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir.join("logs"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("batch-"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The queue order a..: the first is the slowest in the timings file.
fn order_timings(dir: &Path, scripts: &[&str]) {
    let mut t = "script\tseconds\ttests\tasserts\n".to_string();
    for (i, s) in scripts.iter().enumerate() {
        t += &format!("{s}\t{}.000\t2\t3\n", 100 - i);
    }
    std::fs::write(dir.join("timings.tsv"), t).unwrap();
}

fn line_of<'a>(out: &'a str, label: &str, script: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(label) && l.contains(script))
        .unwrap_or_else(|| panic!("no {label} line for {script} in {out}"))
}

#[test]
fn a_batch_runs_the_batch_dirs_scripts_in_one_process_with_per_script_results() {
    let dir = setup_with("batch-green", 30, BATCH_UNIT);
    let o = run(
        &dir,
        &["--workers", "1", PASSING[0], PASSING[1], PASSING[2]],
        &[],
    );
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert_eq!(batch_logs(&dir).len(), 1, "{:?}", batch_logs(&dir));
    for s in PASSING {
        line_of(&out, "ok", s);
    }
    assert!(out.contains("ran 3 of 3 scripts"), "{out}");
    let block = out.split("= Run Summary").nth(1).expect("summary block");
    assert!(block.contains("Tests                 6"), "{block}");
    let timings = std::fs::read_to_string(dir.join("timings.tsv")).unwrap();
    assert_eq!(timings.lines().count(), 4, "{timings}");
    let tt = std::fs::read_to_string(dir.join("test_timings.tsv")).unwrap();
    assert_eq!(tt.lines().count(), 7, "{tt}");
}

#[test]
fn a_crash_in_a_batch_marks_its_script_and_reruns_the_rest_in_a_fresh_process() {
    let dir = setup_with("batch-crash", 30, BATCH_UNIT);
    let scripts = [
        "tests/unit/test_pass_a.gd",
        "tests/unit/test_crash.gd",
        "tests/unit/test_pass_b.gd",
        "tests/unit/test_pass_c.gd",
    ];
    order_timings(&dir, &scripts);
    let mut args = vec!["--workers", "1"];
    args.extend(scripts);
    let o = run(&dir, &args, &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    line_of(&out, "ok", "test_pass_a.gd");
    line_of(&out, "CRASH", "test_crash.gd");
    line_of(&out, "ok", "test_pass_b.gd");
    line_of(&out, "ok", "test_pass_c.gd");
    assert!(out.contains("ran 4 of 4 scripts"), "{out}");
    assert_eq!(batch_logs(&dir).len(), 2, "{:?}", batch_logs(&dir));
}

#[test]
fn a_hang_in_a_batch_is_a_timeout_and_the_rest_still_run() {
    let dir = setup_with("batch-hang", 2, BATCH_UNIT);
    let scripts = [
        "tests/unit/test_pass_a.gd",
        "tests/unit/test_hang.gd",
        "tests/unit/test_pass_b.gd",
    ];
    order_timings(&dir, &scripts);
    let mut args = vec!["--workers", "1"];
    args.extend(scripts);
    let o = run(&dir, &args, &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    line_of(&out, "ok", "test_pass_a.gd");
    line_of(&out, "TIMEOUT", "test_hang.gd");
    line_of(&out, "ok", "test_pass_b.gd");
    assert!(out.contains("ran 3 of 3 scripts"), "{out}");
}

#[test]
fn a_failing_test_in_a_batch_names_its_script() {
    let dir = setup_with("batch-fail", 30, BATCH_UNIT);
    let o = run(
        &dir,
        &[
            "--workers",
            "1",
            PASSING[0],
            "tests/unit/test_fail.gd",
            PASSING[1],
        ],
        &[],
    );
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert_eq!(batch_logs(&dir).len(), 1);
    let block = out.split("= Run Summary").nth(1).expect("summary block");
    assert!(
        block.contains("FAIL tests/unit/test_fail.gd > test_two"),
        "{block}"
    );
    line_of(&out, "ok", PASSING[0]);
    line_of(&out, "ok", PASSING[1]);
}

#[test]
fn mode_script_overrides_the_batch_dirs_and_mode_batch_batches_everything() {
    let dir = setup_with("mode-script", 30, BATCH_UNIT);
    let o = run(&dir, &["--mode", "script", PASSING[0], PASSING[1]], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert!(batch_logs(&dir).is_empty(), "{:?}", batch_logs(&dir));

    let dir = setup("mode-batch", 30);
    let o = run(
        &dir,
        &["--mode", "batch", "--workers", "1", PASSING[0], PASSING[1]],
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert_eq!(batch_logs(&dir).len(), 1);

    let o = run(&dir, &["--mode", "nope"], &[]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn max_scripts_splits_the_batches() {
    let dir = setup_with(
        "batch-k",
        30,
        "\n[batch]\ndirs = [\"tests/unit\"]\nmax_scripts = 2\n",
    );
    let o = run(
        &dir,
        &["--workers", "1", PASSING[0], PASSING[1], PASSING[2]],
        &[],
    );
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    // A batch of two, then the third alone (a batch of one is a plain
    // per-script process, with no batch log).
    let logs = batch_logs(&dir);
    assert_eq!(logs.len(), 1, "{logs:?}");
    let whole = std::fs::read_to_string(dir.join("logs").join(&logs[0])).unwrap();
    assert!(whole.contains("batch of 2 scripts"), "{whole}");
    assert!(out.contains("ran 3 of 3 scripts"), "{out}");
}

// --- warnings and orphans in the Run Summary --------------------------------

const NOTES_ENV: [(&str, &str); 2] = [
    ("STUB_WARN", "test_pass_a=2,test_pass_b=1"),
    ("STUB_ORPHAN", "test_pass_c=3,test_pass_a=1"),
];

fn notes_run(name: &str, extra: &str, args: &[&str]) -> String {
    let dir = setup_with(name, 30, extra);
    let mut all = vec!["--workers", "1"];
    all.extend_from_slice(args);
    all.extend_from_slice(&PASSING);
    let o = run(&dir, &all, &NOTES_ENV);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "warnings never fail a run: {out}");
    out
}

fn totals_line<'a>(out: &'a str, label: &str) -> &'a str {
    out.lines()
        .rev()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label} line in {out}"))
}

fn check_notes(out: &str) {
    assert!(
        totals_line(out, "Warnings").trim_end().ends_with(" 3"),
        "{out}"
    );
    assert!(
        totals_line(out, "Orphans").trim_end().ends_with(" 4"),
        "{out}"
    );
    let orphans = out.split("Orphans by script").nth(1).expect(out);
    let section = orphans.split("Totals").next().unwrap();
    assert!(section.contains("test_pass_c.gd"), "{out}");
    assert!(section.contains("test_pass_a.gd"), "{out}");
    assert!(!section.contains("test_pass_b.gd"), "{out}");
    assert!(section.contains("log: "), "{out}");
    // The per-script warnings list is off by default.
    assert!(!out.contains("Warnings by script"), "{out}");
}

#[test]
fn per_script_mode_sums_warnings_and_orphans_and_names_the_orphan_scripts() {
    check_notes(&notes_run("notes-script", "", &["--mode", "script"]));
}

#[test]
fn batch_mode_attributes_warnings_and_orphans_per_script() {
    let out = notes_run(
        "notes-batch",
        "\n[batch]\ndirs = [\"tests/unit\"]\nmax_scripts = 3\n",
        &["--mode", "batch"],
    );
    check_notes(&out);
}

#[test]
fn the_warnings_flag_lists_the_scripts_with_warnings() {
    let out = notes_run("notes-flag", "", &["--mode", "script", "--warnings"]);
    let w = out.split("Warnings by script").nth(1).expect(&out);
    let section = w.split("Totals").next().unwrap();
    assert!(section.contains("test_pass_a.gd"), "{out}");
    assert!(section.contains("test_pass_b.gd"), "{out}");
    assert!(!section.contains("test_pass_c.gd"), "{out}");
}

#[test]
fn a_clean_run_prints_both_totals_as_none() {
    let dir = setup("notes-none", 30);
    let o = run(&dir, &PASSING, &[]);
    let out = stdout(&o);
    assert!(totals_line(&out, "Warnings").contains("none"), "{out}");
    assert!(totals_line(&out, "Orphans").contains("none"), "{out}");
    assert!(!out.contains("Orphans by script"), "{out}");
}

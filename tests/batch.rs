use std::collections::{HashMap, VecDeque};

use gdtest_runner::config::Config;
use gdtest_runner::run::{Batching, take_batch};

fn queue(names: &[&str]) -> VecDeque<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn batching(batchable: &[&str], max_scripts: usize, max_seconds: f64) -> Batching {
    Batching {
        batchable: batchable.iter().map(|s| s.to_string()).collect(),
        max_scripts,
        max_seconds,
        seconds: HashMap::new(),
    }
}

#[test]
fn a_batch_takes_the_next_k_batchable_scripts() {
    let mut q = queue(&["a", "b", "c", "d", "e"]);
    let b = batching(&["a", "b", "c", "d", "e"], 3, 0.0);
    assert_eq!(take_batch(&mut q, &b), vec!["a", "b", "c"]);
    assert_eq!(take_batch(&mut q, &b), vec!["d", "e"]);
    assert!(take_batch(&mut q, &b).is_empty());
}

#[test]
fn a_non_batchable_front_runs_alone_and_others_keep_their_places() {
    let mut q = queue(&["x", "a", "y", "b"]);
    let b = batching(&["a", "b"], 10, 0.0);
    assert_eq!(take_batch(&mut q, &b), vec!["x"]);
    // a, then b past y (which stays in the queue, in order).
    assert_eq!(take_batch(&mut q, &b), vec!["a", "b"]);
    assert_eq!(q, queue(&["y"]));
}

#[test]
fn the_time_budget_limits_a_batch_but_never_below_one_script() {
    let mut q = queue(&["slow", "a", "b", "c", "d"]);
    let mut b = batching(&["slow", "a", "b", "c", "d"], 10, 2.0);
    b.seconds = [("slow", 5.0), ("a", 1.0), ("b", 0.8), ("c", 0.5)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    // slow alone exceeds the budget: a batch of one.
    assert_eq!(take_batch(&mut q, &b), vec!["slow"]);
    // a + b = 1.8; c would make 2.3. d has no timing (0).
    assert_eq!(take_batch(&mut q, &b), vec!["a", "b", "d"]);
    assert_eq!(take_batch(&mut q, &b), vec!["c"]);
}

#[test]
fn no_batching_takes_one_script_at_a_time() {
    let mut q = queue(&["a", "b"]);
    assert_eq!(take_batch(&mut q, &Batching::none()), vec!["a"]);
}

fn config_with(extra: &str) -> Result<Config, String> {
    let dir = std::env::temp_dir().join(format!(
        "gdtest-batch-config-{}-{}",
        std::process::id(),
        extra.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gdtest.toml");
    std::fs::write(
        &path,
        format!("reporter = \"res://r.gd\"\ntest_dirs = [\"tests\"]\n{extra}"),
    )
    .unwrap();
    Config::load(&path)
}

#[test]
fn the_batch_table_parses_and_defaults_to_no_batch_dirs() {
    let c = config_with("").unwrap();
    assert!(c.batch.dirs.is_empty());
    assert_eq!(c.batch.max_scripts, 20);
    assert_eq!(c.batch.max_seconds, 0.0);
    let c = config_with("[batch]\ndirs = [\"tests/unit\"]\nmax_scripts = 8\nmax_seconds = 2.5\n")
        .unwrap();
    assert_eq!(c.batch.dirs, vec!["tests/unit"]);
    assert_eq!(c.batch.max_scripts, 8);
    assert_eq!(c.batch.max_seconds, 2.5);
}

#[test]
fn a_bad_batch_table_is_an_error() {
    assert!(config_with("[batch]\ndirz = []\n").is_err());
    assert!(config_with("[batch]\nmax_scripts = 0\n").is_err());
    assert!(config_with("[batch]\nmax_seconds = -1.0\n").is_err());
}

// --- judge_batch: one process, a verdict per script -------------------------

use gdtest_runner::batch::{BatchJudgement, ScriptMark, judge_batch};
use gdtest_runner::discover::{Naming, ScriptInfo, parse_script};
use gdtest_runner::events::{Event, GutSummary, LogNotes, ScriptEnd, Status, TestEnd};
use gdtest_runner::verdict::{ExitKind, Outcome, ProcessRun};

const TWO_TESTS: &str = "extends GutTest\nfunc test_one():\n\tpass\nfunc test_two():\n\tpass\n";

fn infos(names: &[&str]) -> HashMap<String, ScriptInfo> {
    names
        .iter()
        .map(|n| {
            (
                n.to_string(),
                ScriptInfo {
                    path: n.to_string(),
                    classes: parse_script(TWO_TESTS, &Naming::default()),
                },
            )
        })
        .collect()
}

/// The events of one script with tests test_one and test_two; `failing`
/// names the failing one, `stop_after` cuts the events after that many
/// (a crash or a hang inside the script).
fn script_events(script: &str, failing: Option<&str>, stop_after: Option<usize>) -> Vec<Event> {
    let res = format!("res://{script}");
    let mut evs = vec![Event::ScriptStart {
        script: res.clone(),
        inner: String::new(),
        tests: 2,
    }];
    let mut passing = 0;
    let mut failed = 0;
    for t in ["test_one", "test_two"] {
        evs.push(Event::TestStart {
            script: res.clone(),
            inner: String::new(),
            test: t.to_string(),
        });
        let fail = failing == Some(t);
        if fail {
            failed += 1;
        } else {
            passing += 1;
        }
        evs.push(Event::TestEnd(TestEnd {
            script: res.clone(),
            inner: String::new(),
            test: t.to_string(),
            status: if fail { Status::Failed } else { Status::Passed },
            asserts: 1,
            seconds: 0.01,
            messages: if fail {
                vec!["boom".to_string()]
            } else {
                vec![]
            },
        }));
    }
    evs.push(Event::ScriptEnd(ScriptEnd {
        script: res,
        inner: String::new(),
        tests: 2,
        passing,
        failing: failed,
        pending: 0,
        risky: 0,
        asserts_passed: passing,
        asserts_failed: failed,
        seconds: 0.2,
        setup_failures: vec![],
    }));
    if let Some(n) = stop_after {
        evs.truncate(n);
    }
    evs
}

fn summary_of(events: &[Event]) -> GutSummary {
    let mut s = GutSummary::default();
    for e in events {
        if let Event::ScriptEnd(x) = e {
            s.scripts += 1;
            s.tests += x.tests;
            s.passing_tests += x.passing;
            s.failing_tests += x.failing;
            s.asserts += x.asserts_passed + x.asserts_failed;
            s.failed_asserts += x.asserts_failed;
        }
    }
    s
}

/// One mark per script at its first event; script i starts at i seconds.
fn marks_of(events: &[Event]) -> Vec<ScriptMark> {
    let mut marks: Vec<ScriptMark> = Vec::new();
    for (i, e) in events.iter().enumerate() {
        if let Event::ScriptStart { script, .. } = e {
            let s = script.trim_start_matches("res://").to_string();
            if marks.last().map(|m| &m.script) != Some(&s) {
                marks.push(ScriptMark {
                    script: s,
                    line: i,
                    at: marks.len() as f64,
                });
            }
        }
    }
    marks
}

fn finished(events: Vec<Event>, exit: ExitKind) -> ProcessRun {
    let mut events = events;
    events.push(Event::RunEnd);
    ProcessRun {
        summary: Some(summary_of(&events)),
        exit,
        events,
        protocol_errors: vec![],
        notes: LogNotes::default(),
        seconds: 5.0,
    }
}

/// A finished batch whose log attributes warnings and orphans as given
/// ((script, count) pairs), and whose summary says the given totals.
fn finished_with_notes(
    events: Vec<Event>,
    warnings: &[(&str, u64)],
    orphans: &[(&str, u64)],
    totals: (u64, u64),
) -> ProcessRun {
    let mut run = finished(events, ExitKind::Code(0));
    let mut s = run.summary.unwrap();
    (s.warnings, s.orphans) = totals;
    run.summary = Some(s);
    run.notes = LogNotes {
        warnings: warnings.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
        orphans: orphans.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
    };
    run
}

#[test]
fn a_batch_gives_each_script_its_own_warnings_and_orphans() {
    let evs: Vec<Event> = ["tests/unit/test_a.gd", "tests/unit/test_b.gd"]
        .iter()
        .flat_map(|s| script_events(s, None, None))
        .collect();
    let run = finished_with_notes(
        evs.clone(),
        &[("tests/unit/test_a.gd", 2)],
        &[("test_b.gd", 4)],
        (2, 4),
    );
    let scripts = names(&["tests/unit/test_a.gd", "tests/unit/test_b.gd"]);
    let j = judge_batch(
        &scripts,
        &infos(&["tests/unit/test_a.gd", "tests/unit/test_b.gd"]),
        &run,
        &marks_of(&evs),
    );
    let counts = |s: &str| {
        j.results
            .iter()
            .find(|r| r.script == s)
            .unwrap()
            .verdict
            .counts
    };
    assert_eq!(outcome_of(&j, "tests/unit/test_a.gd"), Outcome::Passed);
    assert_eq!(counts("tests/unit/test_a.gd").warnings, 2);
    assert_eq!(counts("tests/unit/test_a.gd").orphans, 0);
    assert_eq!(counts("tests/unit/test_b.gd").warnings, 0);
    assert_eq!(counts("tests/unit/test_b.gd").orphans, 4);
}

#[test]
fn orphans_the_log_cannot_attribute_are_a_batch_mismatch_not_lost() {
    let evs: Vec<Event> = ["tests/unit/test_a.gd", "tests/unit/test_b.gd"]
        .iter()
        .flat_map(|s| script_events(s, None, None))
        .collect();
    let run = finished_with_notes(evs.clone(), &[], &[("test_b.gd", 3)], (0, 5));
    let scripts = names(&["tests/unit/test_a.gd", "tests/unit/test_b.gd"]);
    let j = judge_batch(
        &scripts,
        &infos(&["tests/unit/test_a.gd", "tests/unit/test_b.gd"]),
        &run,
        &marks_of(&evs),
    );
    assert_eq!(outcome_of(&j, "tests/unit/test_a.gd"), Outcome::Mismatch);
    let p = &j.results[0].verdict.problems;
    assert!(p.iter().any(|m| m.contains("orphans")), "{p:?}");
}

#[test]
fn warnings_the_log_cannot_attribute_are_a_batch_mismatch_not_lost() {
    let evs: Vec<Event> = script_events("tests/unit/test_a.gd", None, None);
    let run = finished_with_notes(evs.clone(), &[("tests/unit/test_a.gd", 1)], &[], (3, 0));
    let scripts = names(&["tests/unit/test_a.gd"]);
    let j = judge_batch(
        &scripts,
        &infos(&["tests/unit/test_a.gd"]),
        &run,
        &marks_of(&evs),
    );
    assert_eq!(outcome_of(&j, "tests/unit/test_a.gd"), Outcome::Mismatch);
}

#[test]
fn scripts_sharing_a_file_name_cannot_split_orphans() {
    let a = "tests/unit/test_a.gd";
    let b = "tests/other/test_a.gd";
    let evs: Vec<Event> = [a, b]
        .iter()
        .flat_map(|s| script_events(s, None, None))
        .collect();
    let run = finished_with_notes(evs.clone(), &[], &[("test_a.gd", 2)], (0, 2));
    let j = judge_batch(&names(&[a, b]), &infos(&[a, b]), &run, &marks_of(&evs));
    assert_eq!(outcome_of(&j, a), Outcome::Mismatch);
}

fn cut(events: Vec<Event>, exit: ExitKind) -> ProcessRun {
    ProcessRun {
        summary: None,
        exit,
        events,
        protocol_errors: vec![],
        notes: LogNotes::default(),
        seconds: 5.0,
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn outcome_of(j: &BatchJudgement, script: &str) -> Outcome {
    j.results
        .iter()
        .find(|r| r.script == script)
        .unwrap_or_else(|| panic!("no result for {script}"))
        .verdict
        .outcome
}

#[test]
fn a_green_batch_gives_each_script_its_own_passing_verdict_and_events() {
    let batch = names(&["a.gd", "b.gd", "c.gd"]);
    let mut evs = Vec::new();
    for s in &batch {
        evs.extend(script_events(s, None, None));
    }
    let marks = marks_of(&evs);
    let run = finished(evs, ExitKind::Code(0));
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd", "c.gd"]), &run, &marks);
    assert!(j.requeue.is_empty());
    assert_eq!(j.results.len(), 3);
    for r in &j.results {
        assert_eq!(
            r.verdict.outcome,
            Outcome::Passed,
            "{:?}",
            r.verdict.problems
        );
        assert_eq!(r.verdict.counts.tests, 2);
        assert_eq!(r.verdict.counts.asserts, 2);
        assert!(r.run.events.iter().all(|e| match e {
            Event::ScriptStart { script, .. } => *script == format!("res://{}", r.script),
            _ => true,
        }));
    }
    // Segment wall time: a 0..1, b 1..2, c 2..5 (the process end).
    let c = j.results.iter().find(|r| r.script == "c.gd").unwrap();
    assert_eq!(c.run.seconds, 3.0);
}

#[test]
fn a_failing_test_names_its_script_and_the_others_pass() {
    let batch = names(&["a.gd", "b.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("b.gd", Some("test_two"), None));
    let marks = marks_of(&evs);
    let run = finished(evs, ExitKind::Code(1));
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd"]), &run, &marks);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Passed);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::Failed);
    let b = j.results.iter().find(|r| r.script == "b.gd").unwrap();
    assert_eq!(b.verdict.failures[0].test, "test_two");
    assert_eq!(b.verdict.failures[0].message, "boom");
}

#[test]
fn a_crash_marks_the_running_script_and_requeues_the_ones_that_never_ran() {
    let batch = names(&["a.gd", "b.gd", "c.gd", "d.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("b.gd", None, Some(3)));
    let marks = marks_of(&evs);
    let run = cut(evs, ExitKind::Signal(11));
    let j = judge_batch(
        &batch,
        &infos(&["a.gd", "b.gd", "c.gd", "d.gd"]),
        &run,
        &marks,
    );
    assert_eq!(j.results.len(), 2);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Passed);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::Crashed);
    assert_eq!(j.requeue, names(&["c.gd", "d.gd"]));
}

#[test]
fn a_timeout_marks_the_running_script_as_timed_out() {
    let batch = names(&["a.gd", "b.gd", "c.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("b.gd", None, Some(1)));
    let marks = marks_of(&evs);
    let run = cut(evs, ExitKind::TimedOut);
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd", "c.gd"]), &run, &marks);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Passed);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::TimedOut);
    assert_eq!(j.requeue, names(&["c.gd"]));
}

#[test]
fn a_process_that_never_started_a_script_charges_the_first_and_requeues_the_rest() {
    let batch = names(&["a.gd", "b.gd", "c.gd"]);
    let run = cut(vec![], ExitKind::SpawnFailed("nope".to_string()));
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd", "c.gd"]), &run, &[]);
    assert_eq!(j.results.len(), 1);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Crashed);
    assert_eq!(j.requeue, names(&["b.gd", "c.gd"]));
}

#[test]
fn a_script_gut_dropped_in_a_finished_batch_is_a_mismatch_not_a_requeue() {
    let batch = names(&["a.gd", "b.gd", "c.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("c.gd", None, None));
    let marks = marks_of(&evs);
    let run = finished(evs, ExitKind::Code(0));
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd", "c.gd"]), &run, &marks);
    assert!(j.requeue.is_empty());
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Passed);
    assert_eq!(outcome_of(&j, "c.gd"), Outcome::Passed);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::Mismatch);
    let b = j.results.iter().find(|r| r.script == "b.gd").unwrap();
    assert!(
        b.verdict.problems.iter().any(|p| p.contains("ran 0 of 2")),
        "{:?}",
        b.verdict.problems
    );
}

#[test]
fn a_batch_level_problem_is_a_mismatch_for_every_script() {
    let batch = names(&["a.gd", "b.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("b.gd", None, None));
    let marks = marks_of(&evs);
    let mut run = finished(evs, ExitKind::Code(0));
    run.summary.as_mut().unwrap().tests += 1;
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd"]), &run, &marks);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Mismatch);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::Mismatch);
}

#[test]
fn events_of_a_script_outside_the_batch_are_a_mismatch() {
    let batch = names(&["a.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("z.gd", None, None));
    let marks = marks_of(&evs);
    let run = finished(evs, ExitKind::Code(0));
    let j = judge_batch(&batch, &infos(&["a.gd"]), &run, &marks);
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Mismatch);
    assert!(
        j.results[0]
            .verdict
            .problems
            .iter()
            .any(|p| p.contains("z.gd")),
        "{:?}",
        j.results[0].verdict.problems
    );
}

#[test]
fn a_signal_after_the_summary_crashes_every_script_of_the_batch() {
    let batch = names(&["a.gd", "b.gd"]);
    let mut evs = script_events("a.gd", None, None);
    evs.extend(script_events("b.gd", None, None));
    let marks = marks_of(&evs);
    let run = finished(evs, ExitKind::Signal(11));
    let j = judge_batch(&batch, &infos(&["a.gd", "b.gd"]), &run, &marks);
    assert!(j.requeue.is_empty());
    assert_eq!(outcome_of(&j, "a.gd"), Outcome::Crashed);
    assert_eq!(outcome_of(&j, "b.gd"), Outcome::Crashed);
}

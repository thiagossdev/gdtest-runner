use std::path::Path;

use gdtest_runner::discover::{Naming, ScriptInfo, parse_script};
use gdtest_runner::events::{Event, parse_event_line, parse_log_notes, parse_summary};
use gdtest_runner::verdict::{ExitKind, Outcome, ProcessRun, judge};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gut")
            .join(name),
    )
    .unwrap()
}

const MIXED_SRC: &str = "extends GutTest\nfunc test_passes():\n\tpass\nfunc test_fails():\n\tpass\nfunc test_pending():\n\tpass\nfunc test_risky():\n\tpass\nclass TestInner:\n\textends GutTest\n\tfunc test_inner_passes():\n\t\tpass\n";
const SETUP_SRC: &str = "extends GutTest\nfunc test_ok():\n\tpass\nclass TestSkipped:\n\textends GutTest\n\tfunc test_never_runs():\n\t\tpass\n";
const PASSING_SRC: &str = "extends GutTest\nfunc test_a():\n\tpass\nfunc test_b():\n\tpass\nfunc test_c():\n\tpass\nfunc test_d():\n\tpass\n";

fn info(path: &str, src: &str) -> ScriptInfo {
    ScriptInfo {
        path: path.to_string(),
        classes: parse_script(src, &Naming::default()),
    }
}

fn run_of(log: &str, exit: ExitKind) -> ProcessRun {
    let mut events = Vec::new();
    let mut protocol_errors = Vec::new();
    for line in log.lines() {
        match parse_event_line(line) {
            Some(Ok(e)) => events.push(e),
            Some(Err(e)) => protocol_errors.push(e),
            None => {}
        }
    }
    ProcessRun {
        exit,
        events,
        protocol_errors,
        summary: parse_summary(log),
        notes: parse_log_notes(log),
        seconds: 1.0,
    }
}

#[test]
fn a_real_gut_summary_carries_its_warnings_and_orphans() {
    let log = fixture("warnings_orphans.log");
    let s = parse_summary(&log).unwrap();
    assert_eq!((s.warnings, s.orphans), (17, 8));
    assert_eq!((s.scripts, s.tests, s.asserts), (149, 1956, 12648));
}

#[test]
fn the_log_attributes_warnings_to_the_script_header_before_them() {
    let notes = parse_log_notes(&fixture("warnings_orphans.log"));
    assert_eq!(
        notes.warnings,
        vec![("tests/unit/test_effects.gd".to_string(), 1)]
    );
}

#[test]
fn the_log_attributes_orphans_from_the_orphan_section_by_file_name() {
    let notes = parse_log_notes(&fixture("warnings_orphans.log"));
    assert_eq!(
        notes.orphans,
        vec![
            ("test_scene_tree.gd".to_string(), 4),
            ("test_lighting.gd".to_string(), 4)
        ]
    );
    // The inline "2 Orphans" per-test lines are not double counted.
    assert_eq!(notes.orphans.iter().map(|(_, n)| n).sum::<u64>(), 8);
}

#[test]
fn a_summary_without_warnings_or_orphans_has_zeros() {
    let s = parse_summary(&fixture("passing.log")).unwrap();
    assert_eq!((s.warnings, s.orphans), (0, 0));
    let n = parse_log_notes(&fixture("passing.log"));
    assert!(n.warnings.is_empty() && n.orphans.is_empty());
}

#[test]
fn a_single_script_run_takes_the_summarys_warnings_and_orphans() {
    let mut run = run_of(&fixture("passing.log"), ExitKind::Code(0));
    let mut s = run.summary.unwrap();
    s.warnings = 3;
    s.orphans = 2;
    run.summary = Some(s);
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run,
    );
    assert_eq!(v.outcome, Outcome::Passed, "{:?}", v.problems);
    assert_eq!((v.counts.warnings, v.counts.orphans), (3, 2));
}

#[test]
fn a_clean_run_passes_with_its_counts() {
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of(&fixture("passing.log"), ExitKind::Code(0)),
    );
    assert_eq!(v.outcome, Outcome::Passed, "{:?}", v.problems);
    assert_eq!(v.counts.tests, 4);
    assert_eq!(v.counts.passing, 4);
    assert!(v.failures.is_empty());
}

#[test]
fn failing_tests_fail_with_their_messages() {
    let v = judge(
        &info("tests/fixture/test_fixture.gd", MIXED_SRC),
        &run_of(&fixture("mixed.log"), ExitKind::Code(1)),
    );
    assert_eq!(v.outcome, Outcome::Failed, "{:?}", v.problems);
    assert_eq!(v.failures.len(), 1);
    assert_eq!(v.failures[0].test, "test_fails");
    assert!(v.failures[0].message.contains("one is not two"));
    assert_eq!(v.counts.scripts, 2);
    assert_eq!(v.counts.risky_pending, 2);
}

#[test]
fn an_after_all_failure_fails_the_script_and_a_skip_is_not_a_mismatch() {
    let v = judge(
        &info("tests/fixture/test_fixture_setup.gd", SETUP_SRC),
        &run_of(&fixture("setup_skip.log"), ExitKind::Code(1)),
    );
    assert_eq!(v.outcome, Outcome::Failed, "{:?}", v.problems);
    assert!(v.problems.is_empty(), "{:?}", v.problems);
    assert_eq!(v.failures[0].test, "before_all/after_all");
    assert!(v.failures[0].message.contains("after_all broke"));
    assert_eq!(v.counts.failed_asserts, 1);
}

#[test]
fn no_summary_is_a_crash() {
    let log = fixture("passing.log");
    let cut = log.split("= Run Summary").next().unwrap().to_string();
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of(&cut, ExitKind::Signal(11)),
    );
    assert_eq!(v.outcome, Outcome::Crashed);
    assert!(
        v.problems.iter().any(|p| p.contains("signal 11")),
        "{:?}",
        v.problems
    );
}

#[test]
fn a_non_zero_exit_after_a_clean_summary_is_a_crash() {
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of(&fixture("passing.log"), ExitKind::Code(139)),
    );
    assert_eq!(v.outcome, Outcome::Crashed);
}

#[test]
fn a_timeout_wins_over_everything() {
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of("", ExitKind::TimedOut),
    );
    assert_eq!(v.outcome, Outcome::TimedOut);
}

#[test]
fn events_that_disagree_with_the_summary_are_a_mismatch() {
    let log = fixture("passing.log");
    // Drop one test_end event: GUT says 4 tests, the reporter saw 3.
    let mut dropped = false;
    let edited: String = log
        .lines()
        .filter(|l| {
            if !dropped && l.contains("\"event\":\"test_end\"") {
                dropped = true;
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n");
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of(&edited, ExitKind::Code(0)),
    );
    assert_eq!(v.outcome, Outcome::Mismatch);
    assert!(
        v.problems.iter().any(|p| p.contains("test_end")),
        "{:?}",
        v.problems
    );
}

#[test]
fn fewer_tests_than_discovered_is_a_mismatch() {
    let src = format!("{PASSING_SRC}func test_e():\n\tpass\n");
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", &src),
        &run_of(&fixture("passing.log"), ExitKind::Code(0)),
    );
    assert_eq!(v.outcome, Outcome::Mismatch);
    assert!(
        v.problems.iter().any(|p| p.contains("ran 4 of 5")),
        "{:?}",
        v.problems
    );
}

#[test]
fn no_reporter_events_is_a_mismatch_that_names_the_reporter() {
    let log: String = fixture("passing.log")
        .lines()
        .filter(|l| !l.contains("@@gdtest"))
        .collect::<Vec<_>>()
        .join("\n");
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run_of(&log, ExitKind::Code(0)),
    );
    assert_eq!(v.outcome, Outcome::Mismatch);
    assert!(
        v.problems.iter().any(|p| p.contains("reporter")),
        "{:?}",
        v.problems
    );
}

#[test]
fn a_protocol_error_is_a_mismatch() {
    let mut run = run_of(&fixture("passing.log"), ExitKind::Code(0));
    run.protocol_errors.push("bad line".to_string());
    let v = judge(
        &info("tests/unit/test_user_data_isolation.gd", PASSING_SRC),
        &run,
    );
    assert_eq!(v.outcome, Outcome::Mismatch);
}

#[test]
fn events_are_unused_import_guard() {
    // Keeps the Event import honest: the fixture ends with run_end.
    let run = run_of(&fixture("passing.log"), ExitKind::Code(0));
    assert!(matches!(run.events.last(), Some(Event::RunEnd)));
}

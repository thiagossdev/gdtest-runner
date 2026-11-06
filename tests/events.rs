use std::path::Path;

use gdtest_runner::events::{Event, Status, parse_event_line, parse_summary, strip_ansi};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gut")
            .join(name),
    )
    .unwrap()
}

fn events(log: &str) -> Vec<Event> {
    log.lines()
        .filter_map(parse_event_line)
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn strips_colour_codes() {
    assert_eq!(
        strip_ansi("\x1b[31m    [Failed]:  \x1b[0mx"),
        "    [Failed]:  x"
    );
}

#[test]
fn plain_lines_are_not_events() {
    assert!(parse_event_line("* test_passes").is_none());
    assert!(parse_event_line("").is_none());
}

#[test]
fn an_event_after_a_colour_reset_still_parses() {
    let line = "\x1b[0m@@gdtest {\"event\":\"run_end\"}";
    assert!(matches!(parse_event_line(line), Some(Ok(Event::RunEnd))));
}

#[test]
fn a_broken_event_line_is_an_error_not_a_skip() {
    let r = parse_event_line("@@gdtest {\"event\":\"test_end\"");
    assert!(matches!(r, Some(Err(_))));
}

#[test]
fn reads_every_event_of_a_mixed_run() {
    let ev = events(&fixture("mixed.log"));
    let ends: Vec<(&str, Status)> = ev
        .iter()
        .filter_map(|e| match e {
            Event::TestEnd(t) => Some((t.test.as_str(), t.status)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ends,
        vec![
            ("test_passes", Status::Passed),
            ("test_fails", Status::Failed),
            ("test_pending", Status::Pending),
            ("test_risky", Status::Risky),
            ("test_inner_passes", Status::Passed),
        ]
    );
    let fail = ev
        .iter()
        .find_map(|e| match e {
            Event::TestEnd(t) if t.status == Status::Failed => Some(t),
            _ => None,
        })
        .unwrap();
    assert!(fail.messages[0].contains("one is not two"));
    let script_ends: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::ScriptEnd(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(script_ends.len(), 2);
    assert_eq!(script_ends[0].inner, "");
    assert_eq!(script_ends[0].tests, 4);
    assert_eq!(script_ends[0].asserts_failed, 1);
    assert_eq!(script_ends[1].inner, "TestInner");
    assert!(matches!(ev.last(), Some(Event::RunEnd)));
}

#[test]
fn summary_of_a_mixed_run() {
    let s = parse_summary(&fixture("mixed.log")).unwrap();
    assert_eq!(s.scripts, 2);
    assert_eq!(s.tests, 5);
    assert_eq!(s.passing_tests, 2);
    assert_eq!(s.failing_tests, 1);
    assert_eq!(s.risky_pending, 2);
    assert_eq!(s.asserts, 4);
    assert_eq!(s.failed_asserts, 1);
}

#[test]
fn summary_with_none_and_plain_asserts() {
    let s = parse_summary(&fixture("passing.log")).unwrap();
    assert_eq!(s.scripts, 1);
    assert_eq!(s.tests, 4);
    assert_eq!(s.passing_tests, 4);
    assert_eq!(s.failing_tests, 0);
    assert_eq!(s.risky_pending, 0);
    assert_eq!(s.failed_asserts, 0);
    assert!(s.asserts > 0);
}

#[test]
fn skipped_script_event_and_setup_failure() {
    let log = fixture("setup_skip.log");
    let ev = events(&log);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::ScriptSkipped { inner, .. } if inner == "TestSkipped"))
    );
    let end = ev
        .iter()
        .find_map(|e| match e {
            Event::ScriptEnd(s) => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(end.setup_failures.len(), 1);
    let s = parse_summary(&log).unwrap();
    assert_eq!(
        (s.scripts, s.tests, s.failing_tests, s.risky_pending),
        (1, 1, 0, 1)
    );
    assert_eq!((s.asserts, s.failed_asserts), (2, 1));
}

#[test]
fn no_summary_in_a_crashed_log() {
    let log = fixture("mixed.log");
    let cut: String = log.lines().take(20).collect::<Vec<_>>().join("\n");
    assert!(parse_summary(&cut).is_none());
}

#[test]
fn nothing_was_run_is_an_all_zero_summary() {
    let s = parse_summary("[ERROR]:  Nothing was run.\nOn the one hand nothing failed").unwrap();
    assert_eq!((s.scripts, s.tests, s.asserts), (0, 0, 0));
}

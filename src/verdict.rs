//! The verdict on one script's process: its own GUT summary, the reporter's
//! events and static discovery must all agree, or the script fails.

use crate::discover::ScriptInfo;
use crate::events::{Event, GutSummary, LogNotes, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitKind {
    Code(i32),
    Signal(i32),
    TimedOut,
    SpawnFailed(String),
}

impl std::fmt::Display for ExitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitKind::Code(c) => write!(f, "exit {c}"),
            ExitKind::Signal(s) => write!(f, "killed by signal {s}"),
            ExitKind::TimedOut => write!(f, "timed out"),
            ExitKind::SpawnFailed(e) => write!(f, "could not start: {e}"),
        }
    }
}

/// What one Godot process left behind.
#[derive(Debug, Clone)]
pub struct ProcessRun {
    pub exit: ExitKind,
    pub events: Vec<Event>,
    pub protocol_errors: Vec<String>,
    pub summary: Option<GutSummary>,
    /// Warnings and orphans by script, read off the process's log.
    pub notes: LogNotes,
    pub seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    Crashed,
    TimedOut,
    Mismatch,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Passed => "ok",
            Outcome::Failed => "FAIL",
            Outcome::Crashed => "CRASH",
            Outcome::TimedOut => "TIMEOUT",
            Outcome::Mismatch => "MISMATCH",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// `test`, `Inner.test`, or `before_all/after_all` (with the inner
    /// class prefix when there is one).
    pub test: String,
    pub message: String,
}

/// Totals of one process, from the events (equal to GUT's summary when the
/// verdict has no problem).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub scripts: u64,
    pub tests: u64,
    pub passing: u64,
    pub failing: u64,
    pub risky_pending: u64,
    pub asserts: u64,
    pub failed_asserts: u64,
    /// GUT's warnings and orphan nodes: reported, never a failure.
    pub warnings: u64,
    pub orphans: u64,
}

impl Counts {
    pub fn add(&mut self, o: &Counts) {
        self.scripts += o.scripts;
        self.tests += o.tests;
        self.passing += o.passing;
        self.failing += o.failing;
        self.risky_pending += o.risky_pending;
        self.asserts += o.asserts;
        self.failed_asserts += o.failed_asserts;
        self.warnings += o.warnings;
        self.orphans += o.orphans;
    }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub outcome: Outcome,
    /// Why the process is a crash, a timeout or a mismatch.
    pub problems: Vec<String>,
    pub failures: Vec<Failure>,
    pub counts: Counts,
}

fn qualified(inner: &str, name: &str) -> String {
    if inner.is_empty() {
        name.to_string()
    } else {
        format!("{inner}.{name}")
    }
}

/// Counts and failures as the events tell them.
pub fn event_counts(events: &[Event]) -> (Counts, Vec<Failure>) {
    let mut c = Counts::default();
    let mut failures = Vec::new();
    for e in events {
        match e {
            Event::ScriptEnd(s) => {
                c.scripts += 1;
                c.tests += s.tests;
                c.passing += s.passing;
                c.failing += s.failing;
                c.risky_pending += s.risky + s.pending;
                c.asserts += s.asserts_passed + s.asserts_failed;
                c.failed_asserts += s.asserts_failed;
                for m in &s.setup_failures {
                    failures.push(Failure {
                        test: qualified(&s.inner, "before_all/after_all"),
                        message: m.clone(),
                    });
                }
            }
            Event::ScriptSkipped { .. } => c.risky_pending += 1,
            Event::TestEnd(t) if t.status == Status::Failed => failures.push(Failure {
                test: qualified(&t.inner, &t.test),
                message: t.messages.join("\n"),
            }),
            _ => {}
        }
    }
    (c, failures)
}

pub fn judge(info: &ScriptInfo, run: &ProcessRun) -> Verdict {
    let (counts, failures) = event_counts(&run.events);
    let mut problems = Vec::new();

    if run.exit == ExitKind::TimedOut {
        problems.push("timed out; the process group was killed".to_string());
        return Verdict {
            outcome: Outcome::TimedOut,
            problems,
            failures,
            counts,
        };
    }
    let crash = match (&run.exit, &run.summary) {
        (ExitKind::SpawnFailed(e), _) => Some(format!("could not start Godot: {e}")),
        (exit, None) => Some(format!("no GUT run summary ({exit})")),
        (ExitKind::Signal(s), Some(_)) => Some(format!("killed by signal {s} after its summary")),
        (ExitKind::Code(code), Some(s))
            if *code != 0 && s.failing_tests == 0 && s.failed_asserts == 0 =>
        {
            Some(format!("exit {code} with no failing test in its summary"))
        }
        _ => None,
    };
    if let Some(p) = crash {
        problems.push(p);
        return Verdict {
            outcome: Outcome::Crashed,
            problems,
            failures,
            counts,
        };
    }
    let summary = run.summary.expect("checked above");
    // One script, one process: every warning and orphan is its own.
    let counts = Counts {
        warnings: summary.warnings,
        orphans: summary.orphans,
        ..counts
    };

    problems.extend(run.protocol_errors.iter().cloned());
    check_events_against_summary(&run.events, &counts, &summary, &mut problems);
    check_against_discovery(info, &run.events, &mut problems);

    let outcome = if !problems.is_empty() {
        Outcome::Mismatch
    } else if counts.failing > 0 || counts.failed_asserts > 0 || !failures.is_empty() {
        Outcome::Failed
    } else {
        Outcome::Passed
    };
    Verdict {
        outcome,
        problems,
        failures,
        counts,
    }
}

pub(crate) fn check_events_against_summary(
    events: &[Event],
    c: &Counts,
    s: &GutSummary,
    problems: &mut Vec<String>,
) {
    if events.is_empty() {
        problems.push(
            "no reporter events: is the reporter vendored at the configured res:// path?"
                .to_string(),
        );
        return;
    }
    if !events.iter().any(|e| matches!(e, Event::RunEnd)) {
        problems.push("the reporter never reported the end of the run".to_string());
    }
    let pairs = [
        ("scripts", c.scripts, s.scripts),
        ("tests", c.tests, s.tests),
        ("passing tests", c.passing, s.passing_tests),
        ("failing tests", c.failing, s.failing_tests),
        ("risky/pending", c.risky_pending, s.risky_pending),
        ("asserts", c.asserts, s.asserts),
        ("failed asserts", c.failed_asserts, s.failed_asserts),
    ];
    for (name, events_say, gut_says) in pairs {
        if events_say != gut_says {
            problems.push(format!(
                "{name}: the reporter's events say {events_say}, GUT's summary says {gut_says}"
            ));
        }
    }
    let mut ends = 0u64;
    let mut passed = 0u64;
    let mut failed = 0u64;
    for e in events {
        if let Event::TestEnd(t) = e {
            ends += 1;
            passed += u64::from(t.status == Status::Passed);
            failed += u64::from(t.status == Status::Failed);
        }
    }
    if ends != s.tests {
        problems.push(format!(
            "{ends} test_end events for {} tests in GUT's summary",
            s.tests
        ));
    }
    if passed != s.passing_tests || failed != s.failing_tests {
        problems.push(format!(
            "test_end statuses say {passed} passed and {failed} failed, GUT's summary {} and {}",
            s.passing_tests, s.failing_tests
        ));
    }
}

pub(crate) fn check_against_discovery(
    info: &ScriptInfo,
    events: &[Event],
    problems: &mut Vec<String>,
) {
    let res = info.res_path();
    let mut ran: Vec<(&str, u64)> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for e in events {
        match e {
            Event::ScriptEnd(s) => {
                if s.script != res {
                    problems.push(format!("GUT ran {} in the process for {res}", s.script));
                }
                ran.push((&s.inner, s.tests));
            }
            Event::ScriptSkipped { inner, .. } => skipped.push(inner),
            _ => {}
        }
    }
    for class in &info.classes {
        if skipped.contains(&class.inner.as_str()) {
            continue;
        }
        let want = class.tests.len() as u64;
        let got: u64 = ran
            .iter()
            .filter(|(inner, _)| *inner == class.inner)
            .map(|(_, n)| n)
            .sum();
        if got != want {
            problems.push(format!(
                "ran {got} of {want} tests discovered in {}",
                qualified(&class.inner, &info.path)
            ));
        }
    }
    for (inner, n) in &ran {
        if *n > 0 && info.class(inner).is_none() {
            problems.push(format!(
                "GUT ran {n} tests in {} that discovery did not find",
                qualified(inner, &info.path)
            ));
        }
    }
}

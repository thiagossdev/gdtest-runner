//! The verdicts of a batch: several scripts run in one Godot process.
//!
//! The reporter's events carry their script, so they split per script.
//! GUT's run summary covers the whole process: it is checked against all
//! the batch's events (a disagreement fails every script of the batch);
//! static discovery is checked per script. When the process ends early (a
//! crash, a timeout, a failed start), the script running at that moment
//! takes the CRASH or TIMEOUT, the scripts that finished before it keep
//! their own verdicts, and the ones that never started are handed back for
//! a fresh process.

use std::collections::HashMap;

use crate::discover::ScriptInfo;
use crate::events::{Event, GutSummary, LogNotes};
use crate::verdict::{
    ExitKind, Outcome, ProcessRun, Verdict, check_against_discovery, check_events_against_summary,
    event_counts,
};

/// Where a script's output starts in the batch's process: the log line of
/// its first event and the seconds since the process started.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMark {
    pub script: String,
    pub line: usize,
    pub at: f64,
}

/// One script's share of a batch: its verdict, and the process run cut
/// down to its own events and wall-time segment.
#[derive(Debug, Clone)]
pub struct BatchScript {
    pub script: String,
    pub verdict: Verdict,
    pub run: ProcessRun,
}

#[derive(Debug, Clone)]
pub struct BatchJudgement {
    /// In batch order.
    pub results: Vec<BatchScript>,
    /// Scripts that never started: to run again in a fresh process.
    pub requeue: Vec<String>,
}

/// The project-relative script an event belongs to (None for run_end).
pub fn event_script(e: &Event) -> Option<&str> {
    let res = match e {
        Event::ScriptStart { script, .. }
        | Event::TestStart { script, .. }
        | Event::ScriptSkipped { script, .. } => script,
        Event::TestEnd(t) => &t.script,
        Event::ScriptEnd(s) => &s.script,
        Event::RunEnd => return None,
    };
    Some(res.strip_prefix("res://").unwrap_or(res))
}

pub fn judge_batch(
    scripts: &[String],
    infos: &HashMap<String, ScriptInfo>,
    run: &ProcessRun,
    marks: &[ScriptMark],
) -> BatchJudgement {
    let mut own: HashMap<&str, Vec<Event>> = HashMap::new();
    let mut started: Vec<&str> = Vec::new();
    let mut last: Option<&str> = None;
    let mut foreign: Vec<&str> = Vec::new();
    for e in &run.events {
        let Some(s) = event_script(e) else { continue };
        match scripts.iter().find(|b| b.as_str() == s) {
            Some(b) => {
                if !started.contains(&b.as_str()) {
                    started.push(b);
                }
                last = Some(b);
                own.entry(b).or_default().push(e.clone());
            }
            None => {
                if !foreign.contains(&s) {
                    foreign.push(s);
                }
            }
        }
    }
    let mut batch_problems: Vec<String> = run.protocol_errors.clone();
    for s in &foreign {
        batch_problems.push(format!(
            "in its batch: GUT ran {s}, which is not in the batch"
        ));
    }

    let abnormal =
        run.summary.is_none() || matches!(run.exit, ExitKind::TimedOut | ExitKind::SpawnFailed(_));
    let mut results = Vec::new();
    let mut requeue = Vec::new();
    if abnormal {
        let running = last.unwrap_or(&scripts[0]);
        for script in scripts {
            let s = script.as_str();
            let events = own.remove(s).unwrap_or_default();
            if s == running {
                let (counts, failures) = event_counts(&events);
                let (outcome, problem) = match &run.exit {
                    ExitKind::TimedOut => (
                        Outcome::TimedOut,
                        "timed out while it ran in a batch; the process group was killed"
                            .to_string(),
                    ),
                    ExitKind::SpawnFailed(e) => {
                        (Outcome::Crashed, format!("could not start Godot: {e}"))
                    }
                    exit => (
                        Outcome::Crashed,
                        format!(
                            "no GUT run summary ({exit}); the batch's process died while this script ran"
                        ),
                    ),
                };
                let verdict = Verdict {
                    outcome,
                    problems: vec![problem],
                    failures,
                    counts,
                };
                results.push(share(script, verdict, events, run, marks));
            } else if started.contains(&s) {
                let verdict = judge_own(infos, script, &events, &batch_problems, None);
                results.push(share(script, verdict, events, run, marks));
            } else {
                requeue.push(script.clone());
            }
        }
        apply_notes(&mut results, &run.notes);
        return BatchJudgement { results, requeue };
    }

    let summary = run.summary.expect("checked above");
    batch_problems.extend(note_problems(scripts, run, &summary));
    let (all_counts, _) = event_counts(&run.events);
    let mut summary_problems = Vec::new();
    check_events_against_summary(&run.events, &all_counts, &summary, &mut summary_problems);
    batch_problems.extend(
        summary_problems
            .into_iter()
            .map(|p| format!("in its batch: {p}")),
    );
    let crash = match &run.exit {
        ExitKind::Signal(s) => Some(format!(
            "the batch's process was killed by signal {s} after its summary"
        )),
        ExitKind::Code(code)
            if *code != 0 && summary.failing_tests == 0 && summary.failed_asserts == 0 =>
        {
            Some(format!(
                "the batch's process exited {code} with no failing test in its summary"
            ))
        }
        _ => None,
    };
    for script in scripts {
        let events = own.remove(script.as_str()).unwrap_or_default();
        let verdict = judge_own(infos, script, &events, &batch_problems, crash.as_deref());
        results.push(share(script, verdict, events, run, marks));
    }
    apply_notes(&mut results, &run.notes);
    BatchJudgement { results, requeue }
}

fn file_name(script: &str) -> &str {
    script.rsplit('/').next().unwrap_or(script)
}

fn warnings_of(notes: &LogNotes, script: &str) -> u64 {
    notes
        .warnings
        .iter()
        .filter(|(k, _)| k == script)
        .map(|(_, n)| n)
        .sum()
}

/// GUT names the scripts in its orphan section by file name only.
fn orphans_of(notes: &LogNotes, script: &str) -> u64 {
    notes
        .orphans
        .iter()
        .filter(|(k, _)| k == file_name(script))
        .map(|(_, n)| n)
        .sum()
}

/// Each script's own warnings and orphans, as the process's log attributes
/// them.
fn apply_notes(results: &mut [BatchScript], notes: &LogNotes) {
    for r in results {
        r.verdict.counts.warnings = warnings_of(notes, &r.script);
        r.verdict.counts.orphans = orphans_of(notes, &r.script);
    }
}

/// The attribution must account for GUT's own totals, or the per-script
/// numbers would silently lose some.
fn note_problems(scripts: &[String], run: &ProcessRun, summary: &GutSummary) -> Vec<String> {
    let mut problems = Vec::new();
    let warnings: u64 = scripts.iter().map(|s| warnings_of(&run.notes, s)).sum();
    if warnings != summary.warnings {
        problems.push(format!(
            "in its batch: warnings: the log attributes {warnings} to the batch's scripts, GUT's summary says {}",
            summary.warnings
        ));
    }
    let mut names: Vec<&str> = scripts.iter().map(|s| file_name(s)).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    let orphans: u64 = scripts.iter().map(|s| orphans_of(&run.notes, s)).sum();
    if before != names.len() && summary.orphans > 0 {
        problems.push(
            "in its batch: two scripts share a file name, so GUT's orphan section cannot tell them apart"
                .to_string(),
        );
    } else if orphans != summary.orphans {
        problems.push(format!(
            "in its batch: orphans: the log attributes {orphans} to the batch's scripts, GUT's summary says {}",
            summary.orphans
        ));
    }
    problems
}

/// A script judged on its own events: discovery, its test_end events
/// against its script_end counts, and the batch's problems.
fn judge_own(
    infos: &HashMap<String, ScriptInfo>,
    script: &str,
    events: &[Event],
    batch_problems: &[String],
    crash: Option<&str>,
) -> Verdict {
    let (counts, failures) = event_counts(events);
    if let Some(c) = crash {
        return Verdict {
            outcome: Outcome::Crashed,
            problems: vec![c.to_string()],
            failures,
            counts,
        };
    }
    let mut problems = batch_problems.to_vec();
    let info = infos.get(script).expect("batched scripts are discovered");
    check_against_discovery(info, events, &mut problems);
    let ends = events
        .iter()
        .filter(|e| matches!(e, Event::TestEnd(_)))
        .count() as u64;
    if ends != counts.tests {
        problems.push(format!(
            "{ends} test_end events for {} tests in its script_end events",
            counts.tests
        ));
    }
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

/// The script's cut of the process run: its events, and its wall-time
/// segment (the first script's includes the engine start).
fn share(
    script: &str,
    verdict: Verdict,
    events: Vec<Event>,
    run: &ProcessRun,
    marks: &[ScriptMark],
) -> BatchScript {
    let seconds = match marks.iter().position(|m| m.script == script) {
        Some(i) => {
            let start = if i == 0 { 0.0 } else { marks[i].at };
            let end = marks.get(i + 1).map_or(run.seconds, |m| m.at);
            (end - start).max(0.0)
        }
        None => 0.0,
    };
    BatchScript {
        script: script.to_string(),
        verdict,
        run: ProcessRun {
            exit: run.exit.clone(),
            events,
            protocol_errors: run.protocol_errors.clone(),
            summary: run.summary,
            notes: run.notes.clone(),
            seconds,
        },
    }
}

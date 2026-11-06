//! The reporter's event lines and GUT's own run summary.
//!
//! An event line is `@@gdtest <json>` on stdout, possibly after a colour
//! reset that GUT's logger left on the line. A line with the prefix that
//! does not parse is an error, never skipped.

use serde::Deserialize;

pub const PREFIX: &str = "@@gdtest ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    Pending,
    Risky,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TestEnd {
    pub script: String,
    pub inner: String,
    pub test: String,
    pub status: Status,
    pub asserts: u64,
    pub seconds: f64,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScriptEnd {
    pub script: String,
    pub inner: String,
    pub tests: u64,
    pub passing: u64,
    pub failing: u64,
    pub pending: u64,
    pub risky: u64,
    pub asserts_passed: u64,
    pub asserts_failed: u64,
    pub seconds: f64,
    pub setup_failures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    ScriptStart {
        script: String,
        inner: String,
        tests: u64,
    },
    TestStart {
        script: String,
        inner: String,
        test: String,
    },
    TestEnd(TestEnd),
    ScriptEnd(ScriptEnd),
    ScriptSkipped {
        script: String,
        inner: String,
        reason: String,
    },
    RunEnd,
}

/// Removes ANSI escape sequences (CSI `ESC [ ... letter`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// None for an ordinary output line; Some(Err) for a broken event line.
pub fn parse_event_line(line: &str) -> Option<Result<Event, String>> {
    if !line.contains(PREFIX) {
        return None;
    }
    let clean = strip_ansi(line);
    let at = clean.find(PREFIX)?;
    let json = clean[at + PREFIX.len()..].trim();
    Some(serde_json::from_str(json).map_err(|e| format!("unreadable reporter event {json:?}: {e}")))
}

/// GUT's totals for one process, from its "= Run Summary" block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GutSummary {
    pub scripts: u64,
    pub tests: u64,
    pub passing_tests: u64,
    pub failing_tests: u64,
    pub risky_pending: u64,
    pub asserts: u64,
    pub failed_asserts: u64,
    /// GUT prints these two only when non-zero.
    pub warnings: u64,
    pub orphans: u64,
}

/// The last Run Summary block of a log (colours stripped). GUT prints
/// "none" for a zero and "passed/total" for asserts when some failed. A run
/// with no test prints "Nothing was run." instead: an all-zero summary.
/// None when there is neither (a crash before the end).
pub fn parse_summary(log: &str) -> Option<GutSummary> {
    let clean = strip_ansi(log);
    let lines: Vec<&str> = clean.lines().collect();
    let Some(start) = lines.iter().rposition(|l| l.trim() == "= Run Summary") else {
        if lines.iter().any(|l| l.contains("Nothing was run.")) {
            return Some(GutSummary::default());
        }
        return None;
    };
    let mut s = GutSummary::default();
    let mut saw_scripts = false;
    let mut saw_tests = false;
    for line in &lines[start + 1..] {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["Scripts", n] => {
                s.scripts = num(n);
                saw_scripts = true;
            }
            ["Tests", n] => {
                s.tests = num(n);
                saw_tests = true;
            }
            ["Passing", "Tests", n] => s.passing_tests = num(n),
            ["Failing", "Tests", n] => s.failing_tests = num(n),
            ["Risky/Pending", n] => s.risky_pending = num(n),
            ["Warnings", n] => s.warnings = num(n),
            ["Orphans", n] => s.orphans = num(n),
            ["Asserts", n] => {
                if let Some((passed, total)) = n.split_once('/') {
                    s.asserts = num(total);
                    s.failed_asserts = num(total).saturating_sub(num(passed));
                } else {
                    s.asserts = num(n);
                    s.failed_asserts = 0;
                }
            }
            _ => {}
        }
    }
    (saw_scripts && saw_tests).then_some(s)
}

fn num(word: &str) -> u64 {
    word.parse().unwrap_or(0)
}

/// Where a process's warnings and orphans came from, read off its log:
/// `[GUT WARNING]` lines count for the script whose `res://...gd` header
/// came last; the "= N Orphans" section at the end names scripts by file
/// name and lists one `* [object]` line per orphan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogNotes {
    /// (project-relative script path, warnings), first appearance order.
    pub warnings: Vec<(String, u64)>,
    /// (script file name, orphans), first appearance order.
    pub orphans: Vec<(String, u64)>,
}

fn bump(list: &mut Vec<(String, u64)>, key: &str) {
    match list.iter_mut().find(|(k, _)| k == key) {
        Some((_, n)) => *n += 1,
        None => list.push((key.to_string(), 1)),
    }
}

fn is_orphans_banner(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("= ")
        .and_then(|r| r.strip_suffix(" Orphans"))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn parse_log_notes(log: &str) -> LogNotes {
    let clean = strip_ansi(log);
    let mut notes = LogNotes::default();
    let mut current = String::new();
    let mut in_orphans = false;
    let mut group = String::new();
    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed == "= Run Summary" {
            in_orphans = false;
        }
        if is_orphans_banner(trimmed) {
            in_orphans = true;
            group.clear();
            continue;
        }
        if in_orphans {
            if !line.starts_with(' ') && !trimmed.is_empty() && !trimmed.starts_with('=') {
                group = trimmed.to_string();
            } else if trimmed.starts_with("* [") && !group.is_empty() {
                bump(&mut notes.orphans, &group);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("res://")
            && let Some(at) = rest.find(".gd")
        {
            current = rest[..at + 3].to_string();
            continue;
        }
        if trimmed.starts_with("[GUT WARNING]") && !current.is_empty() {
            bump(&mut notes.warnings, &current);
        }
    }
    notes
}

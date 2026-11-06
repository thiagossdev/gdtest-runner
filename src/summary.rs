//! The end-of-run summary: failures, slowest scripts, wall time, then a
//! Run Summary block in GUT's own layout so tools that parse a GUT log
//! (Scripts / Tests / Passing Tests / Failing Tests / Asserts) read the
//! merged totals unchanged.

use std::path::Path;

use crate::run::ScriptResult;
use crate::verdict::{Counts, Outcome};

pub struct RunReport<'a> {
    pub results: &'a [ScriptResult],
    pub discovered_scripts: usize,
    pub discovered_tests: usize,
    pub workers: usize,
    pub wall_seconds: f64,
    pub interrupted: bool,
    /// List the scripts with warnings (the total is always printed).
    pub warnings_by_script: bool,
}

impl RunReport<'_> {
    pub fn totals(&self) -> Counts {
        let mut c = Counts::default();
        for r in self.results {
            c.add(&r.verdict.counts);
        }
        c
    }

    fn count(&self, outcome: Outcome) -> usize {
        self.results
            .iter()
            .filter(|r| r.verdict.outcome == outcome)
            .count()
    }

    /// Green: every discovered script ran, and every one passed.
    pub fn ok(&self) -> bool {
        !self.interrupted
            && self.results.len() == self.discovered_scripts
            && self
                .results
                .iter()
                .all(|r| r.verdict.outcome == Outcome::Passed)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let t = self.totals();

        let mut slow: Vec<&ScriptResult> = self.results.iter().collect();
        slow.sort_by(|a, b| b.run.seconds.total_cmp(&a.run.seconds));
        out += "\nSlowest scripts (process wall time)\n";
        for r in slow.iter().take(10) {
            out += &format!(
                "  {:>7.2} s  {:>4} tests  {}\n",
                r.run.seconds, r.verdict.counts.tests, r.script
            );
        }
        let sum: f64 = self.results.iter().map(|r| r.run.seconds).sum();
        out += &format!(
            "\ngdtest-runner: ran {} of {} scripts ({} tests discovered) on {} workers in {:.1} s wall ({:.1} s of process time)\n",
            self.results.len(),
            self.discovered_scripts,
            self.discovered_tests,
            self.workers,
            self.wall_seconds,
            sum
        );

        out += "\n==============================================\n";
        out += "= Run Summary\n";
        out += "==============================================\n";
        let mut bad: Vec<&ScriptResult> = self
            .results
            .iter()
            .filter(|r| r.verdict.outcome != Outcome::Passed)
            .collect();
        bad.sort_by(|a, b| a.script.cmp(&b.script));
        for r in &bad {
            out += &failure_lines(r);
        }
        out += &self.note_lines("Orphans", true);
        if self.warnings_by_script {
            out += &self.note_lines("Warnings", false);
        }
        out += "\nTotals\n------\n";
        out += &total_line("Warnings", t.warnings);
        out += "\n";
        out += &total_line("Scripts", t.scripts);
        out += &total_line("Tests", t.tests);
        out += &total_line("Passing Tests", t.passing);
        if t.failing > 0 {
            out += &total_line("Failing Tests", t.failing);
        }
        if t.risky_pending > 0 {
            out += &total_line("Risky/Pending", t.risky_pending);
        }
        if t.failed_asserts == 0 {
            out += &total_line("Asserts", t.asserts);
        } else {
            out += &format!(
                "{:<18}{:>5}\n",
                "Asserts",
                format!("{}/{}", t.asserts - t.failed_asserts, t.asserts)
            );
        }
        out += &total_line("Orphans", t.orphans);
        out += &format!(
            "{:<18}{:>5}\n",
            "Time",
            format!("{:.1}s", self.wall_seconds)
        );
        out += "\n";
        out += &format!("---- {} ----\n", self.final_line(&t));
        out
    }

    /// The scripts with orphans or warnings (largest first, then by path),
    /// each with its count and log. Empty when there are none.
    fn note_lines(&self, what: &str, orphans: bool) -> String {
        let count = |r: &ScriptResult| {
            if orphans {
                r.verdict.counts.orphans
            } else {
                r.verdict.counts.warnings
            }
        };
        let mut with: Vec<&ScriptResult> = self.results.iter().filter(|r| count(r) > 0).collect();
        if with.is_empty() {
            return String::new();
        }
        with.sort_by(|a, b| count(b).cmp(&count(a)).then(a.script.cmp(&b.script)));
        let mut out = format!("\n{what} by script\n");
        for r in with {
            out += &format!(
                "  {:>5}  {}\n         log: {}\n",
                count(r),
                r.script,
                display_path(&r.log)
            );
        }
        out
    }

    fn final_line(&self, t: &Counts) -> String {
        let mut parts = Vec::new();
        if self.interrupted {
            parts.push("interrupted".to_string());
        }
        if self.results.len() != self.discovered_scripts {
            parts.push(format!(
                "ran {} of {} scripts",
                self.results.len(),
                self.discovered_scripts
            ));
        }
        for (o, name) in [
            (Outcome::Crashed, "crashed"),
            (Outcome::TimedOut, "timed out"),
            (Outcome::Mismatch, "mismatched"),
        ] {
            let n = self.count(o);
            if n > 0 {
                parts.push(format!("{n} scripts {name}"));
            }
        }
        if t.failing > 0 {
            parts.push(format!("{} failing tests", t.failing));
        } else if self.count(Outcome::Failed) > 0 {
            parts.push(format!(
                "{} assert(s) in before_all/after_all methods failed",
                t.failed_asserts
            ));
        }
        if parts.is_empty() {
            if t.risky_pending > 0 {
                format!("{} pending/risky tests", t.risky_pending)
            } else {
                "All tests passed!".to_string()
            }
        } else {
            parts.join(", ")
        }
    }
}

fn total_line(label: &str, value: u64) -> String {
    let v = if value == 0 {
        "none".to_string()
    } else {
        value.to_string()
    };
    format!("{label:<18}{v:>5}\n")
}

/// Shows a path relative to the current dir when it is under it.
pub fn display_path(p: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| p.strip_prefix(cwd).ok().map(|r| r.display().to_string()))
        .unwrap_or_else(|| p.display().to_string())
}

fn indent(text: &str, pad: &str) -> String {
    text.lines()
        .map(|l| format!("{pad}{}\n", l.trim_end()))
        .collect()
}

/// The lines for one script that did not pass: each failure with its test
/// and message, each problem, and the log.
pub fn failure_lines(r: &ScriptResult) -> String {
    let label = r.verdict.outcome.label();
    let mut out = String::new();
    for f in &r.verdict.failures {
        out += &format!("\n{label} {} > {}\n", r.script, f.test);
        out += &indent(&f.message, "    ");
    }
    if !r.verdict.problems.is_empty() || r.verdict.failures.is_empty() {
        out += &format!("\n{label} {}: {}\n", r.script, r.run.exit);
        for p in &r.verdict.problems {
            out += &indent(p, "    ");
        }
    }
    out += &format!("    log: {}\n", display_path(&r.log));
    out
}

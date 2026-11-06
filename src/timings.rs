//! The timings table: the queue's input and the run's output.
//!
//! Script file (tab-separated, header first, slowest first, three
//! decimals): `script seconds tests asserts`. Per-test file:
//! `script test seconds`. Paths are project-relative without res://.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::discover::ScriptInfo;

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptTiming {
    pub script: String,
    pub seconds: f64,
    pub tests: u64,
    pub asserts: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestTiming {
    pub script: String,
    pub test: String,
    pub seconds: f64,
}

const SCRIPT_HEADER: &str = "script\tseconds\ttests\tasserts";
const TEST_HEADER: &str = "script\ttest\tseconds";

/// The table by script path; a missing file is an empty table, a malformed
/// one an error (never a silent partial order).
pub fn read_timings(path: &Path) -> Result<HashMap<String, ScriptTiming>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(format!("cannot read timings {}: {e}", path.display())),
    };
    let mut out = HashMap::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let bad = || format!("timings {} line {}: {line:?}", path.display(), i + 1);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 4 {
            return Err(bad());
        }
        let row = ScriptTiming {
            script: cols[0].to_string(),
            seconds: cols[1].parse().map_err(|_| bad())?,
            tests: cols[2].parse().map_err(|_| bad())?,
            asserts: cols[3].parse().map_err(|_| bad())?,
        };
        out.insert(row.script.clone(), row);
    }
    Ok(out)
}

/// Script paths in run order: scripts with no timing first (by path), then
/// the known ones slowest first (ties by path).
pub fn queue_order(scripts: &[ScriptInfo], timings: &HashMap<String, ScriptTiming>) -> Vec<String> {
    let mut unknown: Vec<&str> = Vec::new();
    let mut known: Vec<(&str, f64)> = Vec::new();
    for s in scripts {
        match timings.get(&s.path) {
            Some(t) => known.push((&s.path, t.seconds)),
            None => unknown.push(&s.path),
        }
    }
    unknown.sort();
    known.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    unknown
        .into_iter()
        .chain(known.into_iter().map(|(p, _)| p))
        .map(str::to_string)
        .collect()
}

/// Writes this run's rows, plus the old rows of scripts that still exist
/// (`existing`) but did not run this time (a partial run).
pub fn write_script_timings(
    path: &Path,
    ran: &[ScriptTiming],
    old: &HashMap<String, ScriptTiming>,
    existing: &[String],
) -> Result<(), String> {
    let mut rows: Vec<ScriptTiming> = ran.to_vec();
    for script in existing {
        if !ran.iter().any(|r| &r.script == script)
            && let Some(row) = old.get(script)
        {
            rows.push(row.clone());
        }
    }
    rows.sort_by(|a, b| {
        b.seconds
            .total_cmp(&a.seconds)
            .then(a.script.cmp(&b.script))
    });
    let mut text = format!("{SCRIPT_HEADER}\n");
    for r in &rows {
        text += &format!(
            "{}\t{:.3}\t{}\t{}\n",
            r.script, r.seconds, r.tests, r.asserts
        );
    }
    write_atomic(path, &text)
}

pub fn write_test_timings(path: &Path, rows: &[TestTiming]) -> Result<(), String> {
    let mut rows = rows.to_vec();
    rows.sort_by(|a, b| {
        b.seconds
            .total_cmp(&a.seconds)
            .then(a.script.cmp(&b.script))
            .then(a.test.cmp(&b.test))
    });
    let mut text = format!("{TEST_HEADER}\n");
    for r in &rows {
        text += &format!("{}\t{}\t{:.3}\n", r.script, r.test, r.seconds);
    }
    write_atomic(path, &text)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut f =
        std::fs::File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    f.write_all(text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

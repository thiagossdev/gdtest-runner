//! Static discovery of GUT test scripts and their test functions, without
//! starting Godot. The count it finds is the "ran N of N" guard: a script
//! GUT cannot parse, or a test it silently drops, shows up as a mismatch
//! between this count and what the run reports.
//!
//! What counts, as GUT collects it: files named `<script_prefix>*.gd` under
//! the test dirs (recursively); in each, top-level `func <test_prefix>*` and
//! the same at the first indentation level of an inner class named
//! `<inner_class_prefix>*` that extends `GutTest` (or GUT's test.gd).
//! Inherited test methods (a test script that extends another test script)
//! are not followed: they would surface as a mismatch, never silently.

use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Naming {
    pub script_prefix: String,
    pub script_suffix: String,
    pub test_prefix: String,
    pub inner_class_prefix: String,
}

impl Default for Naming {
    fn default() -> Self {
        Naming {
            script_prefix: "test_".to_string(),
            script_suffix: ".gd".to_string(),
            test_prefix: "test_".to_string(),
            inner_class_prefix: "Test".to_string(),
        }
    }
}

impl Naming {
    pub fn from_config(config: &Config) -> Naming {
        Naming {
            script_prefix: config.script_prefix.clone(),
            script_suffix: ".gd".to_string(),
            test_prefix: config.test_prefix.clone(),
            inner_class_prefix: config.inner_class_prefix.clone(),
        }
    }
}

/// The tests of one collected script: the file itself (`inner` empty) or
/// one of its inner test classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassTests {
    pub inner: String,
    pub tests: Vec<String>,
}

/// One test file: its project-relative path (forward slashes, no res://)
/// and its collected scripts, the file's own first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptInfo {
    pub path: String,
    pub classes: Vec<ClassTests>,
}

impl ScriptInfo {
    pub fn test_count(&self) -> usize {
        self.classes.iter().map(|c| c.tests.len()).sum()
    }

    pub fn res_path(&self) -> String {
        format!("res://{}", self.path)
    }

    pub fn class(&self, inner: &str) -> Option<&ClassTests> {
        self.classes.iter().find(|c| c.inner == inner)
    }
}

/// Every test script under the given project-relative dirs, sorted by path.
pub fn discover_dirs(
    project: &Path,
    dirs: &[String],
    naming: &Naming,
) -> Result<Vec<ScriptInfo>, String> {
    let mut files = Vec::new();
    for dir in dirs {
        let abs = project.join(dir);
        if !abs.is_dir() {
            return Err(format!(
                "test dir {dir} does not exist under {}",
                project.display()
            ));
        }
        walk(&abs, naming, &mut files)?;
    }
    let mut rels: Vec<String> = files
        .iter()
        .map(|f| relative(project, f))
        .collect::<Result<_, _>>()?;
    rels.sort();
    rels.dedup();
    rels.iter().map(|rel| load(project, rel, naming)).collect()
}

/// The named scripts (project-relative or res:// paths), in the given order.
pub fn discover_files(
    project: &Path,
    files: &[String],
    naming: &Naming,
) -> Result<Vec<ScriptInfo>, String> {
    files
        .iter()
        .map(|f| {
            let rel = normalize(f);
            if !project.join(&rel).is_file() {
                return Err(format!(
                    "no such test script: {f} (under {})",
                    project.display()
                ));
            }
            load(project, &rel, naming)
        })
        .collect()
}

fn normalize(path: &str) -> String {
    let p = path.strip_prefix("res://").unwrap_or(path);
    let p = p.strip_prefix("./").unwrap_or(p);
    p.replace('\\', "/")
}

fn walk(dir: &Path, naming: &Naming, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, naming, out)?;
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with(&naming.script_prefix)
            && name.ends_with(&naming.script_suffix)
        {
            out.push(path);
        }
    }
    Ok(())
}

fn relative(project: &Path, file: &Path) -> Result<String, String> {
    file.strip_prefix(project)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("{} is not under {}", file.display(), project.display()))
}

fn load(project: &Path, rel: &str, naming: &Naming) -> Result<ScriptInfo, String> {
    let src = std::fs::read_to_string(project.join(rel))
        .map_err(|e| format!("cannot read {rel}: {e}"))?;
    Ok(ScriptInfo {
        path: rel.to_string(),
        classes: parse_script(&src, naming),
    })
}

struct InnerClass {
    name: String,
    is_test: bool,
    extends_pending: bool,
    body_indent: Option<String>,
    tests: Vec<String>,
}

/// The collected scripts of one test file's source: the file itself first
/// (always present, maybe with no tests), then each inner test class in
/// source order.
pub fn parse_script(src: &str, naming: &Naming) -> Vec<ClassTests> {
    let mut top = ClassTests {
        inner: String::new(),
        tests: Vec::new(),
    };
    let mut inners: Vec<ClassTests> = Vec::new();
    let mut current: Option<InnerClass> = None;

    let close = |current: &mut Option<InnerClass>, inners: &mut Vec<ClassTests>| {
        if let Some(c) = current.take()
            && c.is_test
        {
            inners.push(ClassTests {
                inner: c.name,
                tests: c.tests,
            });
        }
    };

    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        if indent.is_empty() {
            close(&mut current, &mut inners);
            if let Some(rest) = trimmed.strip_prefix("class ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let named = name.starts_with(&naming.inner_class_prefix);
                let same_line_extends = rest.contains(" extends ");
                current = Some(InnerClass {
                    is_test: named && same_line_extends && extends_gut(rest),
                    extends_pending: named && !same_line_extends,
                    name,
                    body_indent: None,
                    tests: Vec::new(),
                });
            } else if let Some(name) = func_name(trimmed)
                && name.starts_with(&naming.test_prefix)
            {
                top.tests.push(name);
            }
            continue;
        }
        let Some(class) = current.as_mut() else {
            continue;
        };
        if class.body_indent.is_none() {
            class.body_indent = Some(indent.to_string());
        }
        if class.extends_pending {
            class.extends_pending = false;
            class.is_test = trimmed.starts_with("extends ") && extends_gut(trimmed);
        }
        if class.body_indent.as_deref() == Some(indent)
            && let Some(name) = func_name(trimmed)
            && name.starts_with(&naming.test_prefix)
        {
            class.tests.push(name);
        }
    }
    close(&mut current, &mut inners);
    let mut out = vec![top];
    out.extend(inners);
    out
}

fn extends_gut(text: &str) -> bool {
    text.contains("GutTest") || text.contains("res://addons/gut/test.gd")
}

fn func_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("func ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let after = rest[name.len()..].trim_start();
    if name.is_empty() || !after.starts_with('(') {
        return None;
    }
    Some(name)
}

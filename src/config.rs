//! The runner's settings: a TOML file in the calling project.
//!
//! `project` resolves against the config file's own directory; every other
//! path (test dirs, timings, logs) resolves against the project. Unknown
//! keys are an error, so a typo never silently falls back to a default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Isolation {
    /// Environment variable that gets a fresh, empty temp dir per process
    /// (XDG_DATA_HOME for a Linux Godot's user://). None: no data dir.
    #[serde(default)]
    pub data_dir_var: Option<String>,
    /// Environment variable that gets the worker slot's port base. None: no
    /// port variable.
    #[serde(default)]
    pub port_base_var: Option<String>,
    #[serde(default = "default_port_base")]
    pub port_base: u32,
    #[serde(default = "default_port_stride")]
    pub port_stride: u32,
}

impl Default for Isolation {
    fn default() -> Self {
        Isolation {
            data_dir_var: None,
            port_base_var: None,
            port_base: default_port_base(),
            port_stride: default_port_stride(),
        }
    }
}

fn default_port_base() -> u32 {
    20000
}
fn default_port_stride() -> u32 {
    10
}

/// Batch mode: scripts under `dirs` run several to one Godot process.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    /// Folders (project-relative, recursive) whose scripts run in batches.
    /// Empty: every script in its own process (unless --mode batch).
    #[serde(default)]
    pub dirs: Vec<String>,
    /// At most this many scripts per process.
    #[serde(default = "default_max_scripts")]
    pub max_scripts: usize,
    /// At most this many seconds of the timings file per process (a script
    /// with no row counts 0; a batch always takes one script). 0: no budget.
    #[serde(default)]
    pub max_seconds: f64,
}

impl Default for Batch {
    fn default() -> Self {
        Batch {
            dirs: Vec::new(),
            max_scripts: default_max_scripts(),
            max_seconds: 0.0,
        }
    }
}

fn default_max_scripts() -> usize {
    20
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Project root (holds project.godot), relative to the config file.
    #[serde(default = "default_project")]
    pub project: PathBuf,
    #[serde(default = "default_godot")]
    pub godot: String,
    /// Arguments before `-s <gut_cmdln>` (e.g. --headless, --fixed-fps 60).
    #[serde(default = "default_godot_args")]
    pub godot_args: Vec<String>,
    #[serde(default = "default_gut_cmdln")]
    pub gut_cmdln: String,
    /// Extra GUT options (e.g. -glog=1). The runner always adds -gconfig=,
    /// -gexit, -gpre_run_script=<reporter> and -gtest=<script>.
    #[serde(default)]
    pub gut_args: Vec<String>,
    /// res:// path of the vendored gdtest_reporter.gd.
    pub reporter: String,
    /// res:// path of the project's own GUT pre-run hook, chained by the
    /// reporter through GDTEST_PRE_RUN.
    #[serde(default)]
    pub pre_run_script: Option<String>,
    /// Folders scanned (recursively) for test scripts, relative to project.
    pub test_dirs: Vec<String>,
    #[serde(default = "default_script_prefix")]
    pub script_prefix: String,
    #[serde(default = "default_test_prefix")]
    pub test_prefix: String,
    #[serde(default = "default_inner_class_prefix")]
    pub inner_class_prefix: String,
    #[serde(default = "default_workers")]
    pub workers: usize,
    /// Seconds one script's process may take before it is killed.
    #[serde(default = "default_script_timeout")]
    pub script_timeout: u64,
    /// Script timings (queue order in, updated table out), project-relative.
    #[serde(default)]
    pub timings: Option<String>,
    /// Per-test timings written at the end, project-relative.
    #[serde(default)]
    pub test_timings: Option<String>,
    /// One log per script, project-relative.
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    /// Extra environment for every worker process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default)]
    pub batch: Batch,
}

fn default_project() -> PathBuf {
    PathBuf::from(".")
}
fn default_godot() -> String {
    "godot".to_string()
}
fn default_godot_args() -> Vec<String> {
    vec!["--headless".to_string()]
}
fn default_gut_cmdln() -> String {
    "addons/gut/gut_cmdln.gd".to_string()
}
fn default_script_prefix() -> String {
    "test_".to_string()
}
fn default_test_prefix() -> String {
    "test_".to_string()
}
fn default_inner_class_prefix() -> String {
    "Test".to_string()
}
fn default_workers() -> usize {
    6
}
fn default_script_timeout() -> u64 {
    600
}
fn default_log_dir() -> String {
    ".gdtest/logs".to_string()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        let mut config: Config =
            toml::from_str(&text).map_err(|e| format!("config {}: {e}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        let base = if base.as_os_str().is_empty() {
            Path::new(".")
        } else {
            base
        };
        let project = base.join(&config.project);
        config.project = project.canonicalize().map_err(|e| {
            format!(
                "config {}: project {}: {e}",
                path.display(),
                project.display()
            )
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.workers == 0 {
            return Err("workers must be at least 1".to_string());
        }
        if self.script_timeout == 0 {
            return Err("script_timeout must be at least 1 second".to_string());
        }
        if self.batch.max_scripts == 0 {
            return Err("batch.max_scripts must be at least 1".to_string());
        }
        if !(self.batch.max_seconds >= 0.0 && self.batch.max_seconds.is_finite()) {
            return Err(format!(
                "batch.max_seconds must be 0 or more, got {}",
                self.batch.max_seconds
            ));
        }
        if self.test_dirs.is_empty() {
            return Err("test_dirs is empty".to_string());
        }
        if !self.reporter.starts_with("res://") {
            return Err(format!(
                "reporter must be a res:// path, got {}",
                self.reporter
            ));
        }
        Ok(())
    }

    /// A project-relative path made absolute against the project.
    pub fn in_project(&self, rel: &str) -> PathBuf {
        self.project.join(rel)
    }
}

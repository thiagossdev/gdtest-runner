use std::path::{Path, PathBuf};

use gdtest_runner::config::Config;
use gdtest_runner::discover::{Naming, discover_dirs, discover_files, parse_script};

fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

#[test]
fn finds_test_scripts_in_dirs_and_subdirs_sorted() {
    let dirs = vec!["tests/unit".to_string(), "tests/integration".to_string()];
    let scripts = discover_dirs(&project(), &dirs, &Naming::default()).unwrap();
    let paths: Vec<&str> = scripts.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "tests/integration/test_slow.gd",
            "tests/unit/sub/test_sub.gd",
            "tests/unit/test_inner.gd",
            "tests/unit/test_plain.gd",
        ]
    );
}

#[test]
fn counts_top_level_tests_only() {
    let src = std::fs::read_to_string(project().join("tests/unit/test_plain.gd")).unwrap();
    let classes = parse_script(&src, &Naming::default());
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].inner, "");
    assert_eq!(classes[0].tests, vec!["test_one", "test_two"]);
}

#[test]
fn inner_test_classes_that_extend_gut_test() {
    let src = std::fs::read_to_string(project().join("tests/unit/test_inner.gd")).unwrap();
    let classes = parse_script(&src, &Naming::default());
    let shape: Vec<(&str, Vec<&str>)> = classes
        .iter()
        .map(|c| {
            (
                c.inner.as_str(),
                c.tests.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            ("", vec!["test_outer"]),
            ("TestFirst", vec!["test_a", "test_b"]),
            ("TestSecond", vec!["test_c"]),
        ]
    );
}

#[test]
fn test_count_sums_classes() {
    let files = vec!["tests/unit/test_inner.gd".to_string()];
    let scripts = discover_files(&project(), &files, &Naming::default()).unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].test_count(), 4);
}

#[test]
fn a_named_file_that_does_not_exist_is_an_error() {
    let files = vec!["tests/unit/test_missing.gd".to_string()];
    let err = discover_files(&project(), &files, &Naming::default()).unwrap_err();
    assert!(err.contains("test_missing.gd"), "{err}");
}

#[test]
fn a_missing_test_dir_is_an_error() {
    let dirs = vec!["tests/nowhere".to_string()];
    let err = discover_dirs(&project(), &dirs, &Naming::default()).unwrap_err();
    assert!(err.contains("tests/nowhere"), "{err}");
}

#[test]
fn res_paths_are_accepted_for_named_files() {
    let files = vec!["res://tests/unit/test_plain.gd".to_string()];
    let scripts = discover_files(&project(), &files, &Naming::default()).unwrap();
    assert_eq!(scripts[0].path, "tests/unit/test_plain.gd");
}

#[test]
fn config_resolves_project_against_its_own_dir() {
    let config = Config::load(&project().join("tools/gdtest.toml")).unwrap();
    assert_eq!(
        config.project.canonicalize().unwrap(),
        project().canonicalize().unwrap()
    );
    assert_eq!(config.godot, "godot4");
    assert_eq!(config.godot_args, vec!["--headless", "--fixed-fps", "60"]);
    assert_eq!(config.workers, 3);
    assert_eq!(config.script_timeout, 120);
    assert_eq!(config.test_dirs, vec!["tests/unit", "tests/integration"]);
    assert_eq!(config.reporter, "res://tools/gdtest_reporter.gd");
    assert_eq!(
        config.pre_run_script.as_deref(),
        Some("res://tests/pre_run.gd")
    );
    assert_eq!(config.env.get("EXTRA").map(String::as_str), Some("yes"));
    assert_eq!(config.isolation.port_base, 20000);
    assert_eq!(
        config.isolation.data_dir_var.as_deref(),
        Some("XDG_DATA_HOME")
    );
    assert_eq!(config.timings.as_deref(), Some(".gdtest/timings.tsv"));
}

#[test]
fn config_rejects_unknown_keys() {
    let dir = std::env::temp_dir().join(format!("gdtest-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.toml");
    std::fs::write(
        &file,
        "test_dirs = [\"t\"]\nreporter = \"res://r.gd\"\nworkerz = 3\n",
    )
    .unwrap();
    let err = Config::load(&file).unwrap_err();
    assert!(err.contains("workerz"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

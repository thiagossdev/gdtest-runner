use std::collections::HashMap;
use std::path::PathBuf;

use gdtest_runner::discover::{ClassTests, ScriptInfo};
use gdtest_runner::timings::{
    ScriptTiming, TestTiming, queue_order, read_timings, write_script_timings, write_test_timings,
};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gdtest-timings-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn script(path: &str) -> ScriptInfo {
    ScriptInfo {
        path: path.to_string(),
        classes: vec![ClassTests {
            inner: String::new(),
            tests: vec!["test_a".to_string()],
        }],
    }
}

#[test]
fn reads_the_timings_format() {
    let dir = tmp("read");
    let file = dir.join("t.tsv");
    std::fs::write(
        &file,
        "script\tseconds\ttests\tasserts\ntests/a.gd\t7.500\t2\t5\ntests/b.gd\t0.250\t1\t4\n",
    )
    .unwrap();
    let t = read_timings(&file).unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(t["tests/a.gd"].seconds, 7.5);
    assert_eq!(t["tests/b.gd"].tests, 1);
}

#[test]
fn a_missing_file_is_an_empty_table() {
    let t = read_timings(&tmp("missing").join("none.tsv")).unwrap();
    assert!(t.is_empty());
}

#[test]
fn a_malformed_row_is_an_error() {
    let dir = tmp("bad");
    let file = dir.join("t.tsv");
    std::fs::write(
        &file,
        "script\tseconds\ttests\tasserts\ntests/a.gd\tfast\t2\t5\n",
    )
    .unwrap();
    let err = read_timings(&file).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
}

#[test]
fn queue_is_unknown_first_then_slowest_first() {
    let scripts = vec![
        script("tests/fast.gd"),
        script("tests/new_b.gd"),
        script("tests/slow.gd"),
        script("tests/new_a.gd"),
        script("tests/mid.gd"),
    ];
    let mut t = HashMap::new();
    for (path, seconds) in [
        ("tests/fast.gd", 0.1),
        ("tests/slow.gd", 9.0),
        ("tests/mid.gd", 2.0),
    ] {
        t.insert(
            path.to_string(),
            ScriptTiming {
                script: path.to_string(),
                seconds,
                tests: 1,
                asserts: 1,
            },
        );
    }
    assert_eq!(
        queue_order(&scripts, &t),
        vec![
            "tests/new_a.gd",
            "tests/new_b.gd",
            "tests/slow.gd",
            "tests/mid.gd",
            "tests/fast.gd"
        ]
    );
}

#[test]
fn writes_slowest_first_and_keeps_rows_of_scripts_not_run() {
    let dir = tmp("write");
    let file = dir.join("sub/t.tsv");
    let mut old = HashMap::new();
    old.insert(
        "tests/kept.gd".to_string(),
        ScriptTiming {
            script: "tests/kept.gd".to_string(),
            seconds: 3.0,
            tests: 2,
            asserts: 4,
        },
    );
    old.insert(
        "tests/gone.gd".to_string(),
        ScriptTiming {
            script: "tests/gone.gd".to_string(),
            seconds: 5.0,
            tests: 1,
            asserts: 1,
        },
    );
    old.insert(
        "tests/ran.gd".to_string(),
        ScriptTiming {
            script: "tests/ran.gd".to_string(),
            seconds: 99.0,
            tests: 1,
            asserts: 1,
        },
    );
    let ran = vec![ScriptTiming {
        script: "tests/ran.gd".to_string(),
        seconds: 1.23456,
        tests: 3,
        asserts: 7,
    }];
    let still_there = vec!["tests/kept.gd".to_string(), "tests/ran.gd".to_string()];
    write_script_timings(&file, &ran, &old, &still_there).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "script\tseconds\ttests\tasserts\ntests/kept.gd\t3.000\t2\t4\ntests/ran.gd\t1.235\t3\t7\n"
    );
}

#[test]
fn writes_per_test_timings() {
    let dir = tmp("tests");
    let file = dir.join("tt.tsv");
    let rows = vec![
        TestTiming {
            script: "tests/a.gd".to_string(),
            test: "test_x".to_string(),
            seconds: 0.5,
        },
        TestTiming {
            script: "tests/b.gd".to_string(),
            test: "test_y".to_string(),
            seconds: 2.0,
        },
    ];
    write_test_timings(&file, &rows).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "script\ttest\tseconds\ntests/b.gd\ttest_y\t2.000\ntests/a.gd\ttest_x\t0.500\n"
    );
}

//! The versioning rules of docs/versioning.md: Cargo.toml's version is
//! SemVer, and CHANGELOG.md has `## Unreleased`, then version sections
//! newest first, the newest being Cargo.toml's version.

use std::cmp::Ordering;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

/// A SemVer 2.0.0 version (`1.2.3`, `1.2.3-rc.1`, `1.2.3+build`), or None.
fn parse_version(s: &str) -> Option<Version> {
    let s = match s.split_once('+') {
        Some((v, build)) => {
            if !idents_ok(build, false) {
                return None;
            }
            v
        }
        None => s,
    };
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => {
            if !idents_ok(p, true) {
                return None;
            }
            (c, p.split('.').map(str::to_string).collect())
        }
        None => (s, Vec::new()),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut nums = [0u64; 3];
    for (n, p) in nums.iter_mut().zip(&parts) {
        if !numeric_ok(p) {
            return None;
        }
        *n = p.parse().ok()?;
    }
    Some(Version { core: nums, pre })
}

fn numeric_ok(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'))
}

fn idents_ok(s: &str, pre: bool) -> bool {
    s.split('.').all(|id| {
        !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !(pre && id.bytes().all(|b| b.is_ascii_digit()) && !numeric_ok(id))
    })
}

/// SemVer precedence.
fn compare(a: &Version, b: &Version) -> Ordering {
    a.core
        .cmp(&b.core)
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (x, y) in a.pre.iter().zip(&b.pre) {
                    let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => x.cmp(y),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                a.pre.len().cmp(&b.pre.len())
            }
        })
}

fn date_ok(d: &str) -> bool {
    let p: Vec<&str> = d.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return false;
    }
    let n: Vec<u32> = match p.iter().map(|x| x.parse()).collect() {
        Ok(n) => n,
        Err(_) => return false,
    };
    (1..=12).contains(&n[1]) && (1..=31).contains(&n[2])
}

const SUBSECTIONS: [&str; 6] = [
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
];

/// Every way `changelog` breaks the rules for crate version `version`.
fn changelog_problems(changelog: &str, version: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if parse_version(version).is_none() {
        problems.push(format!("Cargo.toml version {version} is not SemVer"));
    }
    let mut seen_unreleased = false;
    let mut versions: Vec<(String, Version, bool)> = Vec::new(); // text, parsed, dated
    for (i, line) in changelog.lines().enumerate() {
        let n = i + 1;
        if let Some(h) = line.strip_prefix("### ") {
            if !SUBSECTIONS.contains(&h) {
                problems.push(format!(
                    "line {n}: subsection '{h}' is not one of {SUBSECTIONS:?}"
                ));
            }
            continue;
        }
        let Some(h) = line.strip_prefix("## ") else {
            continue;
        };
        if h == "Unreleased" {
            if seen_unreleased || !versions.is_empty() {
                problems.push(format!("line {n}: '## Unreleased' must come once, first"));
            }
            seen_unreleased = true;
            continue;
        }
        if !seen_unreleased {
            problems.push(format!(
                "line {n}: '## Unreleased' must come before '## {h}'"
            ));
            seen_unreleased = true;
        }
        let Some((v, date)) = h.split_once(" - ") else {
            problems.push(format!("line {n}: '## {h}' is not '## X.Y.Z - YYYY-MM-DD'"));
            continue;
        };
        let Some(parsed) = parse_version(v) else {
            problems.push(format!("line {n}: {v} is not SemVer"));
            continue;
        };
        let dated = date_ok(date);
        if !dated && date != "Unreleased" {
            problems.push(format!("line {n}: date '{date}' is not YYYY-MM-DD"));
        }
        if !dated && !versions.is_empty() {
            problems.push(format!(
                "line {n}: only the newest version may be Unreleased"
            ));
        }
        if let Some((prev, prev_parsed, _)) = versions.last()
            && compare(prev_parsed, &parsed) != Ordering::Greater
        {
            problems.push(format!("line {n}: {v} must be older than {prev} above it"));
        }
        versions.push((v.to_string(), parsed, dated));
    }
    if !seen_unreleased {
        problems.push("no '## Unreleased' section".to_string());
    }
    match versions.first() {
        None => problems.push("no version section".to_string()),
        Some((v, _, _)) if v != version => problems.push(format!(
            "the newest changelog section is {v}, Cargo.toml says {version}"
        )),
        _ => {}
    }
    problems
}

#[test]
fn the_crate_version_and_the_changelog_follow_the_rules() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("CHANGELOG.md");
    let changelog = std::fs::read_to_string(path).unwrap();
    let problems = changelog_problems(&changelog, env!("CARGO_PKG_VERSION"));
    assert!(
        problems.is_empty(),
        "CHANGELOG.md / Cargo.toml (see docs/versioning.md):\n{}",
        problems.join("\n")
    );
}

// --- the rules themselves --------------------------------------------------

const GOOD: &str = "# Changelog\n\nintro\n\n## Unreleased\n\n### Fixed\n\n- a\n\n\
                    ## 0.2.0 - Unreleased\n\n### Added\n\n- b\n\n## 0.1.0 - 2026-11-06\n";

#[test]
fn a_well_formed_changelog_passes() {
    assert_eq!(changelog_problems(GOOD, "0.2.0"), Vec::<String>::new());
}

#[test]
fn each_broken_rule_is_reported() {
    let cases: [(&str, &str, &str); 9] = [
        (GOOD, "0.3.0", "Cargo.toml says 0.3.0"),
        (GOOD, "0.2", "not SemVer"),
        (
            &GOOD.replace("## Unreleased", "## Next"),
            "0.2.0",
            "is not '## X.Y.Z",
        ),
        (
            &GOOD.replace("## Unreleased\n", ""),
            "0.2.0",
            "must come before",
        ),
        (
            &GOOD.replace("0.1.0 - 2026-11-06", "0.3.0 - 2026-11-06"),
            "0.2.0",
            "must be older",
        ),
        (
            &GOOD.replace("0.1.0 - 2026-11-06", "0.2.0 - 2026-11-06"),
            "0.2.0",
            "must be older",
        ),
        (
            &GOOD.replace("2026-11-06", "2026-13-06"),
            "0.2.0",
            "not YYYY-MM-DD",
        ),
        (
            &GOOD.replace("0.1.0 - 2026-11-06", "0.1.0 - Unreleased"),
            "0.2.0",
            "only the newest",
        ),
        (
            &GOOD.replace("### Fixed", "### Fixes"),
            "0.2.0",
            "subsection 'Fixes'",
        ),
    ];
    for (text, version, want) in cases {
        let problems = changelog_problems(text, version);
        assert!(
            problems.iter().any(|p| p.contains(want)),
            "want '{want}' for version {version} in {problems:?}\n{text}"
        );
    }
}

#[test]
fn semver_parsing_and_precedence_follow_the_spec() {
    for ok in [
        "0.1.0",
        "1.0.0-rc.1",
        "1.0.0-alpha-1",
        "1.2.3+build.5",
        "10.20.30",
    ] {
        assert!(parse_version(ok).is_some(), "{ok}");
    }
    for bad in [
        "1.0", "01.0.0", "1.0.0-", "1.0.0-01", "1.0.0+", "v1.0.0", "1.0.0.0",
    ] {
        assert!(parse_version(bad).is_none(), "{bad}");
    }
    // The order from semver.org section 11.
    let order = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.1.0",
        "2.0.0",
    ];
    for w in order.windows(2) {
        let (a, b) = (parse_version(w[0]).unwrap(), parse_version(w[1]).unwrap());
        assert_eq!(compare(&a, &b), Ordering::Less, "{} < {}", w[0], w[1]);
    }
}

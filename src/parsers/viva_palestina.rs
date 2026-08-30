// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! viva-palestina audits project dependencies against an ethical-vendor
//! policy. Its `scan` command has no JSON mode, so this parser reads the
//! human-readable report: dependency count, critical excluded count, vendor
//! exposure list, and the per-dependency detailed analysis.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::{json, Value};

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let mut total = 0u64;
    let mut critical = 0u64;
    let mut deps: Vec<Dependency> = Vec::new();

    for line in stdout.lines() {
        if let Some(n) = parse_discovered(line) {
            total = n;
        } else if let Some(n) = parse_critical_count(line) {
            critical = n;
        } else if let Some(dep) = parse_dependency(line) {
            deps.push(dep);
        }
    }

    // If the detailed section is missing but the summary gave a total, treat
    // the scan as having zero classified dependencies rather than claiming
    // zero exposure.
    if total == 0 && deps.is_empty() {
        return ParseOutcome {
            status: Status::Ok,
            score: Some(100.0),
            summary: "no dependencies discovered".to_string(),
            findings: Vec::new(),
            note: None,
            raw: Some(json!({ "total_dependencies": 0, "dependencies": [] })),
        };
    }

    // Prefer the explicit total from the summary line; fall back to the
    // number of classified dependencies we parsed.
    let total = total.max(deps.len() as u64);
    let excluded = count_status(&deps, "EXCLUDE");
    let review = count_status(&deps, "REVIEW");
    let allow = count_status(&deps, "ALLOW");
    let unknown = count_status(&deps, "UNKNOWN");

    let excluded_ratio = excluded as f64 / total as f64;
    let review_ratio = review as f64 / total as f64;
    let score = clamp_score(100.0 - excluded_ratio * 80.0 - review_ratio * 20.0);

    let status = if excluded > 0 || critical > 0 {
        Status::Fail
    } else if review > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = format!(
        "{total} dependencies: {excluded} excluded, {review} review, {allow} allow, {unknown} unknown",
    );

    let mut findings: Vec<String> = deps
        .iter()
        .filter(|d| d.status == "EXCLUDE" || d.status == "REVIEW")
        .map(|d| {
            format!(
                "{} [{}] {} (Conf: {:.1}%)",
                d.name, d.status, d.kind, d.confidence
            )
        })
        .collect();
    findings.truncate(5);

    let raw = Some(json!({
        "total_dependencies": total,
        "critical_excluded": critical,
        "excluded": excluded,
        "review": review,
        "allow": allow,
        "unknown": unknown,
        "dependencies": deps.iter().map(|d| json!({
            "name": d.name,
            "status": d.status,
            "kind": d.kind,
            "confidence": d.confidence,
        })).collect::<Vec<Value>>(),
    }));

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: if unknown > 0 {
            Some(format!("{unknown} vendor(s) had UNKNOWN status; they are not penalized until the registry classifies them"))
        } else {
            None
        },
        raw,
    }
}

struct Dependency {
    name: String,
    status: String,
    kind: String,
    confidence: f64,
}

fn count_status(deps: &[Dependency], status: &str) -> u64 {
    deps.iter().filter(|d| d.status == status).count() as u64
}

fn parse_discovered(line: &str) -> Option<u64> {
    let rest = line.trim_start().strip_prefix("Discovered ")?;
    rest.split_whitespace().next()?.parse().ok()
}

fn parse_critical_count(line: &str) -> Option<u64> {
    let trimmed = line.trim_start();
    if trimmed.contains("No critical excluded dependencies found") {
        return Some(0);
    }
    if !trimmed.contains("critical excluded dependencies") {
        return None;
    }
    trimmed
        .split("Found ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn parse_dependency(line: &str) -> Option<Dependency> {
    let trimmed = line.trim_start();
    // Detailed lines look like:
    //   Name | Exclude | runtime | Conf: 90.0%
    // Names themselves may contain spaces or " | ", so parse the fixed
    // tail (status | kind | Conf: ...) and treat everything before it as
    // the name.
    let parts: Vec<&str> = trimmed.split(" | ").collect();
    if parts.len() < 4 {
        return None;
    }
    let status = parts[parts.len() - 3].trim().to_ascii_uppercase();
    let kind = parts[parts.len() - 2].trim();
    let conf_part = parts[parts.len() - 1].trim();
    let name = parts[..parts.len() - 3].join(" | ").trim().to_string();
    if name.is_empty() {
        return None;
    }

    let confidence = conf_part
        .strip_prefix("Conf: ")
        .and_then(|s| s.strip_suffix('%'))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    Some(Dependency {
        name,
        status,
        kind: kind.to_string(),
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dependencies_scores_perfect() {
        let stdout = "Scanning repository: /tmp/empty\n\nDiscovered 0 dependencies across 0 manifest files\n";
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, Some(100.0));
        assert!(out.summary.contains("no dependencies"));
    }

    #[test]
    fn clean_dependencies_are_ok() {
        let stdout = r#"Scanning repository: /tmp/clean

Discovered 2 dependencies across 1 manifest files: Cargo.toml

## Critical Dependencies

✓ No critical excluded dependencies found.

## Vendor Exposure

  • Grafana (grafana) - 1 dependencies

## Detailed Dependency Analysis

  Grafana | Allow | runtime | Conf: 90.0%
  Travis CI | Allow | runtime | Conf: 60.0%
"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, Some(100.0));
        assert!(out.summary.contains("0 excluded, 0 review, 2 allow"));
    }

    #[test]
    fn excluded_dependency_fails() {
        let stdout = r#"Scanning repository: /tmp/bad

Discovered 2 dependencies across 1 manifest files: Cargo.toml

## Critical Dependencies

⚠ Found 1 critical excluded dependencies:

  • Microsoft (Microsoft)

## Vendor Exposure

  • Microsoft (microsoft) - 1 dependencies

## Detailed Dependency Analysis

  Microsoft | Exclude | runtime | Conf: 90.0%
  Grafana | Allow | runtime | Conf: 90.0%
"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert!(out.score.unwrap() < 100.0);
        assert!(out.findings.iter().any(|f| f.contains("Microsoft")));
    }

    #[test]
    fn review_dependency_warns() {
        let stdout = r#"Scanning repository: /tmp/review

Discovered 2 dependencies across 1 manifest files: Cargo.toml

## Critical Dependencies

✓ No critical excluded dependencies found.

## Vendor Exposure

  • SimilarWeb (similarweb) - 1 dependencies

## Detailed Dependency Analysis

  SimilarWeb | Review | runtime | Conf: 70.0%
  Grafana | Allow | runtime | Conf: 90.0%
"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Warn);
        assert!((out.score.unwrap() - 90.0).abs() < 1e-9);
        assert!(out.findings.iter().any(|f| f.contains("SimilarWeb")));
    }

    #[test]
    fn mixed_statuses_and_kinds() {
        let stdout = r#"Scanning repository: /tmp/mixed

Discovered 3 dependencies across 1 manifest files: Cargo.toml

## Dependency Exposure Summary

### Development Dependencies: 1

### Build Dependencies: 1

### Runtime Dependencies: 1

## Critical Dependencies

⚠ Found 1 critical excluded dependencies:

  • Microsoft (Microsoft)

## Vendor Exposure

  • Microsoft (microsoft) - 1 dependencies
  • SimilarWeb (similarweb) - 1 dependencies
  • Grafana (grafana) - 1 dependencies

## Detailed Dependency Analysis

  Microsoft | Exclude | runtime | Conf: 90.0%
  Grafana | Allow | development | Conf: 90.0%
  SimilarWeb | Review | build | Conf: 90.0%
"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Fail);
        // 1 excluded of 3 => 80/3 = 26.67 penalty; 1 review of 3 => 6.67 penalty => 66.67.
        assert!((out.score.unwrap() - 66.666_666_666_666_66).abs() < 1e-9);
        assert_eq!(out.findings.len(), 2);
    }

    #[test]
    fn name_with_pipe_is_parsed_correctly() {
        let stdout = r#"Discovered 1 dependencies across 1 manifest files: Cargo.toml

## Detailed Dependency Analysis

  Google / Alphabet | Exclude | runtime | Conf: 95.0%
"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert!(out.findings.iter().any(|f| f.contains("Google / Alphabet")));
    }
}

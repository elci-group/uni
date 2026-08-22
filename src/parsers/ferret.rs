// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! `ferret hunt` (called `track` by some builds) prints a stable human summary
//! followed by CodeRabbit-style findings. Ferret currently has no JSON output
//! for this operation, so this parser consumes the labelled summary and
//! finding lines while retaining no ANSI-decorated raw output.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    if exit_code != Some(0) {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "ferret hunt failed".to_string(),
            findings: Vec::new(),
            note: Some(format!("ferret track exited {exit_code:?}")),
            raw: None,
        };
    }

    let hunks = labelled_count(stdout, "hunks:");
    let finding_count = labelled_count(stdout, "findings:");
    let findings: Vec<String> = stdout.lines().filter_map(parse_finding).take(10).collect();

    let Some(finding_count) = finding_count else {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "could not parse ferret hunt output".to_string(),
            findings,
            note: Some(format!(
                "expected a labelled findings count; stdout began: {:?}",
                stdout.chars().take(300).collect::<String>()
            )),
            raw: None,
        };
    };

    let critical = count_severity(stdout, "Critical");
    let major = count_severity(stdout, "Major");
    let minor = count_severity(stdout, "Minor");
    let info = count_severity(stdout, "Info");
    let score = clamp_score(
        100.0 - critical as f64 * 25.0 - major as f64 * 15.0 - minor as f64 * 5.0 - info as f64,
    );
    let status = if critical > 0 || major > 0 {
        Status::Fail
    } else if finding_count > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = match hunks {
        Some(hunks) => format!(
            "{finding_count} review findings across {hunks} hunks: {critical} critical, {major} major, {minor} minor, {info} info"
        ),
        None => format!(
            "{finding_count} review findings: {critical} critical, {major} major, {minor} minor, {info} info"
        ),
    };

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: Some(
            "score deducts 25/15/5/1 points per critical/major/minor/info finding".to_string(),
        ),
        raw: None,
    }
}

fn labelled_count(stdout: &str, label: &str) -> Option<u64> {
    stdout.lines().find_map(|line| {
        let plain = strip_ansi(line);
        let (_, rest) = plain.split_once(label)?;
        let token = rest.split_whitespace().next()?;
        match token.parse() {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::trace!(label, token, error = %e, "labelled count token did not parse");
                None
            }
        }
    })
}

fn count_severity(stdout: &str, severity: &str) -> usize {
    stdout
        .lines()
        .filter(|line| {
            let plain = strip_ansi(line);
            plain.contains('▸') && plain.contains(&format!("[{severity}]"))
        })
        .count()
}

fn parse_finding(line: &str) -> Option<String> {
    let plain = strip_ansi(line);
    let (_, finding) = plain.split_once('▸')?;
    let finding = finding.trim();
    finding.starts_with('[').then(|| finding.to_string())
}

fn strip_ansi(input: &str) -> String {
    let mut plain = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &str = "Track summary\n  target: /tmp/project\n  hunks: 8\n  findings: 0\n";
    const FINDINGS: &str = "Track summary\n  hunks: 12\n  findings: 3\n\nCodeRabbit findings (3)\n  ▸ [Major] src/main.rs:10 — Avoid panic\n  ▸ [Minor] src/lib.rs:20 — Track this TODO\n  ▸ [Info] src/lib.rs:30 — Debug print\n";

    #[test]
    fn clean_hunt_scores_perfectly() {
        let out = parse(CLEAN, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, Some(100.0));
        assert!(out.summary.contains("0 review findings across 8 hunks"));
    }

    #[test]
    fn findings_are_weighted_and_extracted() {
        let out = parse(FINDINGS, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert_eq!(out.score, Some(79.0));
        assert_eq!(out.findings.len(), 3);
        assert!(out.findings[0].contains("Avoid panic"));
    }

    #[test]
    fn malformed_output_is_an_error() {
        let out = parse("unexpected", Some(0));
        assert_eq!(out.status, Status::Error);
        assert_eq!(out.score, None);
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        let out = parse("", Some(2));
        assert_eq!(out.status, Status::Error);
        assert_eq!(out.score, None);
    }
}

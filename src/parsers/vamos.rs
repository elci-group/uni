// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! vamos has no `--json` output either; `stats` prints one fixed-format
//! line per action: `NAME  triggered=N  nominal=N  validated=N
//! avg_structural_confidence=F`. It also has a real "not applicable yet"
//! state that isn't an error: a project simply hasn't adopted vamos
//! tracking (no `vamos.toml`), or has adopted it but has no recorded
//! instances. Both are handled by [`crate::run::run_vamos`] before this
//! parser ever sees stdout — empty stdout here means "no instances", not
//! "something broke."
//!
//! vamos's whole point is telling nominal actions (triggered, never
//! progressed) apart from validated ones (completion predicate actually
//! holds). The score is exactly that ratio, aggregated across every
//! action in the session — it only means anything once a project has
//! been recording real usage through `vamos trigger`/`step`/`fact`.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let mut rows = Vec::new();
    for line in stdout.lines() {
        if let Some(row) = parse_row(line) {
            rows.push(row);
        }
    }

    if rows.is_empty() {
        return ParseOutcome {
            status: Status::Warn,
            score: None,
            summary: "no action instances recorded yet".to_string(),
            findings: Vec::new(),
            note: Some(
                "vamos.toml is present but no instances have been triggered yet; run `vamos trigger`/`step`/`fact` against real usage to start scoring completion"
                    .to_string(),
            ),
            raw: None,
        };
    }

    let total_triggered: u64 = rows.iter().map(|r| r.triggered).sum();
    let total_nominal: u64 = rows.iter().map(|r| r.nominal).sum();
    let total_validated: u64 = rows.iter().map(|r| r.validated).sum();

    let validated_ratio = total_validated as f64 / total_triggered as f64;
    let nominal_ratio = total_nominal as f64 / total_triggered as f64;
    let score = clamp_score(100.0 * validated_ratio - 20.0 * nominal_ratio);

    let status = if score >= 70.0 {
        Status::Ok
    } else if score >= 40.0 {
        Status::Warn
    } else {
        Status::Fail
    };

    let action_count = rows.len();
    let summary = format!(
        "{action_count} actions, {total_triggered} instances: {total_validated} validated, {total_nominal} nominal (never progressed)"
    );

    let mut findings: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{}: triggered={} nominal={} validated={} avg_confidence={:.2}",
                r.action, r.triggered, r.nominal, r.validated, r.avg_structural_confidence
            )
        })
        .collect();
    findings.truncate(5);

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: None,
        raw: None,
    }
}

struct Row {
    action: String,
    triggered: u64,
    nominal: u64,
    validated: u64,
    avg_structural_confidence: f64,
}

fn parse_row(line: &str) -> Option<Row> {
    let triggered_idx = line.find("triggered=")?;
    let action = line[..triggered_idx].trim().to_string();
    if action.is_empty() {
        return None;
    }
    Some(Row {
        action,
        triggered: parse_after(line, "triggered=")?,
        nominal: parse_after(line, "nominal=")?,
        validated: parse_after(line, "validated=")?,
        avg_structural_confidence: parse_float_after(line, "avg_structural_confidence=")?,
    })
}

fn parse_after(line: &str, marker: &str) -> Option<u64> {
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse() {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::trace!(marker, digits, error = %e, "marker had no parseable integer");
            None
        }
    }
}

fn parse_float_after(line: &str, marker: &str) -> Option<f64> {
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    match digits.parse() {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::trace!(marker, digits, error = %e, "marker had no parseable float");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_instances_is_ungraded_not_zero() {
        let out = parse("", Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::Warn);
    }

    #[test]
    fn mixed_actions_aggregate_correctly() {
        let stdout = "create_file    triggered=3    nominal=2    validated=1    avg_structural_confidence=0.33\n\
delete         triggered=1    nominal=0    validated=1    avg_structural_confidence=1.00\n";
        let out = parse(stdout, Some(0));
        // total_triggered=4, total_validated=2, total_nominal=2
        // score = 100*(2/4) - 20*(2/4) = 50 - 10 = 40
        assert!((out.score.unwrap() - 40.0).abs() < 1e-9);
        assert_eq!(out.status, Status::Warn);
        assert_eq!(out.findings.len(), 2);
    }

    #[test]
    fn all_validated_no_nominal_is_ok() {
        let stdout = "deploy triggered=2 nominal=0 validated=2 avg_structural_confidence=1.00\n";
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, Some(100.0));
        assert_eq!(out.status, Status::Ok);
    }

    #[test]
    fn all_nominal_is_fail() {
        let stdout =
            "create_file triggered=5 nominal=5 validated=0 avg_structural_confidence=0.00\n";
        let out = parse(stdout, Some(0));
        // score = 100*0 - 20*1 = -20 -> clamped to 0
        assert_eq!(out.score, Some(0.0));
        assert_eq!(out.status, Status::Fail);
    }
}

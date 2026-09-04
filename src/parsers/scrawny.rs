// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! scrawny measures how review-hostile the current working-tree change is.
//! `metrics.review_load.total` is already a normalized 0-100 index, so the
//! score is its complement. Status thresholds mirror scrawny's own default
//! policy (`scrawny check`): fail above 70 review load or below 0.55
//! cohesion, warn above 40 load or more than 4 concern types. A clean tree
//! scores 100 — it measures the diff, not the project.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

const FAIL_LOAD: f64 = 70.0;
const WARN_LOAD: f64 = 40.0;
const FAIL_COHESION: f64 = 0.55;
const MAX_CONCERNS: usize = 4;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=scrawny stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let Some(load) = root.pointer("/metrics/review_load/total").and_then(Value::as_f64) else {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "scrawny JSON did not contain metrics.review_load.total".to_string(),
            findings: Vec::new(),
            note: Some("ungraded: scrawny output shape drifted from version 2".to_string()),
            raw: Some(root),
        };
    };

    let cohesion = root
        .pointer("/metrics/cohesion")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let files_changed = root
        .pointer("/metrics/files_changed")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut concerns_src = root
        .get("concerns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    concerns_src.sort_by(|a, b| {
        let la = a.get("lines").and_then(Value::as_u64).unwrap_or(0);
        let lb = b.get("lines").and_then(Value::as_u64).unwrap_or(0);
        lb.cmp(&la)
    });
    let findings = concerns_src
        .iter()
        .take(5)
        .map(|c| {
            let concern = c.get("concern").and_then(Value::as_str).unwrap_or("?");
            let lines = c.get("lines").and_then(Value::as_u64).unwrap_or(0);
            let count = c.get("count").and_then(Value::as_u64).unwrap_or(0);
            format!("{concern}: {lines} lines across {count} changes")
        })
        .collect();

    let score = clamp_score(100.0 - load);

    let status = if load > FAIL_LOAD || cohesion < FAIL_COHESION {
        Status::Fail
    } else if load > WARN_LOAD || concerns_src.len() > MAX_CONCERNS {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = format!(
        "review load {:.0}/100, cohesion {:.0}%, {} concern types across {files_changed} files",
        load,
        cohesion * 100.0,
        concerns_src.len()
    );

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: None,
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(load: f64, cohesion: f64, concerns: &str) -> String {
        format!(
            r#"{{"version": "2", "metrics": {{"review_load": {{"total": {load}, "size": 0.0, "concern_multiplicity": 0.0, "file_dispersion": 0.0, "mechanical_noise": 0.0, "behavioural_density": 0.0}}, "cohesion": {cohesion}, "lines_added": 10, "lines_removed": 5, "files_changed": 3}}, "concerns": {concerns}, "clusters": []}}"#
        )
    }

    #[test]
    fn clean_tree_scores_perfectly() {
        let outcome = parse(&fixture(0.0, 1.0, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Ok);
        assert_eq!(outcome.score, Some(100.0));
        assert!(outcome.summary.contains("review load 0/100"));
    }

    #[test]
    fn excessive_load_fails_with_complement_score() {
        let outcome = parse(&fixture(71.0, 0.9, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Fail);
        assert_eq!(outcome.score, Some(29.0));
    }

    #[test]
    fn low_cohesion_fails_even_at_moderate_load() {
        let outcome = parse(&fixture(30.0, 0.4, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Fail);
        assert_eq!(outcome.score, Some(70.0));
    }

    #[test]
    fn moderate_load_or_many_concerns_warns() {
        let outcome = parse(&fixture(50.0, 0.9, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Warn);
        let many = fixture(
            10.0,
            0.9,
            r#"[{"concern": "A", "lines": 1, "count": 1}, {"concern": "B", "lines": 1, "count": 1}, {"concern": "C", "lines": 1, "count": 1}, {"concern": "D", "lines": 1, "count": 1}, {"concern": "E", "lines": 1, "count": 1}]"#,
        );
        assert_eq!(parse(&many, Some(0)).status, Status::Warn);
    }

    #[test]
    fn concerns_are_listed_largest_first() {
        let concerns = r#"[{"concern": "Small", "lines": 3, "count": 1}, {"concern": "Big", "lines": 900, "count": 7}]"#;
        let outcome = parse(&fixture(10.0, 0.9, concerns), Some(0));
        assert_eq!(outcome.findings.len(), 2);
        assert!(outcome.findings[0].starts_with("Big: 900 lines across 7 changes"));
    }

    #[test]
    fn missing_review_load_is_ungraded() {
        let outcome = parse(r#"{"version": "2", "metrics": {}}"# , Some(0));
        assert_eq!(outcome.status, Status::Error);
        assert!(outcome.score.is_none());
        assert!(outcome.note.unwrap().contains("ungraded"));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let outcome = parse("not json", Some(1));
        assert_eq!(outcome.status, Status::Error);
        assert!(outcome.score.is_none());
    }
}

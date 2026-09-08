// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! scrawny measures how review-hostile the current working-tree change is.
//! Review difficulty describes a pending diff, not project health. Valid
//! results are advisory and ungraded; concerning metrics warn, never fail.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

const WARN_LOAD: f64 = 40.0;
const WARN_COHESION: f64 = 0.55;
const MAX_CONCERNS: usize = 4;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=scrawny stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let Some(load) = root
        .pointer("/metrics/review_load/total")
        .and_then(Value::as_f64)
    else {
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

    let status =
        if load > WARN_LOAD || cohesion < WARN_COHESION || concerns_src.len() > MAX_CONCERNS {
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
        score: None,
        summary,
        findings,
        note: Some("Advisory only: review difficulty describes the current changes, not project health; excluded from the overall score and failure cap.".to_string()),
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
    fn clean_tree_is_ok_and_ungraded() {
        let outcome = parse(&fixture(0.0, 1.0, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Ok);
        assert_eq!(outcome.score, None);
        assert!(outcome.summary.contains("review load 0/100"));
    }

    #[test]
    fn excessive_load_warns_without_a_health_score() {
        let outcome = parse(&fixture(71.0, 0.9, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Warn);
        assert_eq!(outcome.score, None);
    }

    #[test]
    fn low_cohesion_warns_without_a_health_score() {
        let outcome = parse(&fixture(30.0, 0.4, "[]"), Some(0));
        assert_eq!(outcome.status, Status::Warn);
        assert_eq!(outcome.score, None);
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
        let outcome = parse(r#"{"version": "2", "metrics": {}}"#, Some(0));
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

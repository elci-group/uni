// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! wilder establishes repository evidence and coverage; it deliberately emits
//! no health score — its contract is that an analysis gap is not a failing
//! result — so we derive one from finding severities. Coverage gaps are
//! reported in the summary but not penalized: the other orchestrated tools
//! cover those domains. Exit code 2 means the analysis is incomplete, which
//! is expected at the 0.3 milestone, so it is surfaced as a note rather than
//! treated as an error.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=wilder stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let coverage = root.get("coverage");
    let analysis_percent = coverage
        .and_then(|c| c.get("analysis_percent"))
        .and_then(Value::as_f64);
    let complete = coverage
        .and_then(|c| c.get("complete_domains"))
        .and_then(Value::as_u64);
    let applicable = coverage
        .and_then(|c| c.get("applicable_domains"))
        .and_then(Value::as_u64);

    let findings_src = root
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut critical = 0u64;
    let mut high = 0u64;
    let mut medium = 0u64;
    let mut low = 0u64;
    for finding in &findings_src {
        match finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_uppercase()
            .as_str()
        {
            "CRITICAL" => critical += 1,
            "HIGH" => high += 1,
            "MEDIUM" => medium += 1,
            "LOW" => low += 1,
            _ => {}
        }
    }

    let findings = findings_src
        .iter()
        .take(5)
        .map(|f| {
            let id = f.get("id").and_then(Value::as_str).unwrap_or("?");
            let severity = f.get("severity").and_then(Value::as_str).unwrap_or("?");
            let confidence = f.get("confidence").and_then(Value::as_str).unwrap_or("?");
            let title = f.get("title").and_then(Value::as_str).unwrap_or("");
            format!("{id} [{severity}/{confidence}] {title}")
        })
        .collect();

    let score = clamp_score(100.0 - 25.0 * critical as f64 - 10.0 * high as f64
        - 4.0 * medium as f64
        - 1.0 * low as f64);

    let status = if critical > 0 {
        Status::Fail
    } else if high > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = match (analysis_percent, complete, applicable) {
        (Some(percent), Some(complete), Some(applicable)) => format!(
            "coverage {percent:.1}% ({complete}/{applicable} domains), {critical} critical / {high} high / {medium} medium / {low} low findings"
        ),
        _ => format!(
            "{critical} critical / {high} high / {medium} medium / {low} low findings"
        ),
    };

    let mut note = None;
    if root.get("findings").is_none() {
        note = Some("wilder JSON did not contain findings; score assumed none".to_string());
    }
    if exit_code == Some(2) {
        let detail = match (complete, applicable) {
            (Some(complete), Some(applicable)) => {
                format!("analysis incomplete (exit 2): {complete}/{applicable} domains covered")
            }
            _ => "analysis incomplete (exit 2)".to_string(),
        };
        note = Some(match note {
            Some(existing) => format!("{existing}; {detail}"),
            None => detail,
        });
    }

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note,
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "schema": "wilder.schema.v1",
        "coverage": {
            "analysis_percent": 27.3,
            "complete_domains": 6,
            "applicable_domains": 22,
            "domains": []
        },
        "evidence": [{"id": "WLD-EVID-1"}, {"id": "WLD-EVID-2"}],
        "findings": [
            {"id": "WLD-A", "title": "Change hotspot", "severity": "MEDIUM", "confidence": "HIGH"},
            {"id": "WLD-B", "title": "Unsafe surface", "severity": "HIGH", "confidence": "MEDIUM"},
            {"id": "WLD-C", "title": "Oversized file", "severity": "LOW", "confidence": "HIGH"},
            {"id": "WLD-D", "title": "Info note", "severity": "INFO", "confidence": "LOW"}
        ]
    }"#;

    #[test]
    fn finding_severities_drive_score_and_status() {
        let outcome = parse(FIXTURE, Some(2));
        // 100 - 10 (high) - 4 (medium) - 1 (low); INFO ignored.
        assert_eq!(outcome.score, Some(85.0));
        assert_eq!(outcome.status, Status::Warn);
        assert!(outcome.summary.contains("27.3% (6/22 domains)"));
        assert_eq!(outcome.findings.len(), 4);
        assert!(outcome.findings[0].contains("WLD-A [MEDIUM/HIGH]"));
    }

    #[test]
    fn critical_findings_fail() {
        let fixture = r#"{"schema": "wilder.schema.v1",
            "findings": [{"id": "WLD-X", "title": "Contradiction", "severity": "CRITICAL", "confidence": "HIGH"}]}"#;
        let outcome = parse(fixture, Some(0));
        assert_eq!(outcome.status, Status::Fail);
        assert_eq!(outcome.score, Some(75.0));
    }

    #[test]
    fn exit_code_two_is_a_note_not_an_error() {
        let outcome = parse(FIXTURE, Some(2));
        assert_ne!(outcome.status, Status::Error);
        let note = outcome.note.expect("exit 2 should be explained");
        assert!(note.contains("analysis incomplete (exit 2): 6/22 domains covered"));
    }

    #[test]
    fn clean_evidence_scores_perfectly() {
        let fixture = r#"{"schema": "wilder.schema.v1", "coverage": {"analysis_percent": 100.0, "complete_domains": 22, "applicable_domains": 22}, "findings": []}"#;
        let outcome = parse(fixture, Some(0));
        assert_eq!(outcome.status, Status::Ok);
        assert_eq!(outcome.score, Some(100.0));
        assert!(outcome.note.is_none());
    }

    #[test]
    fn missing_findings_key_is_flagged_not_assumed_clean() {
        let fixture = r#"{"schema": "wilder.schema.v1", "coverage": {"analysis_percent": 50.0}}"#;
        let outcome = parse(fixture, Some(0));
        assert_eq!(outcome.score, Some(100.0));
        let note = outcome.note.expect("missing findings should be noted");
        assert!(note.contains("did not contain findings"));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let outcome = parse("not json", Some(1));
        assert_eq!(outcome.status, Status::Error);
        assert!(outcome.score.is_none());
    }
}

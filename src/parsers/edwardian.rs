// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! edwardian assesses Windows release-readiness: how much of a Linux-first
//! repository's platform-dependent surface would need to change before it
//! could ship on Windows. It scores its own findings by severity and
//! dimension itself (`readiness.overall`), so uni trusts that figure
//! directly rather than re-deriving one — the same way uni trusts wilder's
//! own domain-coverage figure: the tool's own judgment is authoritative
//! where it already provides one.
//!
//! This is edwardian's *analysis* mode only (`edwardian analyse --format
//! json`). Its revision/fork commands (`edwardian release standalone` /
//! `compatible`) mutate or scaffold a real branch and are never invoked by
//! `uni analyze` — matching how goglz's doc-rewriting mode stays opt-in.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

fn severity_rank(s: &str) -> u8 {
    match s {
        "blocker" => 0,
        "major" => 1,
        "minor" => 2,
        _ => 3,
    }
}

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=edwardian stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let readiness = root.get("readiness");
    let Some(overall) = readiness
        .and_then(|r| r.get("overall"))
        .and_then(Value::as_u64)
    else {
        return ParseOutcome {
            status: Status::NoData,
            score: None,
            summary: "edwardian reported no readiness score".to_string(),
            findings: Vec::new(),
            note: Some("readiness.overall missing from edwardian's report".to_string()),
            raw: Some(root),
        };
    };

    let status_label = readiness
        .and_then(|r| r.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown");
    let blocker_count = readiness
        .and_then(|r| r.get("blocker_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let status = match status_label {
        "BLOCKED" => Status::Fail,
        "CONDITIONAL" => Status::Warn,
        "READY" => Status::Ok,
        _ if blocker_count > 0 => Status::Fail,
        _ => Status::Warn,
    };

    let dimensions = readiness
        .and_then(|r| r.get("dimensions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let weak_dimensions = dimensions
        .iter()
        .filter(|d| d.get("score").and_then(Value::as_u64).unwrap_or(100) < 50)
        .count();

    let findings_src = root
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let directive_count = root
        .get("directives")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let mut ranked = findings_src.clone();
    ranked.sort_by_key(|f| {
        severity_rank(f.get("severity").and_then(Value::as_str).unwrap_or(""))
    });

    let findings = ranked
        .iter()
        .take(5)
        .map(|f| {
            let id = f.get("id").and_then(Value::as_str).unwrap_or("?");
            let severity = f.get("severity").and_then(Value::as_str).unwrap_or("?");
            let dependency = f.get("dependency").and_then(Value::as_str).unwrap_or("?");
            let description = f.get("description").and_then(Value::as_str).unwrap_or("");
            format!("{id} [{severity}/{dependency}] {description}")
        })
        .collect();

    let summary = format!(
        "Windows readiness {overall}/100 ({status_label}), {blocker_count} blocker(s), {weak_dimensions}/{} dimension(s) below 50, {} finding(s), {directive_count} remediation directive(s)",
        dimensions.len(),
        findings_src.len(),
    );

    ParseOutcome {
        status,
        score: Some(overall as f64),
        summary,
        findings,
        note: None,
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "tool": {"name": "edwardian", "version": "0.1.0"},
        "target": "windows",
        "readiness": {
            "target": "windows",
            "status": "CONDITIONAL",
            "overall": 62,
            "blocker_count": 0,
            "dimensions": [
                {"dimension": "filesystem", "score": 40, "finding_count": 3, "highest_severity": "major"},
                {"dimension": "process", "score": 90, "finding_count": 1, "highest_severity": "minor"}
            ]
        },
        "findings": [
            {"id": "EDW-001", "severity": "major", "dependency": "filesystem", "description": "hardcoded forward-slash path", "evidence": []},
            {"id": "EDW-002", "severity": "minor", "dependency": "process", "description": "fork() usage", "evidence": []}
        ],
        "directives": [{"id": "EDW-DIR-1"}]
    }"#;

    #[test]
    fn readiness_score_is_trusted_directly() {
        let out = parse(FIXTURE, Some(0));
        assert_eq!(out.score, Some(62.0));
        assert_eq!(out.status, Status::Warn);
        assert!(out.summary.contains("Windows readiness 62/100 (CONDITIONAL)"));
        assert!(out.summary.contains("1/2 dimension(s) below 50"));
        assert_eq!(out.findings.len(), 2);
        assert!(out.findings[0].contains("EDW-001 [major/filesystem]"));
    }

    #[test]
    fn blocked_status_fails() {
        let fixture = r#"{"readiness":{"status":"BLOCKED","overall":10,"blocker_count":2,"dimensions":[]},"findings":[]}"#;
        let out = parse(fixture, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert_eq!(out.score, Some(10.0));
    }

    #[test]
    fn ready_status_passes() {
        let fixture = r#"{"readiness":{"status":"READY","overall":100,"blocker_count":0,"dimensions":[]},"findings":[]}"#;
        let out = parse(fixture, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, Some(100.0));
    }

    #[test]
    fn missing_readiness_is_no_data() {
        let out = parse(r#"{"findings":[]}"#, Some(0));
        assert_eq!(out.status, Status::NoData);
        assert_eq!(out.score, None);
    }

    #[test]
    fn malformed_json_is_an_error() {
        let out = parse("not json", Some(1));
        assert_eq!(out.status, Status::Error);
        assert!(out.score.is_none());
    }
}

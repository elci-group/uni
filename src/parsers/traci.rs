// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=traci stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let summary = root.get("summary").cloned().unwrap_or(Value::Null);
    let files = summary.get("files").and_then(Value::as_u64).unwrap_or(0);
    let diagnostics_count = summary
        .get("diagnostics")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let info = summary.get("info").and_then(Value::as_u64).unwrap_or(0);
    let warnings = summary.get("warnings").and_then(Value::as_u64).unwrap_or(0);
    let errors = summary.get("errors").and_then(Value::as_u64).unwrap_or(0);
    let critical = summary.get("critical").and_then(Value::as_u64).unwrap_or(0);

    let status = if critical > 0 {
        Status::Fail
    } else if errors > 0 || warnings > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary_text = format!(
        "{files} files, {diagnostics_count} observability diagnostics: {critical} critical, {errors} error-severity, {warnings} warning-severity, {info} info"
    );

    let mut diagnostics = root
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let severity_rank = |s: &str| match s {
        "critical" => 0,
        "error" => 1,
        "warning" => 2,
        _ => 3,
    };
    diagnostics
        .sort_by_key(|d| severity_rank(d.get("severity").and_then(Value::as_str).unwrap_or("")));

    let findings = diagnostics
        .iter()
        .take(5)
        .map(|d| {
            let rule = d.get("rule").and_then(Value::as_str).unwrap_or("?");
            let path = d.get("path").and_then(Value::as_str).unwrap_or("?");
            let line = d.get("line").and_then(Value::as_u64).unwrap_or(0);
            let message = d.get("message").and_then(Value::as_str).unwrap_or("");
            let severity = d.get("severity").and_then(Value::as_str).unwrap_or("info");
            format!("[observability/{severity}] {rule} {path}:{line} — {message}")
        })
        .collect();

    ParseOutcome {
        status,
        score: None,
        summary: summary_text,
        findings,
        note: Some(
            "observability diagnostics are not assigned a project-health score because Traci does not expose a total opportunity/coverage denominator"
                .to_string(),
        ),
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_run_is_ok_and_ungraded_without_denominator() {
        let stdout = r#"{"diagnostics":[],"summary":{"files":5,"diagnostics":0,"info":0,"warnings":0,"errors":0,"critical":0}}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::Ok);
    }

    #[test]
    fn critical_diagnostics_fail_and_sort_first() {
        let stdout = r#"{"diagnostics":[
            {"rule":"TRC001","severity":"warning","path":"a.rs","line":1,"message":"w"},
            {"rule":"TRC007","severity":"critical","path":"b.rs","line":2,"message":"c"}
        ],"summary":{"files":2,"diagnostics":2,"info":0,"warnings":1,"errors":0,"critical":1}}"#;
        let out = parse(stdout, Some(2));
        assert_eq!(out.status, Status::Fail);
        assert_eq!(out.score, None);
        assert!(out.findings[0].contains("TRC007"));
    }

    #[test]
    fn diagnostic_count_does_not_become_a_fake_normalized_score() {
        let stdout = r#"{"diagnostics":[],"summary":{"files":1,"diagnostics":50,"info":0,"warnings":0,"errors":0,"critical":50}}"#;
        let out = parse(stdout, Some(2));
        assert_eq!(out.score, None);
    }
}

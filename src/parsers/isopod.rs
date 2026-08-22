// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! isopod's `check --json` emits a bare array of control results
//! (PASS/FAIL/WARNING/UNKNOWN). Unknown is missing evidence, not failure.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let controls: Vec<Value> = match serde_json::from_str(stdout) {
        Ok(Value::Array(a)) => a,
        Ok(other) => vec![other],
        Err(e) => {
            eprintln!("uni: tool=isopod stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let mut pass = Vec::new();
    let mut warn = Vec::new();
    let mut fail = Vec::new();
    let mut unknown = 0u64;

    for c in &controls {
        let status = c.get("status").and_then(Value::as_str).unwrap_or("UNKNOWN");
        let title = c
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        match status {
            "PASS" => pass.push(title),
            "WARNING" => warn.push(title),
            "FAIL" => fail.push(title),
            _ => unknown += 1,
        }
    }

    let assessed = pass.len() + warn.len() + fail.len();
    let total = controls.len();

    let coverage = if total > 0 {
        assessed as f64 / total as f64
    } else {
        0.0
    };
    let score = if assessed > 0 && coverage >= 0.8 {
        Some(super::clamp_score(
            (pass.len() as f64 + 0.5 * warn.len() as f64) / assessed as f64 * 100.0,
        ))
    } else {
        None
    };

    let status = if total == 0 {
        Status::NoData
    } else if exit_code == Some(1) || !fail.is_empty() {
        Status::Fail
    } else if !warn.is_empty() || unknown > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = format!(
        "{}/{total} controls assessed ({:.1}% coverage): {} pass, {} warn, {} fail, {unknown} unknown",
        assessed,
        coverage * 100.0,
        pass.len(),
        warn.len(),
        fail.len()
    );

    let mut findings: Vec<String> = fail.iter().take(5).map(|t| format!("FAIL: {t}")).collect();
    for t in warn.iter().take(5usize.saturating_sub(findings.len())) {
        findings.push(format!("WARN: {t}"));
    }

    ParseOutcome {
        status,
        score,
        summary,
        findings,
        note: if coverage < 0.8 {
            Some(format!(
                "coverage is {:.1}%; compliance is ungraded below the 80% evidence threshold, and UNKNOWN controls are not treated as failures",
                coverage * 100.0
            ))
        } else {
            None
        },
        raw: Some(Value::Array(controls)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_unknown_is_ungraded_not_zero() {
        let stdout = r#"[{"status":"UNKNOWN","title":"a"},{"status":"UNKNOWN","title":"b"}]"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::Warn);
    }

    #[test]
    fn fail_control_fails_regardless_of_exit_code() {
        let stdout = r#"[{"status":"PASS","title":"a"},{"status":"FAIL","title":"b"}]"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert_eq!(out.score, Some(50.0));
        assert!(out.findings[0].contains("FAIL: b"));
    }

    #[test]
    fn unknown_controls_reduce_coverage_not_posture() {
        let stdout = r#"[{"status":"PASS","title":"a"},{"status":"UNKNOWN","title":"b"}]"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::Warn);
    }

    #[test]
    fn all_pass_is_perfect_score() {
        let stdout = r#"[{"status":"PASS","title":"a"},{"status":"PASS","title":"b"}]"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, Some(100.0));
        assert_eq!(out.status, Status::Ok);
    }
}

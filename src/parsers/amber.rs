// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! amber emits per-dependency replaceability scores, not a project-level
//! grade. We derive one from `propose` recommendations. `security_block`
//! means Amber considers a dependency essential/unsafe to replace, so it is
//! a keep decision rather than a project-health defect.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=amber stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let total = root
        .get("total_dependencies")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let results = root
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut security_block = Vec::new();
    let mut propose = Vec::new();
    let mut proceed = 0u64;

    for dep in &results {
        let name = dep.get("crate").and_then(Value::as_str).unwrap_or("?");
        let overall = dep
            .pointer("/score/overall")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let recommendation = dep
            .pointer("/score/recommendation")
            .and_then(Value::as_str)
            .unwrap_or("");
        match recommendation {
            "security_block" => security_block.push((name.to_string(), overall)),
            "propose" => propose.push((name.to_string(), overall)),
            _ => proceed += 1,
        }
    }

    let score = if total == 0 {
        100.0
    } else {
        let penalty = propose.len() as f64 * 4.0;
        clamp_score(100.0 - penalty)
    };

    let status = if exit_code == Some(2) {
        Status::Fail
    } else if !propose.is_empty() || exit_code == Some(1) {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = format!(
        "{total} dependencies analyzed: {} keep/security-block, {} propose, {proceed} proceed",
        security_block.len(),
        propose.len(),
    );

    let mut findings = Vec::new();
    for (name, score) in security_block.iter().take(5) {
        findings.push(format!("{name}: security_block (replaceability {score})"));
    }
    for (name, score) in propose.iter().take(5usize.saturating_sub(findings.len())) {
        findings.push(format!("{name}: propose (replaceability {score})"));
    }

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

    fn dep(name: &str, overall: i64, recommendation: &str) -> String {
        format!(
            r#"{{"crate":"{name}","score":{{"overall":{overall},"recommendation":"{recommendation}"}}}}"#
        )
    }

    #[test]
    fn no_dependencies_scores_perfect() {
        let stdout = r#"{"amber_version":"0.3.0","total_dependencies":0,"results":[]}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, Some(100.0));
        assert_eq!(out.status, Status::Ok);
    }

    #[test]
    fn security_block_is_a_keep_decision_not_a_penalty() {
        let stdout = format!(
            r#"{{"amber_version":"0.3.0","total_dependencies":2,"results":[{},{}]}}"#,
            dep("anyhow", 66, "security_block"),
            dep("clap", 61, "propose"),
        );
        let out = parse(&stdout, Some(1));
        assert_eq!(out.status, Status::Warn);
        // Only the replaceable/proposed dependency is a reduction finding.
        assert!((out.score.unwrap() - 96.0).abs() < 1e-9);
        assert!(out.findings[0].contains("anyhow"));
    }

    #[test]
    fn strict_policy_violation_is_fail() {
        let stdout = r#"{"amber_version":"0.3.0","total_dependencies":0,"results":[]}"#;
        let out = parse(stdout, Some(2));
        assert_eq!(out.status, Status::Fail);
    }

    #[test]
    fn malformed_json_is_error_not_panic() {
        let out = parse("not json", Some(0));
        assert_eq!(out.status, Status::Error);
        assert!(out.score.is_none());
    }
}

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=tempcheq stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let total = root
        .get("total_actions")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let material = root
        .get("material_deviations")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let implicit = root
        .get("implicit_default")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    if total == 0 {
        return ParseOutcome {
            status: Status::NotApplicable,
            score: None,
            summary: "not applicable: no inference actions found".to_string(),
            findings: Vec::new(),
            note: Some(
                "zero observations do not establish project health and are excluded from scoring"
                    .to_string(),
            ),
            raw: Some(root),
        };
    }

    let ratio = material as f64 / total as f64;
    let score = clamp_score(100.0 - ratio * 100.0);
    let status = if ratio >= 0.3 {
        Status::Fail
    } else if material > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary = format!(
        "{total} inference actions: {material} material deviations, {implicit} implicit/default temperatures"
    );

    let findings = root
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| {
            a.get("material_deviation")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .take(5)
        .map(|a| {
            let name = a.get("name").and_then(Value::as_str).unwrap_or("?");
            let file = a.get("file").and_then(Value::as_str).unwrap_or("?");
            let line = a.get("line").and_then(Value::as_u64).unwrap_or(0);
            let verdict = a.get("verdict").and_then(Value::as_f64).unwrap_or(0.0);
            format!("{name} ({file}:{line}): verdict {verdict:+.2}")
        })
        .collect();

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

    #[test]
    fn no_actions_is_not_applicable_and_ungraded() {
        let stdout = r#"{"workspace":".","total_actions":0,"temperature_controlled":0,"implicit_default":0,"inapplicable":0,"material_deviations":0,"actions":[]}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::NotApplicable);
    }

    #[test]
    fn material_deviation_ratio_drives_score_and_fail_threshold() {
        // 3/10 = 0.3 => Fail (ratio >= 0.3)
        let stdout = r#"{"workspace":".","total_actions":10,"temperature_controlled":7,"implicit_default":0,"inapplicable":0,"material_deviations":3,"actions":[
            {"name":"x","file":"a.rs","line":1,"material_deviation":true,"verdict":0.5}
        ]}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Fail);
        assert!((out.score.unwrap() - 70.0).abs() < 1e-9);
        assert_eq!(out.findings.len(), 1);
    }

    #[test]
    fn low_ratio_is_warn_not_fail() {
        let stdout = r#"{"workspace":".","total_actions":100,"temperature_controlled":0,"implicit_default":0,"inapplicable":0,"material_deviations":1,"actions":[]}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Warn);
    }
}

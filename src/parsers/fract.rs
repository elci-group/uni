// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! fract is the one tool that already emits a 0-100 project score
//! (`summary.score`), so we use it directly rather than re-deriving one.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=fract stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let summary = root.get("summary").cloned().unwrap_or(Value::Null);
    let total = summary.get("total").and_then(Value::as_u64).unwrap_or(0);
    let excellent = summary
        .get("excellent")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let healthy = summary.get("healthy").and_then(Value::as_u64).unwrap_or(0);
    let warning = summary.get("warning").and_then(Value::as_u64).unwrap_or(0);
    let critical = summary.get("critical").and_then(Value::as_u64).unwrap_or(0);
    let score = summary.get("score").and_then(Value::as_f64);

    let status = if critical > 0 {
        Status::Fail
    } else if warning > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary_text = format!(
        "{total} modules indexed: {excellent} excellent, {healthy} healthy, {warning} warning, {critical} critical"
    );

    let mut findings_src = root
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    findings_src.sort_by(|a, b| {
        let ea = a.get("entropy").and_then(Value::as_f64).unwrap_or(0.0);
        let eb = b.get("entropy").and_then(Value::as_f64).unwrap_or(0.0);
        eb.partial_cmp(&ea).unwrap_or(std::cmp::Ordering::Equal)
    });

    let findings = findings_src
        .iter()
        .take(5)
        .map(|f| {
            let module = f.get("module").and_then(Value::as_str).unwrap_or("?");
            let entropy = f.get("entropy").and_then(Value::as_f64).unwrap_or(0.0);
            let message = f.get("message").and_then(Value::as_str).unwrap_or("");
            format!("{module}: entropy {entropy:.2} — {message}")
        })
        .collect();

    ParseOutcome {
        status,
        score,
        summary: summary_text,
        findings,
        note: if score.is_none() {
            Some("fract JSON did not contain summary.score".to_string())
        } else {
            None
        },
        raw: Some(root),
    }
}

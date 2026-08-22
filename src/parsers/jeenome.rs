// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! jeenome is opt-in and best-effort: it audits an strace log, not a
//! project, so there's no established health-score mapping for its output.
//! We parse leniently (single JSON document, or NDJSON — one object per
//! line) and report descriptively rather than grading it.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return ParseOutcome {
            status: if exit_code == Some(0) {
                Status::Ok
            } else {
                Status::Error
            },
            score: None,
            summary: "no behavioural events produced".to_string(),
            findings: Vec::new(),
            note: None,
            raw: None,
        };
    }

    let events: Vec<Value> = if let Ok(single) = serde_json::from_str::<Value>(trimmed) {
        match single {
            Value::Array(a) => a,
            other => vec![other],
        }
    } else {
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| match serde_json::from_str::<Value>(l) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::trace!(line = l, error = %e, "ndjson line did not parse as JSON");
                    None
                }
            })
            .collect()
    };

    if events.is_empty() {
        return ParseOutcome::json_error("no parseable JSON/NDJSON lines", stdout);
    }

    let summary = format!("{} behavioural event(s) analyzed", events.len());
    let findings = events
        .iter()
        .take(5)
        .map(|e| {
            e.get("summary")
                .or_else(|| e.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| e.to_string())
        })
        .collect();

    ParseOutcome {
        status: Status::Ok,
        score: None,
        summary,
        findings,
        note: Some(
            "jeenome findings are descriptive; uni does not assign a health score to them"
                .to_string(),
        ),
        raw: Some(Value::Array(events)),
    }
}

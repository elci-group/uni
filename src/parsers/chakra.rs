// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! chakra maps data flow rather than judging it. We turn that into a health
//! proxy: how much of the project it could actually analyze (file coverage)
//! and how much of the flow graph is directly observed versus guessed
//! (average flow confidence). Neither is a defect signal on its own — low
//! numbers mean "the map is incomplete," not "the code is bad."
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=chakra stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let flows = root
        .get("flows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let file_count = root
        .pointer("/metadata/file_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let analyzed = root
        .pointer("/metadata/analyzed_file_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let coverage = if file_count > 0 {
        analyzed as f64 / file_count as f64
    } else {
        1.0
    };

    let avg_confidence = if flows.is_empty() {
        1.0
    } else {
        let sum: f64 = flows
            .iter()
            .filter_map(|f| f.get("confidence").and_then(Value::as_f64))
            .sum();
        sum / flows.len() as f64
    };

    let score = clamp_score(100.0 * (0.5 * avg_confidence + 0.5 * coverage));
    let status = if coverage < 0.5 {
        Status::Warn
    } else {
        Status::Ok
    };

    let mut provenance_counts: std::collections::BTreeMap<String, usize> = Default::default();
    for f in &flows {
        let p = f
            .get("provenance")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *provenance_counts.entry(p).or_insert(0) += 1;
    }

    let summary = format!(
        "{nodes} nodes, {} flows; coverage {:.1}% ({analyzed}/{file_count} files), confidence {:.1}%",
        flows.len(),
        coverage * 100.0,
        avg_confidence * 100.0
    );

    let findings = provenance_counts
        .iter()
        .map(|(k, v)| format!("{v} flow(s) with provenance={k}"))
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

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=lwoodz stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let has_license = root
        .get("has_license_file")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let spdx_valid = root
        .get("spdx_valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let detected_license = root.get("detected_license").and_then(Value::as_str);

    let total_files = root
        .pointer("/header_coverage/total_files")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let with_header = root
        .pointer("/header_coverage/with_header")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let sampled_of_total = root
        .pointer("/header_coverage/sampled_of_total")
        .and_then(Value::as_u64);
    let header_ratio = if total_files > 0 {
        with_header as f64 / total_files as f64
    } else {
        1.0
    };

    let total_deps = root
        .pointer("/compatibility/total_deps")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let incompatible = root
        .pointer("/compatibility/incompatible")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let warnings = root
        .pointer("/compatibility/warnings")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let issues = root
        .pointer("/compatibility/issues")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut penalty = 0.0;
    if !has_license {
        penalty += 30.0;
    }
    if !spdx_valid {
        penalty += 15.0;
    }
    penalty += (1.0 - header_ratio) * 15.0;
    penalty += incompatible as f64 * 20.0;
    let warning_ratio = if total_deps > 0 {
        warnings as f64 / total_deps as f64
    } else {
        0.0
    };
    // Advisory dependency-license warnings scale with the dependency set.
    // A large tree containing many attribution notices must not look worse
    // than a missing project license or an actual incompatibility.
    penalty += warning_ratio * 20.0;
    let score = clamp_score(100.0 - penalty);

    let status = if exit_code == Some(2) || incompatible > 0 || !has_license {
        Status::Fail
    } else if exit_code == Some(1) || warnings > 0 || !spdx_valid || header_ratio < 0.5 {
        Status::Warn
    } else {
        Status::Ok
    };

    let sample_note = match sampled_of_total {
        Some(eligible) if eligible > total_files => format!(" (sampled {total_files} of {eligible} eligible)"),
        _ => String::new(),
    };
    let summary = format!(
        "license={}, spdx_valid={spdx_valid}, header coverage {with_header}/{total_files}{sample_note}, {incompatible} incompatible / {warnings} warning issues among {total_deps} deps",
        detected_license.unwrap_or("unknown")
    );

    let findings = issues
        .iter()
        .take(5)
        .map(|i| {
            let dep = i.get("dependency").and_then(Value::as_str).unwrap_or("?");
            let sev = i.get("severity").and_then(Value::as_str).unwrap_or("?");
            let reason = i.get("reason").and_then(Value::as_str).unwrap_or("");
            format!("{dep} [{sev}]: {reason}")
        })
        .collect();

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: Some(
            "advisory warning deductions are proportional to dependency count; missing licenses and incompatible dependencies remain hard findings"
                .to_string(),
        ),
        raw: Some(root),
    }
}

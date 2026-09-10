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
    // Older reports (pre-dating this field) fall back to file_count, which
    // reproduces their original all-files ratio rather than crashing.
    let supported = root
        .pointer("/metadata/supported_language_file_count")
        .and_then(Value::as_u64)
        .unwrap_or(file_count);

    // file_count == 0 means chakra had nothing to walk at all (missing
    // metadata or a genuinely empty project). supported == 0 means chakra
    // walked a real project but none of it was in a language it implements
    // (rust/javascript/typescript/python/go) — a documentation-only repo,
    // say. Neither is a defect: scoring either as 100 would reward "had
    // nothing to analyze" the same as "analyzed everything," so both are
    // no-data, matching how isopod treats zero controls.
    if file_count == 0 || supported == 0 {
        let reason = if file_count == 0 {
            "no file metadata reported"
        } else {
            "no files found in a language chakra implements (rust/javascript/typescript/python/go)"
        };
        return ParseOutcome {
            status: Status::NoData,
            score: None,
            summary: format!("{nodes} nodes, {} flows; {reason}", flows.len()),
            findings: Vec::new(),
            note: Some(format!(
                "chakra reported {reason}; analyzer completeness cannot be computed"
            )),
            raw: Some(root),
        };
    }

    // Analyzer completeness: of the files chakra recognizes as one of its
    // implemented languages, how many it actually analyzed. This is the
    // primary, actionable coverage figure — a low value means chakra
    // itself missed something it should have handled.
    let completeness = analyzed as f64 / supported as f64;
    // All-files ratio: informational context only. A repository that's
    // mostly documentation/config reports a low figure here even with
    // complete analyzer completeness — it describes the repository's
    // language mix, not a chakra defect, so it never drives status/score.
    let all_files_ratio = analyzed as f64 / file_count as f64;

    let avg_confidence = if flows.is_empty() {
        1.0
    } else {
        let sum: f64 = flows
            .iter()
            .filter_map(|f| f.get("confidence").and_then(Value::as_f64))
            .sum();
        sum / flows.len() as f64
    };

    let score = clamp_score(100.0 * (0.5 * avg_confidence + 0.5 * completeness));
    let status = if completeness < 0.5 {
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
        "{nodes} nodes, {} flows; analyzer completeness {:.1}% ({analyzed}/{supported} supported-language files), {:.1}% of all {file_count} files, confidence {:.1}%",
        flows.len(),
        completeness * 100.0,
        all_files_ratio * 100.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_files_is_no_data_not_a_perfect_score() {
        let stdout =
            r#"{"nodes":[],"flows":[],"metadata":{"file_count":0,"analyzed_file_count":0}}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::NoData);
        assert_eq!(out.score, None);
    }

    #[test]
    fn partial_coverage_scores_below_full_marks() {
        let stdout = r#"{"nodes":[{}],"flows":[{"confidence":1.0,"provenance":"static"}],"metadata":{"file_count":10,"analyzed_file_count":3,"supported_language_file_count":3}}"#;
        let out = parse(stdout, Some(0));
        // completeness 1.0 (3/3 supported files analyzed), confidence 1.0 => 100
        assert!((out.score.unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(out.status, Status::Ok);
    }

    #[test]
    fn zero_supported_language_files_is_no_data() {
        let stdout = r#"{"nodes":[],"flows":[],"metadata":{"file_count":12,"analyzed_file_count":0,"supported_language_file_count":0}}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::NoData);
        assert_eq!(out.score, None);
    }

    #[test]
    fn documentation_heavy_repo_scores_on_completeness_not_all_files_ratio() {
        // 2 of 2 supported-language files analyzed, but only 2 of 20 files
        // overall — should score as fully complete, not as low coverage.
        let stdout = r#"{"nodes":[{}],"flows":[],"metadata":{"file_count":20,"analyzed_file_count":2,"supported_language_file_count":2}}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!((out.score.unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn missing_supported_language_field_falls_back_to_file_count() {
        // Old report, pre-dating supported_language_file_count: reproduces
        // the original all-files-ratio behavior rather than erroring.
        let stdout = r#"{"nodes":[{}],"flows":[{"confidence":1.0,"provenance":"static"}],"metadata":{"file_count":10,"analyzed_file_count":3}}"#;
        let out = parse(stdout, Some(0));
        assert!((out.score.unwrap() - 65.0).abs() < 1e-9);
        assert_eq!(out.status, Status::Warn);
    }
}

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! catskin proposes deterministic, type-checked rewrites of Rust code (loop
//! -> iterator, filter-loop -> `filter().collect()`, if/else chain -> match)
//! and reports which ones actually verify. Whether a function *has* a
//! verified alternate form isn't a defect signal — a codebase with zero
//! candidates just means catskin's current rule set doesn't apply here, not
//! that it's unhealthy — so, like Bart, this is scored `None` and reported
//! informationally rather than folded into project health.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=catskin stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let candidates = root
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if candidates.is_empty() {
        return ParseOutcome {
            status: Status::NotApplicable,
            score: None,
            summary: "not applicable: no source could be lowered into catskin's process IR"
                .to_string(),
            findings: Vec::new(),
            note: Some(
                "zero observations do not establish project health and are excluded from scoring"
                    .to_string(),
            ),
            raw: Some(root),
        };
    }

    let file_count = candidates
        .iter()
        .filter_map(|c| c.get("process_id").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    let mut verified = Vec::new();
    let mut attempted = 0u64;
    for c in &candidates {
        let mutations = c.get("mutations").and_then(Value::as_array);
        let Some(mutations) = mutations.filter(|m| !m.is_empty()) else {
            continue;
        };
        attempted += 1;
        let is_valid = c
            .pointer("/verification/status/status")
            .and_then(Value::as_str)
            == Some("type_valid");
        if is_valid {
            let process = c
                .get("process_id")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .trim_start_matches("process://");
            let rules: Vec<&str> = mutations
                .iter()
                .filter_map(|m| m.get("rule_id").and_then(Value::as_str))
                .collect();
            verified.push(format!("{process}: {}", rules.join("+")));
        }
    }

    let summary = format!(
        "{file_count} file(s) parsed, {} of {attempted} attempted rewrite(s) verified equivalent",
        verified.len()
    );

    ParseOutcome {
        status: Status::Ok,
        score: None,
        summary,
        findings: verified.into_iter().take(5).collect(),
        note: Some(
            "informational only — catskin surfaces verified-equivalent rewrite candidates, not a project-health defect"
                .to_string(),
        ),
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(process_id: &str, mutations: &str, status: &str) -> String {
        format!(
            r#"{{"process_id":"{process_id}","mutations":{mutations},"verification":{{"status":{{"status":"{status}"}}}}}}"#
        )
    }

    #[test]
    fn no_candidates_is_not_applicable_and_ungraded() {
        let stdout = r#"{"candidates":[],"manifest":{}}"#;
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, None);
        assert_eq!(out.status, Status::NotApplicable);
    }

    #[test]
    fn identity_only_candidates_score_ok_with_zero_verified() {
        let stdout = format!(
            r#"{{"candidates":[{}],"manifest":{{}}}}"#,
            candidate("process://src/lib.rs", "[]", "type_valid"),
        );
        let out = parse(&stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, None);
        assert!(out.findings.is_empty());
        assert!(out.summary.contains("0 of 0"));
    }

    #[test]
    fn verified_mutation_is_a_finding_not_a_penalty() {
        let stdout = format!(
            r#"{{"candidates":[{},{}],"manifest":{{}}}}"#,
            candidate("process://src/lib.rs", "[]", "type_valid"),
            candidate(
                "process://src/lib.rs",
                r#"[{"rule_id":"rust.loop_to_iter"}]"#,
                "type_valid"
            ),
        );
        let out = parse(&stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, None);
        assert_eq!(out.findings.len(), 1);
        assert!(out.findings[0].contains("src/lib.rs"));
        assert!(out.findings[0].contains("rust.loop_to_iter"));
    }

    #[test]
    fn rejected_mutation_is_not_a_finding() {
        let stdout = format!(
            r#"{{"candidates":[{}],"manifest":{{}}}}"#,
            candidate(
                "process://src/lib.rs",
                r#"[{"rule_id":"rust.loop_to_iter"}]"#,
                "rejected"
            ),
        );
        let out = parse(&stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!(out.findings.is_empty());
        assert!(out.summary.contains("0 of 1"));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let out = parse("not json", Some(0));
        assert_eq!(out.status, Status::Error);
        assert_eq!(out.score, None);
    }
}

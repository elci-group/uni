// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! AMI is consumed only through a machine-readable `show-project` contract.
//! Older builds expose only a decorated table; the runner classifies those
//! builds as incompatible instead of scraping presentation output.

use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let Ok(raw) = serde_json::from_str::<Value>(stdout) else {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "could not parse ami JSON output".to_string(),
            findings: Vec::new(),
            note: Some("ami's declared machine interface did not return valid JSON".to_string()),
            raw: None,
        };
    };
    let profile = raw.get("project").unwrap_or(&raw);
    let Some(name) = string_field(profile, &["name"]) else {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "ami JSON omitted the project name".to_string(),
            findings: Vec::new(),
            note: Some("expected `name` or `project.name` in AMI's machine response".to_string()),
            raw: Some(raw),
        };
    };

    let description = string_field(profile, &["description"]).unwrap_or_default();
    let repository = string_field(profile, &["repository", "repository_url"]).unwrap_or_default();
    let stage = string_field(profile, &["development_stage", "stage"]).unwrap_or_default();
    let capabilities = collection_count(profile.get("capabilities"));
    let keywords = collection_count(profile.get("keywords"));

    let mut score = 0.0;
    if is_present(&description) {
        score += 25.0;
    }
    if is_present(&repository) {
        score += 15.0;
    }
    if is_present(&stage) {
        score += 10.0;
    }
    score += capabilities.min(5) as f64 / 5.0 * 25.0;
    score += keywords.min(5) as f64 / 5.0 * 25.0;
    let score = clamp_score(score);

    let mut findings = Vec::new();
    if !is_present(&description) {
        findings.push("no project description detected".to_string());
    }
    if !is_present(&repository) {
        findings.push("no repository URL detected".to_string());
    }
    if capabilities == 0 {
        findings.push("no capabilities detected".to_string());
    }
    if keywords == 0 {
        findings.push("no keywords detected".to_string());
    }

    ParseOutcome {
        status: if score < 40.0 {
            Status::Warn
        } else {
            Status::Ok
        },
        score: Some(score),
        summary: format!(
            "profile for {name}: {capabilities} capabilities, {keywords} keywords detected"
        ),
        findings,
        note: Some("score reflects project-profile completeness, not market reach".to_string()),
        raw: Some(raw),
    }
}

fn is_present(value: &str) -> bool {
    !value.is_empty() && value != "None" && value != "Unknown"
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(Value::as_str))
        .map(str::to_string)
}

fn collection_count(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_array()
                .map(|items| items.len() as u64)
                .or_else(|| value.as_u64())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_machine_shape() {
        let sample = r#"{"project":{"name":"tempcheq","description":"auditor","repository":null,"development_stage":"Alpha","capabilities":["audit","report"],"keywords":[]}}"#;
        let out = parse(sample, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!(out.summary.contains("tempcheq"));
        assert!(out.summary.contains("2 capabilities"));
        assert!(out.findings.iter().any(|f| f.contains("no repository")));
    }

    #[test]
    fn human_output_is_rejected() {
        let out = parse("▶ Project information for: /tmp/x", Some(0));
        assert_eq!(out.status, Status::Error);
        assert!(out.score.is_none());
    }
}

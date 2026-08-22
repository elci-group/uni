// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! ami has no machine-readable output at all — `show-project` prints a
//! Unicode box-drawn table, so this parser scrapes that fixed layout rather
//! than JSON. It never calls Groq or the network (that's `ami analyze`,
//! which uni does not run by default), so no gating is needed here.
//!
//! ami measures addressable market, not code health, so there's no honest
//! pass/fail signal to derive. What we score instead is profile
//! completeness — whether ami's project reader found a description,
//! repository, capabilities, and keywords — a legitimate documentation/
//! discoverability proxy, not a judgment on the project's market prospects.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let Some(name) = extract_field(stdout, "Name") else {
        return ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "could not parse ami show-project output".to_string(),
            findings: Vec::new(),
            note: Some(format!(
                "unexpected output shape; stdout began: {:?}",
                stdout.chars().take(300).collect::<String>()
            )),
            raw: None,
        };
    };

    let description = extract_field(stdout, "Description").unwrap_or_default();
    let repository = extract_field(stdout, "Repository").unwrap_or_default();
    let stage = extract_field(stdout, "Development stage").unwrap_or_default();
    let capabilities = extract_count(stdout, "Capabilities · ").unwrap_or(0);
    let keywords = extract_count(stdout, "Keywords · ").unwrap_or(0);

    let has_description = is_present(&description);
    let has_repository = is_present(&repository);
    let has_stage = is_present(&stage);

    let mut score = 0.0;
    if has_description {
        score += 25.0;
    }
    if has_repository {
        score += 15.0;
    }
    if has_stage {
        score += 10.0;
    }
    score += (capabilities.min(5) as f64 / 5.0) * 25.0;
    score += (keywords.min(5) as f64 / 5.0) * 25.0;
    let score = clamp_score(score);

    let status = if score < 40.0 {
        Status::Warn
    } else {
        Status::Ok
    };

    let summary =
        format!("profile for {name}: {capabilities} capabilities, {keywords} keywords detected");

    let mut findings = Vec::new();
    if !has_description {
        findings.push("no project description detected".to_string());
    }
    if !has_repository {
        findings.push("no repository URL detected".to_string());
    }
    if capabilities == 0 {
        findings.push("no capabilities detected".to_string());
    }
    if keywords == 0 {
        findings.push("no keywords detected".to_string());
    }

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: Some(
            "score reflects how complete ami's project profile is (description/repository/capabilities/keywords), not market reach; run `ami analyze` separately for the full discovery pipeline"
                .to_string(),
        ),
        raw: None,
    }
}

fn is_present(value: &str) -> bool {
    !value.is_empty() && value != "None" && value != "Unknown" && value != "\"Unknown\""
}

/// Pulls a value out of ami's `│ Field │ Value │` table rows.
fn extract_field(stdout: &str, field: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let cells: Vec<&str> = line.split('│').collect();
        if cells.len() >= 3 && cells[1].trim() == field {
            Some(cells[2].trim().to_string())
        } else {
            None
        }
    })
}

/// Pulls the count out of a `"<marker><N> total"` heading line.
fn extract_count(stdout: &str, marker: &str) -> Option<u64> {
    let idx = stdout.find(marker)?;
    let rest = &stdout[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse() {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::trace!(marker, digits, error = %e, "count marker had no parseable digits");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "📖 Project information for: /home/sal/tempcheq\n\
┌───────────────────┬─────────────────────────────────────────────┐\n\
│ Field             │ Value                                       │\n\
├───────────────────┼─────────────────────────────────────────────┤\n\
│ Name              │ tempcheq                                    │\n\
│ Description       │ Inference-temperature auditor and optimiser │\n\
│ Repository        │ None                                        │\n\
│ Website           │ None                                        │\n\
│ Development stage │ Alpha                                       │\n\
└───────────────────┴─────────────────────────────────────────────┘\n\
\n\
🔧 Capabilities · 12 total\n\
\n\
🔑 Keywords · 0 total\n\
(none)\n";

    #[test]
    fn parses_real_shape() {
        let out = parse(SAMPLE, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!(out.summary.contains("tempcheq"));
        assert!(out.summary.contains("12 capabilities"));
        assert!(out.findings.iter().any(|f| f.contains("no repository")));
        assert!(out.findings.iter().any(|f| f.contains("no keywords")));
    }

    #[test]
    fn empty_profile_scores_low_and_warns() {
        let stdout = "📖 Project information for: /tmp/x\n\
┌──────┬───────┐\n\
│ Field │ Value │\n\
├──────┼───────┤\n\
│ Name │ x     │\n\
│ Description │ None │\n\
│ Repository │ None │\n\
│ Development stage │ Unknown │\n\
└──────┴───────┘\n\
🔧 Capabilities · 0 total\n\
🔑 Keywords · 0 total\n";
        let out = parse(stdout, Some(0));
        assert_eq!(out.score, Some(0.0));
        assert_eq!(out.status, Status::Warn);
    }

    #[test]
    fn garbage_input_is_error_not_panic() {
        let out = parse("not an ami table", Some(0));
        assert_eq!(out.status, Status::Error);
        assert!(out.score.is_none());
    }
}

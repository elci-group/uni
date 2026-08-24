// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! The report shape. Every field here is a struct field (not a HashMap), and
//! `tools` is always built by walking `ToolId::ALL` in order — so the same
//! target, run twice, produces byte-identical JSON key/array ordering. Only
//! `generated_at` and each tool's `duration_ms` vary between runs.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Error,
    Skipped,
    Unavailable,
    NotApplicable,
    NoData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Installed,
    Installable,
    Incompatible,
    Unavailable,
    NotChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Execution {
    Succeeded,
    Failed,
    Skipped,
    NotRun,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub coverage: Option<f64>,
    pub confidence: Option<f64>,
    pub observations: Option<u64>,
}

/// Maps a 0-100 health score to a letter grade. Higher is healthier for
/// every tool's `score` by construction (parsers normalize to that
/// convention even when the underlying tool's own numbers point the other
/// way, e.g. amber's "replaceability" score).
pub fn letter_for(score: f64) -> &'static str {
    match score {
        s if s >= 97.0 => "A+",
        s if s >= 93.0 => "A",
        s if s >= 90.0 => "A-",
        s if s >= 87.0 => "B+",
        s if s >= 83.0 => "B",
        s if s >= 80.0 => "B-",
        s if s >= 77.0 => "C+",
        s if s >= 73.0 => "C",
        s if s >= 70.0 => "C-",
        s if s >= 67.0 => "D+",
        s if s >= 63.0 => "D",
        s if s >= 60.0 => "D-",
        _ => "F",
    }
}

#[derive(Debug, Serialize)]
pub struct ToolReport {
    pub tool: &'static str,
    pub purpose: &'static str,
    pub status: Status,
    pub availability: Availability,
    pub execution: Execution,
    pub evidence: Evidence,
    pub binary: Option<String>,
    pub score: Option<f64>,
    pub grade: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    pub summary: String,
    pub findings: Vec<String>,
    pub note: Option<String>,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct Overall {
    pub score: Option<f64>,
    pub grade: Option<&'static str>,
    pub graded_tools: usize,
    pub total_tools: usize,
    /// (tool key, weight) pairs, in the same fixed alphabetical order as
    /// `tools`, so weights are auditable without re-deriving them.
    pub weights: Vec<(String, f64)>,
    pub provisional: bool,
}

#[derive(Debug, Serialize)]
pub struct SuiteHealth {
    pub required_tools: usize,
    pub available_tools: usize,
    pub executed_tools: usize,
    pub valid_results: usize,
    pub analysis_coverage: Option<f64>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Healthy,
    Degraded,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct AnalysisIntegrity {
    pub status: IntegrityStatus,
    pub score: f64,
    pub grade: &'static str,
    pub defects: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub target: String,
    pub generated_at: String,
    pub tools_dir: String,
    pub tools: Vec<ToolReport>,
    pub overall: Overall,
    pub suite: SuiteHealth,
    pub integrity: AnalysisIntegrity,
}

impl Report {
    pub fn compute_overall(tools: &[ToolReport]) -> Overall {
        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;
        let mut weights = Vec::new();

        for t in tools {
            if let Some(score) = t.score {
                weights.push((t.tool.to_string(), 1.0));
                weighted_sum += score;
                weight_total += 1.0;
            }
        }

        let score = if weight_total > 0.0 {
            Some(weighted_sum / weight_total)
        } else {
            None
        };

        Overall {
            score,
            grade: score.map(letter_for),
            graded_tools: weights.len(),
            total_tools: tools.len(),
            weights,
            provisional: tools.iter().any(|t| {
                matches!(
                    t.status,
                    Status::Error | Status::Unavailable | Status::NoData
                ) || t.evidence.coverage.is_some_and(|coverage| coverage < 0.8)
            }),
        }
    }

    pub fn compute_suite(tools: &[ToolReport]) -> SuiteHealth {
        let required: Vec<_> = tools
            .iter()
            .filter(|t| !matches!(t.availability, Availability::NotChecked))
            .collect();
        let average = |values: Vec<f64>| {
            if values.is_empty() {
                None
            } else {
                Some(values.iter().sum::<f64>() / values.len() as f64)
            }
        };
        SuiteHealth {
            required_tools: required.len(),
            available_tools: required
                .iter()
                .filter(|t| matches!(t.availability, Availability::Installed))
                .count(),
            executed_tools: required
                .iter()
                .filter(|t| matches!(t.execution, Execution::Succeeded))
                .count(),
            valid_results: required
                .iter()
                .filter(|t| {
                    matches!(t.execution, Execution::Succeeded)
                        && !matches!(t.status, Status::Error)
                })
                .count(),
            analysis_coverage: average(
                required
                    .iter()
                    .filter_map(|t| t.evidence.coverage)
                    .collect(),
            ),
            confidence: average(
                required
                    .iter()
                    .filter_map(|t| t.evidence.confidence)
                    .collect(),
            ),
        }
    }

    pub fn compute_integrity(tools: &[ToolReport], suite: &SuiteHealth) -> AnalysisIntegrity {
        let defects: Vec<String> = tools
            .iter()
            .filter(|tool| {
                !matches!(tool.availability, Availability::NotChecked)
                    && (matches!(
                        tool.availability,
                        Availability::Incompatible | Availability::Unavailable
                    ) || matches!(tool.execution, Execution::Failed)
                        || matches!(tool.status, Status::Error))
            })
            .map(|tool| format!("{}: {}", tool.tool, tool.summary))
            .collect();
        let score = if suite.required_tools == 0 {
            100.0
        } else {
            suite.valid_results as f64 / suite.required_tools as f64 * 100.0
        };
        let status = if defects.is_empty() && score >= 99.95 {
            IntegrityStatus::Healthy
        } else if score >= 80.0 {
            IntegrityStatus::Degraded
        } else {
            IntegrityStatus::Failed
        };
        AnalysisIntegrity {
            status,
            score,
            grade: letter_for(score),
            defects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_boundaries() {
        assert_eq!(letter_for(100.0), "A+");
        assert_eq!(letter_for(97.0), "A+");
        assert_eq!(letter_for(96.9), "A");
        assert_eq!(letter_for(90.0), "A-");
        assert_eq!(letter_for(89.9), "B+");
        assert_eq!(letter_for(60.0), "D-");
        assert_eq!(letter_for(59.9), "F");
        assert_eq!(letter_for(0.0), "F");
    }

    fn tool_report(tool: &'static str, score: Option<f64>) -> ToolReport {
        ToolReport {
            tool,
            purpose: "test",
            status: Status::Ok,
            availability: Availability::Installed,
            execution: Execution::Succeeded,
            evidence: Evidence {
                coverage: Some(1.0),
                confidence: Some(1.0),
                observations: Some(1),
            },
            binary: None,
            score,
            grade: score.map(letter_for),
            exit_code: Some(0),
            duration_ms: Some(1),
            summary: String::new(),
            findings: Vec::new(),
            note: None,
            raw: None,
        }
    }

    #[test]
    fn overall_ignores_ungraded_tools() {
        let tools = vec![
            tool_report("amber", Some(80.0)),
            tool_report("bart", None),
            tool_report("traci", Some(100.0)),
        ];
        let overall = Report::compute_overall(&tools);
        assert_eq!(overall.graded_tools, 2);
        assert_eq!(overall.total_tools, 3);
        assert!((overall.score.unwrap() - 90.0).abs() < 1e-9);
        assert_eq!(overall.grade, Some("A-"));
    }

    #[test]
    fn overall_is_none_when_nothing_graded() {
        let tools = vec![tool_report("bart", None)];
        let overall = Report::compute_overall(&tools);
        assert_eq!(overall.score, None);
        assert_eq!(overall.grade, None);
    }

    #[test]
    fn low_evidence_coverage_marks_project_health_provisional() {
        let mut report = tool_report("isopod", None);
        report.evidence.coverage = Some(0.35);
        assert!(Report::compute_overall(&[report]).provisional);
    }
}

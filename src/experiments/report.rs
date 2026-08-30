// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Data model for `uni.experiment/v1` reports.
//!
//! An experiment captures a baseline revision, a candidate revision, the full
//! UNI analysis of each, and a differential verdict. Every experiment is
//! identified by a stable ID derived from the candidate SHA and creation time,
//! so re-running the same baseline/candidate/configuration pair produces the
//! same identity.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::report::Report;

pub const SCHEMA: &str = "uni.experiment/v1";

/// Lifecycle state of an experiment. Phase 1 only emits `Completed` and
/// `Blocked`, but the enum reserves states for future phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    Discovered,
    Queued,
    Running,
    Completed,
    Blocked,
    Expired,
    Superseded,
}

/// Source of a candidate branch. Classification is deterministic and based on
/// the branch name prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    Dependabot(DependabotMetadata),
    Agent,
    Bot,
    Automation,
    Renovate,
    Feature,
    Experiment,
    Unknown,
}

impl CandidateSource {
    pub fn display_label(&self) -> String {
        match self {
            CandidateSource::Dependabot(meta) => {
                let mut s = format!("dependabot ({})", meta.ecosystem);
                if let Some(pkg) = &meta.package {
                    s.push_str(&format!(", package={}", pkg));
                }
                if let Some(ver) = &meta.target_version {
                    s.push_str(&format!(", version={}", ver));
                }
                if let Some(group) = &meta.group {
                    s.push_str(&format!(", group={}", group));
                }
                s
            }
            CandidateSource::Agent => "agent".to_string(),
            CandidateSource::Bot => "bot".to_string(),
            CandidateSource::Automation => "automation".to_string(),
            CandidateSource::Renovate => "renovate".to_string(),
            CandidateSource::Feature => "feature".to_string(),
            CandidateSource::Experiment => "experiment".to_string(),
            CandidateSource::Unknown => "unknown".to_string(),
        }
    }
}

/// Parsed Dependabot branch metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependabotMetadata {
    pub ecosystem: String,
    pub package: Option<String>,
    pub target_version: Option<String>,
    pub group: Option<String>,
}

/// A resolved git revision, including both the symbolic name used to reach it
/// and the full SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub branch: String,
    pub sha: String,
    pub short_sha: String,
}

/// Repository identity captured for auditability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    pub path: PathBuf,
    pub remote_url: Option<String>,
}

/// Verdict levels. `Blocked` is reserved for hard-gate failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Superior,
    LikelySuperior,
    Equivalent,
    Uncertain,
    LikelyInferior,
    Inferior,
    Blocked,
}

/// Health dimensions evaluated during comparison. The order here is the stable
/// order used in reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Correctness,
    DependencyHealth,
    Security,
    Maintainability,
    Compatibility,
    Performance,
    Overall,
}

impl Dimension {
    pub const ALL: [Dimension; 7] = [
        Dimension::Correctness,
        Dimension::DependencyHealth,
        Dimension::Security,
        Dimension::Maintainability,
        Dimension::Compatibility,
        Dimension::Performance,
        Dimension::Overall,
    ];

    #[allow(dead_code)]
    pub fn key(self) -> &'static str {
        match self {
            Dimension::Correctness => "correctness",
            Dimension::DependencyHealth => "dependency_health",
            Dimension::Security => "security",
            Dimension::Maintainability => "maintainability",
            Dimension::Compatibility => "compatibility",
            Dimension::Performance => "performance",
            Dimension::Overall => "overall",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dimension::Correctness => "Correctness",
            Dimension::DependencyHealth => "Dependency health",
            Dimension::Security => "Security",
            Dimension::Maintainability => "Maintainability",
            Dimension::Compatibility => "Compatibility",
            Dimension::Performance => "Performance",
            Dimension::Overall => "Overall",
        }
    }

    /// Whether a failure on this dimension should block adoption.
    pub fn is_hard_gate(self) -> bool {
        matches!(self, Dimension::Correctness | Dimension::Security)
    }
}

/// Delta for a single dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionDelta {
    pub dimension: Dimension,
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
    pub delta: Option<f64>,
    pub baseline_grade: Option<String>,
    pub candidate_grade: Option<String>,
}

/// Delta for the aggregate score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreDelta {
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
    pub delta: Option<f64>,
}

/// A regression: a dimension that moved in the wrong direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    pub dimension: Dimension,
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
    pub delta: f64,
    pub hard_gate: bool,
    pub description: String,
}

/// An improvement: a dimension that moved in the positive direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    pub dimension: Dimension,
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
    pub delta: f64,
    pub description: String,
}

/// An unknown effect: a dimension without enough data to score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unknown {
    pub dimension: Dimension,
    pub reason: String,
}

/// Result of `cargo check` and `cargo test` correctness checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectnessValidation {
    pub cargo_check: ValidationCheck,
    pub cargo_test: ValidationCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub command: String,
    pub success: bool,
    pub duration_ms: Option<u128>,
    pub exit_code: Option<i32>,
    pub summary: String,
    pub detail: Option<String>,
}

/// The differential comparison between baseline and candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub dimensions: Vec<DimensionDelta>,
    pub overall_delta: ScoreDelta,
    pub regressions: Vec<Regression>,
    pub improvements: Vec<Improvement>,
    pub unknowns: Vec<Unknown>,
    pub confidence: f64,
    pub verdict: Verdict,
    pub decision_basis: String,
}

/// Associated GitHub pull request, if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub head_branch: String,
}

/// CI status rollup for a pull request or branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiStatus {
    pub state: String,
    pub checks: Vec<CiCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCheck {
    pub name: String,
    pub state: String,
    pub conclusion: Option<String>,
}

/// A single experiment.
#[derive(Debug, Serialize)]
pub struct Experiment {
    pub schema: &'static str,
    pub id: String,
    pub status: ExperimentStatus,
    pub repository: RepositoryIdentity,
    pub baseline: Revision,
    pub candidate: Revision,
    pub source: CandidateSource,
    pub pull_request: Option<PullRequest>,
    pub ci_status: Option<CiStatus>,
    pub created_at: String,
    pub uni_version: String,
    pub baseline_report: Report,
    pub candidate_report: Report,
    pub correctness: CorrectnessValidation,
    pub comparison: ComparisonResult,
}

impl Experiment {
    pub fn is_adoption_eligible(&self) -> bool {
        matches!(
            self.comparison.verdict,
            Verdict::Superior | Verdict::LikelySuperior
        )
    }
}

/// Top-level report emitted by `uni experiments`.
#[derive(Debug, Serialize)]
pub struct ExperimentReport {
    pub schema: &'static str,
    pub generated_at: String,
    pub baseline: Revision,
    pub experiments: Vec<Experiment>,
}

impl ExperimentReport {
    pub fn ranking(&self) -> Vec<(&Experiment, f64)> {
        let mut ranked: Vec<_> = self
            .experiments
            .iter()
            .map(|e| {
                let score = e
                    .comparison
                    .overall_delta
                    .delta
                    .unwrap_or(f64::NEG_INFINITY);
                (e, score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }

    /// The strongest adoption-eligible experiment, if any.
    pub fn best_eligible(&self) -> Option<&Experiment> {
        self.ranking()
            .into_iter()
            .map(|(e, _)| e)
            .find(|e| e.is_adoption_eligible())
    }

    /// Whether any experiment triggered a hard gate.
    pub fn any_blocked(&self) -> bool {
        self.experiments
            .iter()
            .any(|e| e.comparison.verdict == Verdict::Blocked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_keys_are_stable() {
        assert_eq!(Dimension::Correctness.key(), "correctness");
        assert_eq!(Dimension::DependencyHealth.key(), "dependency_health");
        assert_eq!(Dimension::Overall.key(), "overall");
    }

    #[test]
    fn hard_gates_are_correctness_and_security() {
        assert!(Dimension::Correctness.is_hard_gate());
        assert!(Dimension::Security.is_hard_gate());
        assert!(!Dimension::Maintainability.is_hard_gate());
        assert!(!Dimension::Performance.is_hard_gate());
    }

    #[test]
    fn ranking_orders_by_overall_delta() {
        fn experiment(delta: f64) -> Experiment {
            Experiment {
                schema: SCHEMA,
                id: "test".into(),
                status: ExperimentStatus::Completed,
                repository: RepositoryIdentity {
                    path: PathBuf::from("."),
                    remote_url: None,
                },
                baseline: Revision {
                    branch: "main".into(),
                    sha: "abc".into(),
                    short_sha: "abc".into(),
                },
                candidate: Revision {
                    branch: "candidate".into(),
                    sha: "def".into(),
                    short_sha: "def".into(),
                },
                source: CandidateSource::Unknown,
                pull_request: None,
                ci_status: None,
                created_at: String::new(),
                uni_version: String::new(),
                baseline_report: dummy_report(),
                candidate_report: dummy_report(),
                correctness: CorrectnessValidation {
                    cargo_check: dummy_check(true),
                    cargo_test: dummy_check(true),
                },
                comparison: ComparisonResult {
                    dimensions: Vec::new(),
                    overall_delta: ScoreDelta {
                        baseline: Some(80.0),
                        candidate: Some(80.0 + delta),
                        delta: Some(delta),
                    },
                    regressions: Vec::new(),
                    improvements: Vec::new(),
                    unknowns: Vec::new(),
                    confidence: 1.0,
                    verdict: Verdict::Equivalent,
                    decision_basis: String::new(),
                },
            }
        }

        let report = ExperimentReport {
            schema: SCHEMA,
            generated_at: String::new(),
            baseline: Revision {
                branch: "main".into(),
                sha: "abc".into(),
                short_sha: "abc".into(),
            },
            experiments: vec![experiment(1.0), experiment(5.0), experiment(-2.0)],
        };

        let ranked = report.ranking();
        assert!(ranked[0].1 > ranked[1].1);
        assert!(ranked[1].1 > ranked[2].1);
    }

    fn dummy_report() -> Report {
        // This only needs to serialize; use a minimal default is not available,
        // so we construct the struct manually with default-ish values.
        Report {
            schema: "uni.report/v3",
            target: ".".into(),
            generated_at: String::new(),
            tools_dir: ".".into(),
            tools: Vec::new(),
            overall: crate::report::Overall {
                score: None,
                grade: None,
                graded_tools: 0,
                total_tools: 0,
                weights: Vec::new(),
                provisional: false,
            },
            suite: crate::report::SuiteHealth {
                required_tools: 0,
                available_tools: 0,
                executed_tools: 0,
                valid_results: 0,
                analysis_coverage: None,
                confidence: None,
            },
            integrity: crate::report::AnalysisIntegrity {
                status: crate::report::IntegrityStatus::Healthy,
                score: 100.0,
                grade: "A+",
                defects: Vec::new(),
            },
        }
    }

    fn dummy_check(success: bool) -> ValidationCheck {
        ValidationCheck {
            command: "cargo check".into(),
            success,
            duration_ms: None,
            exit_code: if success { Some(0) } else { Some(101) },
            summary: String::new(),
            detail: None,
        }
    }
}

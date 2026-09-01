// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Differential analysis: extract dimension scores from baseline and candidate
//! UNI reports, compute deltas, detect regressions, and produce a verdict with
//! confidence.

use crate::experiments::report::{
    ComparisonResult, CorrectnessValidation, Dimension, DimensionDelta, ExperimentStatus,
    Improvement, Regression, ScoreDelta, Unknown, Verdict,
};
use crate::report::{IntegrityStatus, Report};

const SUPERIOR_THRESHOLD: f64 = 2.0;
const INFERIOR_THRESHOLD: f64 = -2.0;
const EQUIVALENT_BAND: f64 = 1.0;
const HARD_GATE_FAILURE_THRESHOLD: f64 = 60.0;

/// Extract dimension scores from a UNI report and correctness validation.
fn extract_dimensions(
    report: &Report,
    correctness: &CorrectnessValidation,
) -> Vec<(Dimension, f64)> {
    let mut scores = Vec::new();

    // Correctness is observed from cargo check/test, not from tool scores.
    let correctness_score = if correctness.cargo_check.success && correctness.cargo_test.success {
        Some(100.0)
    } else if !correctness.cargo_check.success || !correctness.cargo_test.success {
        Some(0.0)
    } else {
        None
    };
    if let Some(score) = correctness_score {
        scores.push((Dimension::Correctness, score));
    }

    // Dependency health: amber.
    if let Some(score) = tool_score(report, "amber") {
        scores.push((Dimension::DependencyHealth, score));
    }

    // Security: isopod, or amber if isopod is absent/ungraded.
    if let Some(score) = tool_score(report, "isopod").or_else(|| tool_score(report, "amber")) {
        scores.push((Dimension::Security, score));
    }

    // Maintainability: average of fract, traci, tempcheq.
    let maintainability_tools = ["fract", "traci", "tempcheq"];
    let maintainability_scores: Vec<f64> = maintainability_tools
        .iter()
        .filter_map(|t| tool_score(report, t))
        .collect();
    if !maintainability_scores.is_empty() {
        let avg = maintainability_scores.iter().sum::<f64>() / maintainability_scores.len() as f64;
        scores.push((Dimension::Maintainability, avg));
    }

    // Compatibility: lwoodz (license/attribution proxy).
    if let Some(score) = tool_score(report, "lwoodz") {
        scores.push((Dimension::Compatibility, score));
    }

    // Performance: not evaluated in Phase 1.

    // Overall project health.
    if let Some(score) = report.overall.score {
        scores.push((Dimension::Overall, score));
    }

    scores
}

fn tool_score(report: &Report, tool: &str) -> Option<f64> {
    report
        .tools
        .iter()
        .find(|t| t.tool == tool)
        .and_then(|t| t.score)
}

fn dimension_score(scores: &[(Dimension, f64)], dimension: Dimension) -> Option<f64> {
    scores
        .iter()
        .find(|(d, _)| *d == dimension)
        .map(|(_, s)| *s)
}

/// Compare baseline and candidate, producing a structured differential result.
pub fn compare(
    baseline_report: &Report,
    candidate_report: &Report,
    baseline_correctness: &CorrectnessValidation,
    candidate_correctness: &CorrectnessValidation,
) -> ComparisonResult {
    let baseline_scores = extract_dimensions(baseline_report, baseline_correctness);
    let candidate_scores = extract_dimensions(candidate_report, candidate_correctness);

    let mut dimensions = Vec::new();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let mut unknowns = Vec::new();

    for dimension in Dimension::ALL {
        let baseline = dimension_score(&baseline_scores, dimension);
        let candidate = dimension_score(&candidate_scores, dimension);
        let delta = match (baseline, candidate) {
            (Some(b), Some(c)) => Some(c - b),
            _ => None,
        };

        dimensions.push(DimensionDelta {
            dimension,
            baseline,
            candidate,
            delta,
            baseline_grade: baseline
                .map(crate::report::letter_for)
                .map(|g| g.to_string()),
            candidate_grade: candidate
                .map(crate::report::letter_for)
                .map(|g| g.to_string()),
        });

        if baseline.is_none() && candidate.is_none() {
            unknowns.push(Unknown {
                dimension,
                reason: "no data on either side".to_string(),
            });
            continue;
        }

        if let Some(delta) = delta {
            if delta < -EQUIVALENT_BAND {
                regressions.push(Regression {
                    dimension,
                    baseline,
                    candidate,
                    delta,
                    hard_gate: dimension.is_hard_gate(),
                    description: format!("{} regressed by {:.1} points", dimension.label(), -delta),
                });
            } else if delta > EQUIVALENT_BAND {
                improvements.push(Improvement {
                    dimension,
                    baseline,
                    candidate,
                    delta,
                    description: format!("{} improved by {:.1} points", dimension.label(), delta),
                });
            }
        } else {
            unknowns.push(Unknown {
                dimension,
                reason: "only one side has data".to_string(),
            });
        }
    }

    let overall_delta = ScoreDelta {
        baseline: baseline_report.overall.score,
        candidate: candidate_report.overall.score,
        delta: match (
            baseline_report.overall.score,
            candidate_report.overall.score,
        ) {
            (Some(b), Some(c)) => Some(c - b),
            _ => None,
        },
    };

    let hard_gate_failed = hard_gate_failure(&dimensions, &candidate_correctness);

    let baseline_integrity_failed = baseline_report.integrity.status == IntegrityStatus::Failed;
    let candidate_integrity_failed = candidate_report.integrity.status == IntegrityStatus::Failed;

    let confidence = compute_confidence(
        &dimensions,
        baseline_report,
        candidate_report,
        hard_gate_failed,
    );

    let verdict = if hard_gate_failed || candidate_integrity_failed || baseline_integrity_failed {
        Verdict::Blocked
    } else {
        determine_verdict(
            &overall_delta,
            &regressions,
            &improvements,
            &unknowns,
            confidence,
        )
    };

    let decision_basis = build_decision_basis(
        verdict,
        confidence,
        &regressions,
        &improvements,
        &unknowns,
        hard_gate_failed,
    );

    ComparisonResult {
        dimensions,
        overall_delta,
        regressions,
        improvements,
        unknowns,
        confidence,
        verdict,
        decision_basis,
    }
}

fn hard_gate_failure(
    dimensions: &[DimensionDelta],
    candidate_correctness: &CorrectnessValidation,
) -> bool {
    // Correctness failures always block.
    if !candidate_correctness.cargo_check.success || !candidate_correctness.cargo_test.success {
        return true;
    }

    // Hard-gate dimensions block when the candidate is in a failing state,
    // not merely when they regress.
    dimensions
        .iter()
        .filter(|d| d.dimension.is_hard_gate())
        .any(|d| {
            d.candidate
                .is_some_and(|score| score < HARD_GATE_FAILURE_THRESHOLD)
        })
}

fn compute_confidence(
    dimensions: &[DimensionDelta],
    baseline_report: &Report,
    candidate_report: &Report,
    hard_gate_failed: bool,
) -> f64 {
    if hard_gate_failed {
        return 1.0;
    }

    // Data completeness: dimensions (excluding overall) with data on both sides.
    let evaluable: Vec<_> = dimensions
        .iter()
        .filter(|d| d.dimension != Dimension::Overall)
        .collect();
    let with_data = evaluable
        .iter()
        .filter(|d| d.baseline.is_some() && d.candidate.is_some())
        .count();
    let completeness = if evaluable.is_empty() {
        0.0
    } else {
        with_data as f64 / evaluable.len() as f64
    };

    // Integrity factor.
    let integrity_factor = match (
        baseline_report.integrity.status,
        candidate_report.integrity.status,
    ) {
        (IntegrityStatus::Healthy, IntegrityStatus::Healthy) => 1.0,
        (IntegrityStatus::Degraded, _) | (_, IntegrityStatus::Degraded) => 0.85,
        _ => 0.6,
    };

    // Provisional flag further reduces confidence.
    let provisional_factor = if candidate_report.overall.provisional {
        0.9
    } else {
        1.0
    };

    let raw = completeness * integrity_factor * provisional_factor;
    (raw * 100.0).round() / 100.0
}

fn determine_verdict(
    overall_delta: &ScoreDelta,
    regressions: &[Regression],
    improvements: &[Improvement],
    unknowns: &[Unknown],
    confidence: f64,
) -> Verdict {
    let delta = overall_delta.delta.unwrap_or(0.0);

    if !unknowns.is_empty() && confidence < 0.7 {
        return Verdict::Uncertain;
    }

    if delta >= SUPERIOR_THRESHOLD {
        if regressions.is_empty() && confidence >= 0.8 {
            return Verdict::Superior;
        }
        return Verdict::LikelySuperior;
    }

    if delta <= INFERIOR_THRESHOLD {
        if confidence >= 0.8 {
            return Verdict::Inferior;
        }
        return Verdict::LikelyInferior;
    }

    if delta.abs() <= EQUIVALENT_BAND {
        return Verdict::Equivalent;
    }

    // Mixed small movement with regressions or improvements but not enough to
    // cross a threshold.
    if !regressions.is_empty() && improvements.is_empty() {
        return Verdict::LikelyInferior;
    }
    if !improvements.is_empty() && regressions.is_empty() {
        return Verdict::LikelySuperior;
    }

    Verdict::Uncertain
}

fn build_decision_basis(
    verdict: Verdict,
    confidence: f64,
    regressions: &[Regression],
    improvements: &[Improvement],
    unknowns: &[Unknown],
    hard_gate_failed: bool,
) -> String {
    let mut basis = String::new();
    match verdict {
        Verdict::Blocked => {
            basis.push_str("Candidate is blocked because a hard gate failed.");
        }
        Verdict::Superior => {
            basis.push_str("Candidate is superior to baseline with high confidence.");
        }
        Verdict::LikelySuperior => {
            basis.push_str(&format!(
                "Candidate is likely superior, but confidence ({:.0}%) or minor regressions warrant review.",
                confidence * 100.0
            ));
        }
        Verdict::Equivalent => {
            basis.push_str("Candidate is equivalent to baseline within the measurement band.");
        }
        Verdict::Uncertain => {
            basis.push_str(&format!(
                "Outcome is uncertain due to incomplete data or mixed signals (confidence {:.0}%).",
                confidence * 100.0
            ));
        }
        Verdict::LikelyInferior => {
            basis.push_str(&format!(
                "Candidate is likely inferior (confidence {:.0}%).",
                confidence * 100.0
            ));
        }
        Verdict::Inferior => {
            basis.push_str("Candidate is inferior to baseline.");
        }
    }

    if hard_gate_failed {
        basis.push_str(" A hard gate (correctness or security regression) was triggered.");
    }

    if !improvements.is_empty() {
        let names: Vec<_> = improvements.iter().map(|i| i.dimension.label()).collect();
        basis.push_str(&format!(" Improvements: {}.", names.join(", ")));
    }

    if !regressions.is_empty() {
        let names: Vec<_> = regressions.iter().map(|r| r.dimension.label()).collect();
        basis.push_str(&format!(" Regressions: {}.", names.join(", ")));
    }

    if !unknowns.is_empty() {
        let names: Vec<_> = unknowns.iter().map(|u| u.dimension.label()).collect();
        basis.push_str(&format!(" Unknowns: {}.", names.join(", ")));
    }

    basis
}

/// Map a verdict to the lifecycle status used when persisting the experiment.
pub fn status_for_verdict(verdict: Verdict) -> ExperimentStatus {
    match verdict {
        Verdict::Blocked => ExperimentStatus::Blocked,
        _ => ExperimentStatus::Completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiments::report::ValidationCheck;
    use crate::report::{
        AnalysisIntegrity, Availability, Evidence, Execution, Overall, Status, SuiteHealth,
        ToolReport,
    };

    fn empty_correctness(success: bool) -> CorrectnessValidation {
        CorrectnessValidation {
            cargo_check: ValidationCheck {
                command: "cargo check".into(),
                success,
                duration_ms: None,
                exit_code: Some(if success { 0 } else { 101 }),
                summary: String::new(),
                detail: None,
            },
            cargo_test: ValidationCheck {
                command: "cargo test".into(),
                success,
                duration_ms: None,
                exit_code: Some(if success { 0 } else { 101 }),
                summary: String::new(),
                detail: None,
            },
        }
    }

    fn report_with_scores(scores: &[(&'static str, f64)]) -> Report {
        let tools = scores
            .iter()
            .map(|(tool, score)| ToolReport {
                tool: *tool,
                purpose: "test",
                status: Status::Ok,
                availability: Availability::Installed,
                execution: Execution::Succeeded,
                evidence: Evidence {
                    coverage: None,
                    confidence: None,
                    observations: None,
                },
                binary: None,
                score: Some(*score),
                grade: Some(crate::report::letter_for(*score)),
                exit_code: Some(0),
                duration_ms: Some(1),
                summary: String::new(),
                findings: Vec::new(),
                note: None,
                raw: None,
            })
            .collect();

        Report {
            schema: "uni.report/v3",
            target: ".".into(),
            generated_at: String::new(),
            tools_dir: ".".into(),
            tools,
            overall: Overall {
                score: Some(
                    scores.iter().map(|(_, s)| *s).sum::<f64>() / scores.len().max(1) as f64,
                ),
                grade: None,
                graded_tools: scores.len(),
                total_tools: scores.len(),
                weights: Vec::new(),
                provisional: false,
            },
            suite: SuiteHealth {
                required_tools: 0,
                available_tools: 0,
                executed_tools: 0,
                valid_results: 0,
                analysis_coverage: None,
                confidence: None,
            },
            integrity: AnalysisIntegrity {
                status: IntegrityStatus::Healthy,
                score: 100.0,
                grade: "A+",
                defects: Vec::new(),
            },
        }
    }

    #[test]
    fn superior_when_all_dimensions_improve() {
        let baseline = report_with_scores(&[
            ("amber", 70.0),
            ("isopod", 70.0),
            ("fract", 70.0),
            ("lwoodz", 70.0),
        ]);
        let candidate = report_with_scores(&[
            ("amber", 85.0),
            ("isopod", 85.0),
            ("fract", 85.0),
            ("lwoodz", 85.0),
        ]);
        let result = compare(
            &baseline,
            &candidate,
            &empty_correctness(true),
            &empty_correctness(true),
        );
        assert!(
            matches!(result.verdict, Verdict::Superior | Verdict::LikelySuperior),
            "got {:?}",
            result.verdict
        );
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn blocked_on_correctness_failure() {
        let baseline = report_with_scores(&[("amber", 80.0)]);
        let candidate = report_with_scores(&[("amber", 90.0)]);
        let result = compare(
            &baseline,
            &candidate,
            &empty_correctness(true),
            &empty_correctness(false),
        );
        assert_eq!(result.verdict, Verdict::Blocked);
    }

    #[test]
    fn blocked_on_security_regression() {
        let baseline = report_with_scores(&[("isopod", 80.0)]);
        let candidate = report_with_scores(&[("isopod", 50.0)]);
        let result = compare(
            &baseline,
            &candidate,
            &empty_correctness(true),
            &empty_correctness(true),
        );
        assert_eq!(result.verdict, Verdict::Blocked);
    }

    #[test]
    fn equivalent_when_scores_unchanged() {
        let baseline = report_with_scores(&[
            ("amber", 80.0),
            ("isopod", 80.0),
            ("fract", 80.0),
            ("lwoodz", 80.0),
        ]);
        let candidate = report_with_scores(&[
            ("amber", 80.0),
            ("isopod", 80.0),
            ("fract", 80.0),
            ("lwoodz", 80.0),
        ]);
        let result = compare(
            &baseline,
            &candidate,
            &empty_correctness(true),
            &empty_correctness(true),
        );
        assert_eq!(result.verdict, Verdict::Equivalent);
    }

    #[test]
    fn inferior_when_overall_drops() {
        let baseline = report_with_scores(&[
            ("amber", 90.0),
            ("isopod", 90.0),
            ("fract", 90.0),
            ("lwoodz", 90.0),
        ]);
        let candidate = report_with_scores(&[
            ("amber", 70.0),
            ("isopod", 70.0),
            ("fract", 70.0),
            ("lwoodz", 70.0),
        ]);
        let result = compare(
            &baseline,
            &candidate,
            &empty_correctness(true),
            &empty_correctness(true),
        );
        assert!(
            matches!(result.verdict, Verdict::Inferior | Verdict::LikelyInferior),
            "got {:?}",
            result.verdict
        );
    }
}

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Human-readable rendering for experiment reports.

use crate::experiments::report::{Dimension, Experiment, ExperimentReport, Verdict};

pub fn human_report(report: &ExperimentReport) -> String {
    let mut out = String::new();

    out.push_str(
        "╔═══════════════════════════════════════════════════════════════════════════════╗\n",
    );
    out.push_str("║ 🧪 UNI EXPERIMENTS REPORT\n");
    out.push_str(
        "╠═══════════════════════════════════════════════════════════════════════════════╣\n",
    );
    out.push_str(&format!(
        "║ Baseline: {:<66}\n",
        format!("{} @ {}", report.baseline.branch, report.baseline.short_sha)
    ));
    out.push_str(&format!("║ Generated: {:<65}\n", report.generated_at));
    out.push_str(
        "╚═══════════════════════════════════════════════════════════════════════════════╝\n\n",
    );

    if report.experiments.is_empty() {
        out.push_str("No experiments were run.\n");
        return out;
    }

    // Summary table.
    out.push_str(
        "┌─────────────────────────────────┬────────────────┬─────────────┬────────────┐\n",
    );
    out.push_str(
        "│ CANDIDATE                       │ VERDICT        │ CONFIDENCE  │ OVERALL Δ  │\n",
    );
    out.push_str(
        "├─────────────────────────────────┼────────────────┼─────────────┼────────────┤\n",
    );

    for exp in report.ranking().iter().map(|(e, _)| *e) {
        let candidate = format!("{} @ {}", exp.candidate.branch, exp.candidate.short_sha);
        let verdict = verdict_label(exp.comparison.verdict);
        let confidence = format!("{:.0}%", exp.comparison.confidence * 100.0);
        let delta = exp
            .comparison
            .overall_delta
            .delta
            .map(|d| format!("{:+.1}", d))
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "│ {:<31} │ {:<14} │ {:<11} │ {:<10} │\n",
            truncate(&candidate, 31),
            verdict,
            confidence,
            delta
        ));
    }

    out.push_str(
        "└─────────────────────────────────┴────────────────┴─────────────┴────────────┘\n\n",
    );

    // Recommendation summary.
    if let Some(best) = report.best_eligible() {
        out.push_str("RECOMMENDATION:\n");
        out.push_str(&format!(
            "  Strongest adoption-eligible candidate: {}\n",
            best.candidate.branch
        ));
        out.push_str(&format!(
            "  Verdict: {} (confidence {:.0}%, overall Δ {:+.1})\n",
            verdict_label(best.comparison.verdict),
            best.comparison.confidence * 100.0,
            best.comparison.overall_delta.delta.unwrap_or(0.0)
        ));
        if let Some(pr) = &best.pull_request {
            out.push_str(&format!("  PR: #{} {}\n", pr.number, pr.url));
        }
        out.push('\n');
    } else if report.any_blocked() {
        out.push_str("RECOMMENDATION:\n");
        out.push_str("  At least one candidate is blocked by a hard gate. Review regressions before adoption.\n\n");
    } else {
        out.push_str("RECOMMENDATION:\n");
        out.push_str("  No candidate is adoption-eligible under current policy.\n\n");
    }

    // Detail per experiment.
    for exp in &report.experiments {
        render_experiment(&mut out, exp);
    }

    out
}

fn render_experiment(out: &mut String, exp: &Experiment) {
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str(&format!("EXPERIMENT: {}\n\n", exp.id));

    out.push_str(&format!(
        "Baseline:  {} @ {}\n",
        exp.baseline.branch, exp.baseline.short_sha
    ));
    out.push_str(&format!(
        "Candidate: {} @ {}\n",
        exp.candidate.branch, exp.candidate.short_sha
    ));
    out.push_str(&format!("Source:    {}\n", exp.source.display_label()));
    if let Some(pr) = &exp.pull_request {
        out.push_str(&format!(
            "PR:        #{} {} ({})\n",
            pr.number, pr.title, pr.url
        ));
    }
    if let Some(ci) = &exp.ci_status {
        out.push_str(&format!(
            "CI:        {} ({} checks)\n",
            ci.state,
            ci.checks.len()
        ));
    }
    out.push_str(&format!(
        "Status:    {}\n\n",
        format!("{:?}", exp.status).to_lowercase()
    ));

    out.push_str(&format!(
        "Verdict:   {}\n",
        verdict_label(exp.comparison.verdict)
    ));
    out.push_str(&format!(
        "Confidence: {:.0}%\n\n",
        exp.comparison.confidence * 100.0
    ));

    // Dimension deltas.
    out.push_str("Dimension deltas:\n");
    out.push_str("┌──────────────────────┬──────────┬──────────┬─────────┬──────────┐\n");
    out.push_str("│ Dimension            │ Baseline │ Candidate│ Δ       │ Gate     │\n");
    out.push_str("├──────────────────────┼──────────┼──────────┼─────────┼──────────┤\n");

    for d in &exp.comparison.dimensions {
        if d.dimension == Dimension::Overall {
            continue;
        }
        let name = format!("{:<20}", d.dimension.label());
        let baseline = score_str(d.baseline);
        let candidate = score_str(d.candidate);
        let delta = d
            .delta
            .map(|v| format!("{:+.1}", v))
            .unwrap_or_else(|| "—".to_string());
        let gate = if d.dimension.is_hard_gate() {
            "hard"
        } else {
            "soft"
        };
        out.push_str(&format!(
            "│ {} │ {:>8} │ {:>8} │ {:>7} │ {:>8} │\n",
            name, baseline, candidate, delta, gate
        ));
    }

    out.push_str("└──────────────────────┴──────────┴──────────┴─────────┴──────────┘\n\n");

    if !exp.comparison.improvements.is_empty() {
        out.push_str("Improvements:\n");
        for i in &exp.comparison.improvements {
            out.push_str(&format!("  + {}: {:+.1}\n", i.dimension.label(), i.delta));
        }
        out.push('\n');
    }

    if !exp.comparison.regressions.is_empty() {
        out.push_str("Regressions:\n");
        for r in &exp.comparison.regressions {
            let marker = if r.hard_gate { " [HARD GATE]" } else { "" };
            out.push_str(&format!(
                "  - {}: {:+.1}{}\n",
                r.dimension.label(),
                r.delta,
                marker
            ));
        }
        out.push('\n');
    }

    if !exp.comparison.unknowns.is_empty() {
        out.push_str("Unknowns:\n");
        for u in &exp.comparison.unknowns {
            out.push_str(&format!("  ? {}: {}\n", u.dimension.label(), u.reason));
        }
        out.push('\n');
    }

    // Correctness validation.
    out.push_str("Validation:\n");
    out.push_str(&format!(
        "  cargo check: {}\n",
        check_label(&exp.correctness.cargo_check)
    ));
    out.push_str(&format!(
        "  cargo test:  {}\n",
        check_label(&exp.correctness.cargo_test)
    ));
    out.push_str(&format!(
        "  UNI integrity: baseline={}, candidate={}\n\n",
        integrity_label(&exp.baseline_report.integrity.status),
        integrity_label(&exp.candidate_report.integrity.status)
    ));

    out.push_str("Decision basis:\n");
    for line in simple_wrap(&exp.comparison.decision_basis, 74) {
        out.push_str(&format!("  {}\n", line));
    }
    out.push('\n');
}

fn score_str(score: Option<f64>) -> String {
    score
        .map(|s| format!("{:.1}", s))
        .unwrap_or_else(|| "—".to_string())
}

fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Superior => "SUPERIOR",
        Verdict::LikelySuperior => "LIKELY_SUPERIOR",
        Verdict::Equivalent => "EQUIVALENT",
        Verdict::Uncertain => "UNCERTAIN",
        Verdict::LikelyInferior => "LIKELY_INFERIOR",
        Verdict::Inferior => "INFERIOR",
        Verdict::Blocked => "BLOCKED",
    }
}

fn check_label(check: &crate::experiments::report::ValidationCheck) -> String {
    if check.success {
        "PASS".to_string()
    } else if check.summary.starts_with("skipped:") {
        format!("SKIPPED ({})", &check.summary[8..].trim())
    } else {
        "FAIL".to_string()
    }
}

fn integrity_label(status: &crate::report::IntegrityStatus) -> &'static str {
    match status {
        crate::report::IntegrityStatus::Healthy => "HEALTHY",
        crate::report::IntegrityStatus::Degraded => "DEGRADED",
        crate::report::IntegrityStatus::Failed => "FAILED",
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        s.chars().take(max_len - 1).collect::<String>() + "…"
    }
}

fn simple_wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split("\n\n") {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiments::report::{
        CandidateSource, ComparisonResult, CorrectnessValidation, Dimension, DimensionDelta,
        Experiment, ExperimentReport, ExperimentStatus, Improvement, RepositoryIdentity, Revision,
        ScoreDelta, ValidationCheck, Verdict, SCHEMA,
    };
    use crate::report::{AnalysisIntegrity, IntegrityStatus, Overall, Report, SuiteHealth};

    fn dummy_experiment() -> Experiment {
        Experiment {
            schema: SCHEMA,
            id: "EXP-2026-00001".into(),
            status: ExperimentStatus::Completed,
            repository: RepositoryIdentity {
                path: std::path::PathBuf::from("."),
                remote_url: None,
            },
            baseline: Revision {
                branch: "main".into(),
                sha: "abc123".into(),
                short_sha: "abc123".into(),
            },
            candidate: Revision {
                branch: "dependabot/cargo/ureq-3.3.0".into(),
                sha: "def456".into(),
                short_sha: "def456".into(),
            },
            source: CandidateSource::Dependabot(crate::experiments::report::DependabotMetadata {
                ecosystem: "cargo".into(),
                package: Some("ureq".into()),
                target_version: Some("3.3.0".into()),
                group: None,
            }),
            pull_request: None,
            ci_status: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            uni_version: "0.1.9".into(),
            baseline_report: dummy_report(80.0),
            candidate_report: dummy_report(85.0),
            correctness: CorrectnessValidation {
                cargo_check: ValidationCheck {
                    command: "cargo check".into(),
                    success: true,
                    duration_ms: None,
                    exit_code: Some(0),
                    summary: "cargo check passed".into(),
                    detail: None,
                },
                cargo_test: ValidationCheck {
                    command: "cargo test".into(),
                    success: true,
                    duration_ms: None,
                    exit_code: Some(0),
                    summary: "cargo test passed".into(),
                    detail: None,
                },
            },
            comparison: ComparisonResult {
                dimensions: vec![DimensionDelta {
                    dimension: Dimension::Overall,
                    baseline: Some(80.0),
                    candidate: Some(85.0),
                    delta: Some(5.0),
                    baseline_grade: Some("B-".into()),
                    candidate_grade: Some("B".into()),
                }],
                overall_delta: ScoreDelta {
                    baseline: Some(80.0),
                    candidate: Some(85.0),
                    delta: Some(5.0),
                },
                regressions: Vec::new(),
                improvements: vec![Improvement {
                    dimension: Dimension::Overall,
                    baseline: Some(80.0),
                    candidate: Some(85.0),
                    delta: 5.0,
                    description: "overall improved".into(),
                }],
                unknowns: Vec::new(),
                confidence: 0.91,
                verdict: Verdict::LikelySuperior,
                decision_basis: "Candidate improves overall project health.".into(),
            },
        }
    }

    fn dummy_report(score: f64) -> Report {
        Report {
            schema: "uni.report/v3",
            target: ".".into(),
            generated_at: String::new(),
            tools_dir: ".".into(),
            tools: Vec::new(),
            overall: Overall {
                score: Some(score),
                grade: Some(crate::report::letter_for(score)),
                graded_tools: 1,
                total_tools: 1,
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
    fn render_includes_experiment_id_and_verdict() {
        let report = ExperimentReport {
            schema: SCHEMA,
            generated_at: "2026-01-01T00:00:00Z".into(),
            baseline: Revision {
                branch: "main".into(),
                sha: "abc123".into(),
                short_sha: "abc123".into(),
            },
            experiments: vec![dummy_experiment()],
        };

        let text = human_report(&report);
        assert!(text.contains("EXP-2026-00001"));
        assert!(text.contains("LIKELY_SUPERIOR"));
        assert!(text.contains("dependabot/cargo/ureq-3.3.0"));
    }

    #[test]
    fn truncate_short_strings_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_strings_adds_ellipsis() {
        let s = "a".repeat(40);
        let out = truncate(&s, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
    }
}

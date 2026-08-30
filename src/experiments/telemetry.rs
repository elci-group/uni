// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Structured telemetry events for `uni experiments`.
//!
//! Events are emitted as JSON lines to stderr. Each event contains identifiers
//! and small metadata rather than duplicating large analysis payloads.

use serde::Serialize;

use crate::experiments::report::{CandidateSource, Experiment, Verdict};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ExperimentDiscovered,
    ExperimentStarted,
    ExperimentAnalysisCompleted,
    ExperimentComparisonCompleted,
    ExperimentVerdictProduced,
    ExperimentBlocked,
    ExperimentAccepted,
    ExperimentRejected,
    ExperimentSuperseded,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub event: EventKind,
    pub experiment_id: String,
    pub candidate_branch: String,
    pub candidate_sha: String,
    pub baseline_branch: String,
    pub baseline_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Event {
    pub fn new(kind: EventKind, experiment_id: impl Into<String>, experiment: &Experiment) -> Self {
        Self {
            event: kind,
            experiment_id: experiment_id.into(),
            candidate_branch: experiment.candidate.branch.clone(),
            candidate_sha: experiment.candidate.sha.clone(),
            baseline_branch: experiment.baseline.branch.clone(),
            baseline_sha: experiment.baseline.sha.clone(),
            verdict: Some(experiment.comparison.verdict),
            confidence: Some(experiment.comparison.confidence),
            source: Some(source_label(&experiment.source)),
        }
    }

    pub fn discovered(
        experiment_id: impl Into<String>,
        candidate_branch: impl Into<String>,
        candidate_sha: impl Into<String>,
        baseline_branch: impl Into<String>,
        baseline_sha: impl Into<String>,
        source: &CandidateSource,
    ) -> Self {
        Self {
            event: EventKind::ExperimentDiscovered,
            experiment_id: experiment_id.into(),
            candidate_branch: candidate_branch.into(),
            candidate_sha: candidate_sha.into(),
            baseline_branch: baseline_branch.into(),
            baseline_sha: baseline_sha.into(),
            verdict: None,
            confidence: None,
            source: Some(source_label(source)),
        }
    }

    pub fn emit(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            eprintln!("uni telemetry: {json}");
        }
    }
}

fn source_label(source: &CandidateSource) -> String {
    match source {
        CandidateSource::Dependabot(meta) => {
            let mut s = format!("dependabot/{}", meta.ecosystem);
            if let Some(pkg) = &meta.package {
                s.push_str(&format!(
                    "/{}-{}",
                    pkg,
                    meta.target_version.as_deref().unwrap_or("?")
                ));
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

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! `uni experiments` — comparative branch evaluation.
//!
//! Discovers candidate branches, isolates baseline and candidate revisions in
//! git worktrees, runs the standard UNI analysis on each, and produces a
//! differential verdict.

pub mod cli;
pub mod compare;
pub mod correctness;
pub mod git;
pub mod github;
pub mod render;
pub mod report;
pub mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ingauge_gate::Admitter;

use crate::experiments::cli::ExperimentsArgs;
use crate::experiments::compare::{compare, status_for_verdict};
use crate::experiments::correctness::validate;
pub use crate::experiments::git::CandidateBranch;
use crate::experiments::git::{
    create_worktree, discover_candidates, remove_worktree, repository_identity,
    resolve_default_branch, resolve_repo_root, resolve_revision,
};
use crate::experiments::github::{ci_status, find_pull_request};
use crate::experiments::render::human_report;
use crate::experiments::report::{
    CandidateSource, Experiment, ExperimentReport, ExperimentStatus, RepositoryIdentity, Revision,
    SCHEMA,
};
use crate::experiments::telemetry::{Event, EventKind};
use crate::run;

/// List discovered candidate branches without analyzing them.
pub async fn list(args: &ExperimentsArgs) -> Result<Vec<CandidateBranch>, String> {
    let target = std::fs::canonicalize(&args.target)
        .map_err(|e| format!("target path {:?} is not accessible: {e}", args.target))?;
    let repo = resolve_repo_root(&target).await?;
    let identity = repository_identity(&repo).await;
    let baseline_branch = match &args.baseline {
        Some(b) => b.clone(),
        None => resolve_default_branch(&repo).await?,
    };
    let baseline = resolve_revision(&repo, &baseline_branch).await?;
    let discovered = discover_candidates(&repo).await?;
    let mut candidates = Vec::with_capacity(discovered.len());

    for candidate in discovered {
        match resolve_revision(&repo, &candidate.name).await {
            Ok(candidate_rev) if candidate_rev.sha == baseline.sha => {
                // Skip the baseline branch in listings.
                continue;
            }
            Ok(candidate_rev) => {
                Event::discovered(
                    experiment_id(&identity, &baseline, &candidate_rev, args),
                    &candidate.name,
                    &candidate_rev.sha,
                    &baseline.branch,
                    &baseline.sha,
                    &candidate.source,
                )
                .emit();
                candidates.push(candidate);
            }
            Err(_) => {
                // If we can't resolve a candidate, keep it and let the user decide.
                candidates.push(candidate);
            }
        }
    }

    Ok(candidates)
}

/// Run the experiments subsystem.
pub async fn run(args: &ExperimentsArgs) -> Result<ExperimentReport, String> {
    let target = std::fs::canonicalize(&args.target)
        .map_err(|e| format!("target path {:?} is not accessible: {e}", args.target))?;
    let repo = resolve_repo_root(&target).await?;
    let identity = repository_identity(&repo).await;

    let baseline_branch = match &args.baseline {
        Some(b) => b.clone(),
        None => resolve_default_branch(&repo).await?,
    };
    let baseline = resolve_revision(&repo, &baseline_branch).await?;

    let candidates = if args.list {
        let _discovered = discover_candidates(&repo).await?;
        return Ok(ExperimentReport {
            schema: SCHEMA,
            generated_at: chrono::Utc::now().to_rfc3339(),
            baseline: baseline.clone(),
            experiments: Vec::new(),
        });
    } else if !args.branch.is_empty() {
        args.branch
            .iter()
            .map(|b| CandidateBranch {
                name: b.clone(),
                source: classify_manual_branch(b),
            })
            .collect()
    } else if args.all_candidates {
        discover_candidates(&repo).await?
    } else {
        let discovered = discover_candidates(&repo).await?;
        if discovered.is_empty() {
            return Err(
                "no candidate branches discovered; pass --branch, --all-candidates, or --list"
                    .to_string(),
            );
        }
        discovered
    };

    if candidates.is_empty() {
        return Err("no candidate branches to analyze".to_string());
    }

    let admitter = Arc::new(Admitter::from_env());
    let mut experiments = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let candidate_revision = resolve_revision(&repo, &candidate.name).await?;
        if candidate_revision.sha == baseline.sha {
            eprintln!(
                "uni: experiments: skipping {} because it is the baseline revision",
                candidate.name
            );
            continue;
        }
        let experiment = run_single_experiment(
            &repo,
            &identity,
            &baseline,
            &candidate_revision,
            &candidate.source,
            args,
            Arc::clone(&admitter),
        )
        .await?;
        experiments.push(experiment);
    }

    Ok(ExperimentReport {
        schema: SCHEMA,
        generated_at: chrono::Utc::now().to_rfc3339(),
        baseline,
        experiments,
    })
}

async fn run_single_experiment(
    repo: &Path,
    identity: &RepositoryIdentity,
    baseline: &Revision,
    candidate: &Revision,
    source: &CandidateSource,
    args: &ExperimentsArgs,
    admitter: Arc<Admitter>,
) -> Result<Experiment, String> {
    let id = experiment_id(identity, baseline, candidate, args);
    let temp_base = std::env::temp_dir().join(format!("uni-experiments-{}", id));
    let baseline_worktree = temp_base.join("baseline");
    let candidate_worktree = temp_base.join("candidate");

    Event {
        event: EventKind::ExperimentStarted,
        experiment_id: id.clone(),
        candidate_branch: candidate.branch.clone(),
        candidate_sha: candidate.sha.clone(),
        baseline_branch: baseline.branch.clone(),
        baseline_sha: baseline.sha.clone(),
        verdict: None,
        confidence: None,
        source: Some(source.display_label()),
    }
    .emit();

    create_worktree(repo, &baseline_worktree, &baseline.sha).await?;
    let baseline_guard = WorktreeCleanup::new(repo.to_path_buf(), baseline_worktree.clone());

    create_worktree(repo, &candidate_worktree, &candidate.sha).await?;
    let candidate_guard = WorktreeCleanup::new(repo.to_path_buf(), candidate_worktree.clone());

    let mut baseline_opts = args.analyze_options();
    baseline_opts.target = baseline_guard.path().to_path_buf();

    let mut candidate_opts = args.analyze_options();
    candidate_opts.target = candidate_guard.path().to_path_buf();

    let baseline_report = run::execute_with_admitter(&baseline_opts, Some(Arc::clone(&admitter)))
        .await
        .map_err(|e| format!("baseline analysis failed: {e}"))?;
    let candidate_report = run::execute_with_admitter(&candidate_opts, Some(admitter))
        .await
        .map_err(|e| format!("candidate analysis failed: {e}"))?;

    let baseline_correctness = validate(baseline_guard.path(), args.timeout).await;
    let candidate_correctness = validate(candidate_guard.path(), args.timeout).await;

    let comparison = compare(
        &baseline_report,
        &candidate_report,
        &baseline_correctness,
        &candidate_correctness,
    );

    let pull_request = find_pull_request(repo, &candidate.branch).await;
    let ci_status = if let Some(pr) = &pull_request {
        ci_status(repo, &pr.head_branch).await
    } else {
        None
    };

    let status = status_for_verdict(comparison.verdict);
    let experiment = Experiment {
        schema: SCHEMA,
        id: id.clone(),
        status,
        repository: identity.clone(),
        baseline: baseline.clone(),
        candidate: candidate.clone(),
        source: source.clone(),
        pull_request,
        ci_status,
        created_at: chrono::Utc::now().to_rfc3339(),
        uni_version: env!("CARGO_PKG_VERSION").to_string(),
        baseline_report,
        candidate_report,
        correctness: candidate_correctness,
        comparison,
    };

    Event::new(EventKind::ExperimentAnalysisCompleted, &id, &experiment).emit();
    Event::new(EventKind::ExperimentComparisonCompleted, &id, &experiment).emit();
    Event::new(EventKind::ExperimentVerdictProduced, &id, &experiment).emit();
    if experiment.comparison.verdict == crate::experiments::report::Verdict::Blocked {
        Event::new(EventKind::ExperimentBlocked, &id, &experiment).emit();
    }

    persist_experiment(repo, &experiment).await;

    // Guards are dropped here, cleaning up worktrees.
    drop(baseline_guard);
    drop(candidate_guard);

    Ok(experiment)
}

fn experiment_id(
    identity: &RepositoryIdentity,
    baseline: &Revision,
    candidate: &Revision,
    args: &ExperimentsArgs,
) -> String {
    // Deterministic ID: same repository + baseline + candidate + config + UNI
    // version always produces the same experiment identity.
    let config = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        identity.path.display(),
        identity.remote_url.as_deref().unwrap_or(""),
        baseline.sha,
        candidate.sha,
        args.only.join(","),
        args.skip.join(","),
        args.timeout,
        env!("CARGO_PKG_VERSION")
    );
    let digest = md5::compute(config.as_bytes());
    format!("EXP-{digest:032x}")
}

fn classify_manual_branch(name: &str) -> CandidateSource {
    git::classify_source(name)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                eprintln!("uni: experiments: could not write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!(
            "uni: experiments: could not serialize {}: {e}",
            path.display()
        ),
    }
}

async fn persist_experiment(repo: &Path, experiment: &Experiment) {
    let dir = repo.join(".uni").join("experiments").join(&experiment.id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "uni: experiments: could not create persistence directory {}: {e}",
            dir.display()
        );
        return;
    }

    write_json(&dir.join("manifest.json"), &Manifest::from(experiment));
    write_json(
        &dir.join("baseline_report.json"),
        &experiment.baseline_report,
    );
    write_json(
        &dir.join("candidate_report.json"),
        &experiment.candidate_report,
    );
    write_json(&dir.join("comparison.json"), &experiment.comparison);
    write_json(&dir.join("verdict.json"), &VerdictRecord::from(experiment));
}

#[derive(serde::Serialize)]
struct Manifest {
    schema: &'static str,
    id: String,
    status: ExperimentStatus,
    repository: RepositoryIdentity,
    baseline: Revision,
    candidate: Revision,
    source: CandidateSource,
    created_at: String,
    uni_version: String,
}

impl From<&Experiment> for Manifest {
    fn from(e: &Experiment) -> Self {
        Self {
            schema: SCHEMA,
            id: e.id.clone(),
            status: e.status,
            repository: e.repository.clone(),
            baseline: e.baseline.clone(),
            candidate: e.candidate.clone(),
            source: e.source.clone(),
            created_at: e.created_at.clone(),
            uni_version: e.uni_version.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct VerdictRecord {
    id: String,
    verdict: crate::experiments::report::Verdict,
    confidence: f64,
    adoption_eligible: bool,
    decision_basis: String,
}

impl From<&Experiment> for VerdictRecord {
    fn from(e: &Experiment) -> Self {
        Self {
            id: e.id.clone(),
            verdict: e.comparison.verdict,
            confidence: e.comparison.confidence,
            adoption_eligible: e.is_adoption_eligible(),
            decision_basis: e.comparison.decision_basis.clone(),
        }
    }
}

struct WorktreeCleanup {
    repo: PathBuf,
    path: PathBuf,
}

impl WorktreeCleanup {
    fn new(repo: PathBuf, path: PathBuf) -> Self {
        Self { repo, path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorktreeCleanup {
    fn drop(&mut self) {
        let repo = self.repo.clone();
        let path = self.path.clone();
        tokio::spawn(async move {
            let _ = remove_worktree(&repo, &path).await;
            // Also remove the parent temp directory if empty.
            if let Some(parent) = path.parent() {
                let parent = parent.to_path_buf();
                let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir(parent)).await;
            }
        });
    }
}

/// Render an experiment report for human consumption.
pub fn render(report: &ExperimentReport) -> String {
    human_report(report)
}

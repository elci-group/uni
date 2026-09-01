// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! `--cohort` discovers every first-party elci-group repo via `gh repo
//! list` — authoritative: fork status isn't reliably derivable from local
//! git metadata, and a filesystem walk would also sweep in the dormant/
//! legacy repos this account accumulates — and runs uni's ordinary
//! per-project [`run::execute_with_admitter`] against each, in timed
//! batches rather than one large fan-out.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ingauge_gate::Admitter;
use serde::{Deserialize, Serialize};

use crate::cli::{AnalyzeOptions, CohortOptions};
use crate::report::{CohortCycle, CohortRepoEntry, CohortRepoStatus, CohortReport, CohortRollup};
use crate::{run, tool};

#[derive(Debug, Deserialize)]
struct GhRepo {
    name: String,
    #[serde(rename = "isFork")]
    is_fork: bool,
    #[serde(rename = "isArchived")]
    is_archived: bool,
}

/// A first-party repo resolved to a local path, or flagged as not checked
/// out. Fork/archive filtering already happened in [`discover`].
struct CohortTarget {
    repo: String,
    path: Option<PathBuf>,
}

/// Calls `gh repo list <org> --json name,isFork,isArchived --limit 500` and
/// filters to non-fork, non-archived repos. Resolves each surviving repo to
/// `<root>/<name>`, the same sibling-checkout convention
/// [`tool::default_tools_dir`] already uses for uni's own 12 tools.
async fn discover(org: &str, root: &Path) -> Result<Vec<CohortTarget>, String> {
    let gh = tool::resolve_binary_by_name("gh").ok_or_else(|| {
        "gh (GitHub CLI) is required for --cohort discovery and was not found on PATH".to_string()
    })?;

    let output = tokio::process::Command::new(&gh)
        .args([
            "repo",
            "list",
            org,
            "--json",
            "name,isFork,isArchived",
            "--limit",
            "500",
        ])
        .output()
        .await
        .map_err(|e| format!("failed to run gh repo list {org}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "gh repo list {org} exited {:?}: {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    let repos: Vec<GhRepo> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse gh repo list JSON: {e}"))?;

    Ok(filter_first_party(repos)
        .into_iter()
        .map(|r| resolve_target(r, root))
        .collect())
}

/// First-party: not a fork, not archived. Forks/mirrors under the same
/// account (confirmed: fork status isn't derivable from local git metadata
/// alone) and the account's dormant/archived repos are excluded here so
/// they never reach the analysis fan-out.
fn filter_first_party(repos: Vec<GhRepo>) -> Vec<GhRepo> {
    repos
        .into_iter()
        .filter(|r| !r.is_fork && !r.is_archived)
        .collect()
}

/// A repo is locally available only if `<root>/<name>/.git` exists —
/// covers both a plain clone (`.git` directory) and a worktree checkout
/// (`.git` file).
fn resolve_target(repo: GhRepo, root: &Path) -> CohortTarget {
    let candidate = root.join(&repo.name);
    let path = candidate.join(".git").exists().then_some(candidate);
    CohortTarget {
        repo: repo.name,
        path,
    }
}

fn write_json_report(path: &Path, value: &impl Serialize) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                eprintln!("uni: cohort: could not write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!(
            "uni: cohort: could not serialize report for {}: {e}",
            path.display()
        ),
    }
}

fn not_locally_available(repo: String) -> CohortRepoEntry {
    CohortRepoEntry {
        repo,
        path: None,
        report_file: None,
        status: CohortRepoStatus::NotLocallyAvailable,
        overall_score: None,
        overall_grade: None,
        integrity_status: None,
    }
}

/// Runs `run::execute_with_admitter` for every locally-available discovered
/// repo, `batch_size` at a time, pausing `cycle_seconds` between batches —
/// "no need to rush" pacing rather than firing every repo's full tool suite
/// at once. Every result is written to `<out>/<repo>.json` as it lands; the
/// final rollup goes to `<out>/summary.json`. Reuses one shared [`Admitter`]
/// across the whole run rather than building one per repo.
pub async fn run(cohort: &CohortOptions, base: &AnalyzeOptions) -> Result<CohortReport, String> {
    let root = cohort.root.clone().unwrap_or_else(tool::default_tools_dir);
    let targets = discover(&cohort.org, &root).await?;
    let discovered = targets.len();
    let locally_available = targets.iter().filter(|t| t.path.is_some()).count();

    let out_dir = cohort.out.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "uni-cohort-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ))
    });
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("could not create --cohort-out directory {out_dir:?}: {e}"))?;

    let admitter = Arc::new(Admitter::from_env());
    let mut repos: Vec<CohortRepoEntry> = Vec::with_capacity(targets.len());
    let batch_size = cohort.batch_size.max(1);
    let mut batches = targets.chunks(batch_size).peekable();

    while let Some(batch) = batches.next() {
        let mut handles = Vec::with_capacity(batch.len());
        for target in batch {
            let Some(path) = target.path.clone() else {
                repos.push(not_locally_available(target.repo.clone()));
                continue;
            };
            let mut opts = base.clone();
            opts.target = path;
            let admitter = Arc::clone(&admitter);
            let repo = target.repo.clone();
            handles.push(tokio::spawn(async move {
                (
                    repo,
                    run::execute_with_admitter(&opts, Some(admitter)).await,
                )
            }));
        }

        for handle in handles {
            match handle.await {
                Ok((repo, Ok(report))) => {
                    let file_name = format!("{repo}.json");
                    write_json_report(&out_dir.join(&file_name), &report);
                    repos.push(CohortRepoEntry {
                        repo,
                        path: Some(report.target.clone()),
                        report_file: Some(file_name),
                        status: CohortRepoStatus::Graded,
                        overall_score: report.overall.score,
                        overall_grade: report.overall.grade,
                        integrity_status: Some(report.integrity.status),
                    });
                }
                Ok((repo, Err(e))) => {
                    eprintln!("uni: cohort: {repo}: {e}");
                    repos.push(not_locally_available(repo));
                }
                Err(e) => {
                    eprintln!("uni: cohort: internal error: a repo task panicked: {e}");
                }
            }
        }

        if batches.peek().is_some() {
            tokio::time::sleep(Duration::from_secs(cohort.cycle_seconds)).await;
        }
    }

    let rollup = CohortRollup::compute(&repos);
    let report = CohortReport {
        schema: "uni.cohort/v1",
        org: cohort.org.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        discovered,
        locally_available,
        cycle: CohortCycle {
            batch_size: cohort.batch_size,
            cycle_seconds: cohort.cycle_seconds,
        },
        repos,
        rollup,
    };

    write_json_report(&out_dir.join("summary.json"), &report);

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh_repo(name: &str, is_fork: bool, is_archived: bool) -> GhRepo {
        GhRepo {
            name: name.to_string(),
            is_fork,
            is_archived,
        }
    }

    #[test]
    fn filters_out_forks_and_archived_repos() {
        let repos = vec![
            gh_repo("uni", false, false),
            gh_repo("some-fork", true, false),
            gh_repo("old-project", false, true),
        ];
        let kept = filter_first_party(repos);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "uni");
    }

    #[test]
    fn resolves_local_path_when_checkout_exists() {
        let tmp = std::env::temp_dir().join(format!("uni-cohort-test-{}", std::process::id()));
        let repo_dir = tmp.join("present");
        std::fs::create_dir_all(repo_dir.join(".git")).unwrap();

        let target = resolve_target(gh_repo("present", false, false), &tmp);
        assert_eq!(target.path, Some(repo_dir));

        let missing = resolve_target(gh_repo("absent", false, false), &tmp);
        assert_eq!(missing.path, None);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn batches_chunk_to_the_configured_size() {
        let targets: Vec<u32> = (0..10).collect();
        let batches: Vec<&[u32]> = targets.chunks(4).collect();
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 4);
        assert_eq!(batches[2].len(), 2);
    }
}

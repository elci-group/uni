// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Git operations for experiment isolation and discovery.
//!
//! All git interaction goes through the `git` binary on PATH. No native
//! libgit2 dependency is introduced.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use crate::experiments::report::{
    CandidateSource, DependabotMetadata, RepositoryIdentity, Revision,
};
use crate::tool;

/// A candidate branch discovered in the repository.
#[derive(Debug, Clone)]
pub struct CandidateBranch {
    pub name: String,
    pub source: CandidateSource,
}

/// Resolve the filesystem path to the git binary, or error.
fn require_git() -> Result<PathBuf, String> {
    tool::resolve_binary_by_name("git")
        .ok_or_else(|| "git is required for uni experiments and was not found on PATH".to_string())
}

/// Find the git repository root containing `path`.
pub async fn resolve_repo_root(path: &Path) -> Result<PathBuf, String> {
    let git = require_git()?;
    let output = Command::new(&git)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to run git rev-parse --show-toplevel: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} is not inside a git repository: {}",
            path.display(),
            stderr.trim()
        ));
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Resolve the repository's default branch. Falls back to `main` if the remote
/// HEAD cannot be determined.
pub async fn resolve_default_branch(repo: &Path) -> Result<String, String> {
    let git = require_git()?;
    let output = Command::new(&git)
        .args(["rev-parse", "--abbrev-ref", "origin/HEAD"])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to run git rev-parse origin/HEAD: {e}"))?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout)
            .trim()
            .strip_prefix("origin/")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "main".to_string());
        return Ok(branch);
    }

    // Try `main`, then `master`.
    for fallback in ["main", "master"] {
        if ref_exists(repo, fallback).await {
            return Ok(fallback.to_string());
        }
    }

    Ok("main".to_string())
}

async fn ref_exists(repo: &Path, refname: &str) -> bool {
    let Ok(git) = require_git() else {
        return false;
    };
    match Command::new(&git)
        .args(["rev-parse", "--verify", &format!("{refname}^{{commit}}")])
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Resolve a branch or ref to a full SHA and short SHA.
pub async fn resolve_revision(repo: &Path, branch: &str) -> Result<Revision, String> {
    let git = require_git()?;
    let output = Command::new(&git)
        .args(["rev-parse", "--verify", &format!("{branch}^{{commit}}")])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to resolve {branch}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("could not resolve {branch}: {}", stderr.trim()));
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let short = sha.chars().take(12).collect();
    Ok(Revision {
        branch: branch.to_string(),
        sha,
        short_sha: short,
    })
}

/// Get the remote URL of the repository, if any.
pub async fn remote_url(repo: &Path) -> Option<String> {
    let git = require_git().ok()?;
    let output = Command::new(&git)
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Build repository identity metadata.
pub async fn repository_identity(repo: &Path) -> RepositoryIdentity {
    RepositoryIdentity {
        path: repo.to_path_buf(),
        remote_url: remote_url(repo).await,
    }
}

/// Discover candidate branches in the repository matching any known source
/// prefix. Local branches are preferred; remote tracking branches are included
/// only when no local branch with the same name exists.
pub async fn discover_candidates(repo: &Path) -> Result<Vec<CandidateBranch>, String> {
    let _git = require_git()?;

    let local = list_refs(repo, "refs/heads/").await?;
    let remote = list_refs(repo, "refs/remotes/origin/").await?;

    let mut by_name: std::collections::HashMap<String, CandidateBranch> =
        std::collections::HashMap::new();

    for (name, _sha) in local {
        let source = classify_source(&name);
        by_name
            .entry(name.clone())
            .or_insert(CandidateBranch { name, source });
    }

    // Remote branches without a local counterpart are candidates, but strip the
    // `origin/` prefix for presentation.
    for (name, _sha) in remote {
        let local_name = name.strip_prefix("origin/").unwrap_or(&name);
        if by_name.contains_key(local_name) {
            continue;
        }
        let source = classify_source(local_name);
        by_name
            .entry(local_name.to_string())
            .or_insert(CandidateBranch {
                name: local_name.to_string(),
                source,
            });
    }

    let mut candidates: Vec<_> = by_name.into_values().collect();
    candidates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(candidates)
}

async fn list_refs(repo: &Path, prefix: &str) -> Result<Vec<(String, String)>, String> {
    let git = require_git()?;
    let output = Command::new(&git)
        .args(["for-each-ref", "--format=%(refname) %(objectname)", prefix])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to list {prefix} refs: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git for-each-ref failed: {}", stderr.trim()));
    }

    let mut refs = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((full_ref, sha)) = line.rsplit_once(' ') else {
            continue;
        };
        let name = full_ref
            .strip_prefix(prefix)
            .unwrap_or(full_ref)
            .to_string();
        if name.is_empty() {
            continue;
        }
        refs.push((name, sha.to_string()));
    }
    Ok(refs)
}

/// Create a detached worktree at `path` pointing at `commit`.
pub async fn create_worktree(repo: &Path, path: &Path, commit: &str) -> Result<(), String> {
    let git = require_git()?;
    let output = Command::new(&git)
        .args([
            "worktree",
            "add",
            "--detach",
            &path.to_string_lossy(),
            commit,
        ])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to create worktree at {}: {e}", path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git worktree add failed for {}: {}",
            path.display(),
            stderr.trim()
        ));
    }
    Ok(())
}

/// Remove a worktree created by [`create_worktree`].
pub async fn remove_worktree(repo: &Path, path: &Path) -> Result<(), String> {
    let git = require_git()?;
    let _ = Command::new(&git)
        .args(["worktree", "remove", "--force", &path.to_string_lossy()])
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .await;

    // Also remove the directory if git left it behind.
    if path.exists() {
        let path = path.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(path)).await;
    }
    Ok(())
}

/// Classify a branch name into a candidate source.
pub fn classify_source(name: &str) -> CandidateSource {
    let lower = name.to_lowercase();
    if lower.starts_with("dependabot/") {
        return CandidateSource::Dependabot(parse_dependabot(name));
    }
    if lower.starts_with("agent/") {
        return CandidateSource::Agent;
    }
    if lower.starts_with("bot/") {
        return CandidateSource::Bot;
    }
    if lower.starts_with("automation/") {
        return CandidateSource::Automation;
    }
    if lower.starts_with("renovate/") {
        return CandidateSource::Renovate;
    }
    if lower.starts_with("feature/") {
        return CandidateSource::Feature;
    }
    if lower.starts_with("experiment/") {
        return CandidateSource::Experiment;
    }
    CandidateSource::Unknown
}

/// Parse Dependabot branch naming conventions.
///
/// Supported forms:
/// - `dependabot/cargo/ureq-3.3.0`
/// - `dependabot/cargo/group/xyz`
fn parse_dependabot(name: &str) -> DependabotMetadata {
    let parts: Vec<&str> = name.split('/').collect();
    let ecosystem = parts.get(1).unwrap_or(&"").to_string();

    if parts.len() >= 4 && parts[2].eq_ignore_ascii_case("group") {
        return DependabotMetadata {
            ecosystem,
            package: None,
            target_version: None,
            group: Some(parts[3..].join("/")),
        };
    }

    if let Some(last) = parts.last() {
        if let Some((package, version)) = last.rsplit_once('-') {
            // Heuristic: if the trailing component looks like `name-1.2.3`,
            // treat the part after the last dash as a version when it starts
            // with a digit.
            if version
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return DependabotMetadata {
                    ecosystem: ecosystem.clone(),
                    package: Some(package.to_string()),
                    target_version: Some(version.to_string()),
                    group: None,
                };
            }
        }
    }

    DependabotMetadata {
        ecosystem,
        package: None,
        target_version: None,
        group: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_dependabot_branch() {
        let source = classify_source("dependabot/cargo/ureq-3.3.0");
        assert!(
            matches!(source, CandidateSource::Dependabot(DependabotMetadata { ecosystem, package: Some(pkg), target_version: Some(ver), .. }) if ecosystem == "cargo" && pkg == "ureq" && ver == "3.3.0")
        );
    }

    #[test]
    fn classifies_dependabot_group() {
        let source = classify_source("dependabot/cargo/group/rust-dependencies");
        assert!(
            matches!(source, CandidateSource::Dependabot(DependabotMetadata { ecosystem, group: Some(g), .. }) if ecosystem == "cargo" && g == "rust-dependencies")
        );
    }

    #[test]
    fn classifies_known_prefixes() {
        assert!(matches!(
            classify_source("agent/refactor-parser"),
            CandidateSource::Agent
        ));
        assert!(matches!(
            classify_source("bot/format"),
            CandidateSource::Bot
        ));
        assert!(matches!(
            classify_source("automation/docs"),
            CandidateSource::Automation
        ));
        assert!(matches!(
            classify_source("renovate/serde"),
            CandidateSource::Renovate
        ));
        assert!(matches!(
            classify_source("feature/oauth"),
            CandidateSource::Feature
        ));
        assert!(matches!(
            classify_source("experiment/foo"),
            CandidateSource::Experiment
        ));
        assert!(matches!(
            classify_source("random/thing"),
            CandidateSource::Unknown
        ));
    }

    #[test]
    fn dependabot_without_version_has_no_package_or_version() {
        let source = classify_source("dependabot/cargo/some-package");
        assert!(
            matches!(source, CandidateSource::Dependabot(DependabotMetadata { ecosystem, package: None, target_version: None, .. }) if ecosystem == "cargo")
        );
    }
}

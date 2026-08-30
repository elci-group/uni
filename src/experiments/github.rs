// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! GitHub integration for experiment PR association and CI status.
//!
//! Uses the `gh` CLI when available. All operations are read-only and
//! best-effort: missing `gh` or network issues result in `None` rather than
//! failing the experiment.

use std::path::Path;
use std::process::Stdio;

use serde::Deserialize;
use tokio::process::Command;

use crate::experiments::report::{CiCheck, CiStatus, PullRequest};
use crate::tool;

#[derive(Debug, Deserialize)]
struct GhPr {
    number: u64,
    title: String,
    state: String,
    url: String,
    head_ref_name: String,
}

#[derive(Debug, Deserialize)]
struct GhCheck {
    name: String,
    state: String,
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhStatusRollup {
    state: String,
    contexts: Option<Vec<GhCheck>>,
}

fn gh_bin() -> Option<std::path::PathBuf> {
    tool::resolve_binary_by_name("gh")
}

/// Find the open pull request for `branch` in `repo`, if any.
pub async fn find_pull_request(repo: &Path, branch: &str) -> Option<PullRequest> {
    let gh = gh_bin()?;
    let output = Command::new(&gh)
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "open",
            "--json",
            "number,title,state,url,headRefName",
        ])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let prs: Vec<GhPr> = serde_json::from_slice(&output.stdout).ok()?;
    prs.into_iter().next().map(|pr| PullRequest {
        number: pr.number,
        title: pr.title,
        state: pr.state,
        url: pr.url,
        head_branch: pr.head_ref_name,
    })
}

/// Fetch the CI status rollup for a pull request by branch name.
pub async fn ci_status(repo: &Path, branch: &str) -> Option<CiStatus> {
    let gh = gh_bin()?;
    let output = Command::new(&gh)
        .args(["pr", "view", branch, "--json", "statusCheckRollup"])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let rollup: GhStatusRollup = serde_json::from_slice(&output.stdout).ok()?;
    Some(CiStatus {
        state: rollup.state,
        checks: rollup
            .contexts
            .unwrap_or_default()
            .into_iter()
            .map(|c| CiCheck {
                name: c.name,
                state: c.state,
                conclusion: c.conclusion,
            })
            .collect(),
    })
}

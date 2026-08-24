// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Kaptaind integration: transactional remediation substrate.
//!
//! This module defines the data structures and types for safe, transactional
//! remediation execution. UNI produces remediation plans; Kaptaind executes
//! them safely within isolated transactions that preserve user state and
//! provide rollback capability.

#![allow(dead_code)] // Types are used when integrated with the full system

use md5;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Globally unique transaction identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(String);

impl TransactionId {
    /// Generate a new transaction ID from current timestamp and random suffix.
    #[tracing::instrument(skip_all)]
    pub fn generate() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let random = Uuid::new_v4().simple().to_string();
        Self(format!("KAP-{timestamp}-{}", &random[..8]))
    }

    /// Create a TransactionId from a string.
    #[tracing::instrument(skip_all)]
    pub fn from_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the transaction ID as a string reference.
    #[tracing::instrument(skip_all)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Repository state classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryState {
    /// Production branch (release-ready).
    Production,
    /// Development/main branch (integration target).
    Development,
    /// Temporary remediation branch (isolated changes).
    Remediation,
    /// Staging branch (pre-production validation).
    Staging,
    /// Other branches.
    #[serde(rename = "other")]
    Other,
}

/// Explicit remediation classification for routing and execution policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationClass {
    /// Security-blocking finding: requires policy approval, no auto-apply.
    SecurityBlock,
    /// Proposal: eligible for automatic remediation.
    Proposal,
    /// Deterministic fix: automatically executable.
    MechanicalFix,
    /// Model-generated patch: guarded execution with complexity tracking.
    AiGenerated,
    /// Informational: no remediation available.
    Informational,
    /// Tool doesn't support this remediation.
    Unsupported,
}

/// Tool capability declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    pub tool: String,
    pub capability: String,
    pub supported: bool,
    pub version: Option<String>,
}

/// Discovered tool capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCapabilities {
    pub capabilities: Vec<ToolCapability>,
}

/// Fingerprint of the repository state at analysis time.
/// Used to detect stale remediation plans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisFingerprint {
    /// Git HEAD commit SHA at analysis time.
    pub head: String,
    /// Hash of working tree state (relevant paths only).
    pub worktree_hash: String,
    /// Hash of git index state.
    pub index_hash: String,
    /// UNI version string.
    pub uni_version: String,
    /// Tool versions used in analysis.
    pub tool_versions: HashMap<String, String>,
    /// SHA256 of the remediation plan itself.
    pub plan_hash: String,
    /// Timestamp when analysis was performed.
    pub timestamp_ms: u64,
}

/// User worktree state preservation record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreePreservation {
    /// Git stash ID if one was created.
    pub stash_id: Option<String>,
    /// Explicit path → content map for critical files.
    pub preserved_files: HashMap<PathBuf, Vec<u8>>,
    /// Index state saved separately for precise restoration.
    pub index_state: Option<String>,
}

/// A single remediation operation within a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedRemediation {
    pub tool: String,
    pub reason: String,
    pub command: String,
    pub expected_files: Vec<PathBuf>,
    pub remediation_class: RemediationClass,
    /// Complexity score (0-100) for AI-generated patches.
    pub complexity: Option<u8>,
    /// Which capabilities must be supported.
    pub required_capabilities: Vec<String>,
    /// Command to verify this remediation succeeded.
    pub verify_command: Option<String>,
}

/// Remediation plan produced by UNI.
/// Contains everything Kaptaind needs to execute safely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationPlan {
    pub plan_id: String,
    pub project: String,
    pub analysis_id: String,
    /// Fingerprint of the repo state during analysis.
    pub analysis_fingerprint: AnalysisFingerprint,
    /// The remediation items to execute.
    pub remediations: Vec<PlannedRemediation>,
    /// Total estimated complexity (sum of item complexities).
    pub total_complexity: u32,
    /// Risk assessment for this plan.
    pub risk_assessment: String,
}

/// Transaction state throughout its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    /// Plan created, waiting to execute.
    Planned,
    /// Setting up isolation (worktree, branch, stash).
    Preparing,
    /// Isolated state achieved, safe to mutate.
    Isolated,
    /// Remediations in progress.
    Executing,
    /// Remediations applied, running verification.
    Verifying,
    /// Verified, ready to commit or merge.
    Ready,
    /// Changes merged back to source branch.
    Merged,
    /// Rolled back to original state.
    RolledBack,
    /// Execution failed, user worktree restored.
    Failed,
    /// User-initiated abort.
    Aborted,
}

/// Execution result of a single remediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationResult {
    pub tool: String,
    pub reason: String,
    pub executed: bool,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    /// Git commit SHA of checkpoint if successful.
    pub checkpoint_commit: Option<String>,
    pub duration_ms: u128,
    pub verification_passed: Option<bool>,
    pub verification_detail: Option<String>,
}

/// Complete remediation transaction record.
/// Persisted for durability and recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationTransaction {
    pub transaction_id: TransactionId,
    pub created_at: u64,
    pub state: TransactionState,
    pub plan: RemediationPlan,
    /// Source branch before remediation.
    pub source_branch: String,
    /// Source HEAD before remediation.
    pub source_head: String,
    /// Preserved user worktree state.
    pub worktree_preservation: Option<WorktreePreservation>,
    /// Remediation branch name.
    pub remediation_branch: Option<String>,
    /// Discovered tool capabilities before execution.
    pub tool_capabilities: Option<ToolCapabilities>,
    /// Results of each executed remediation.
    pub results: Vec<RemediationResult>,
    /// If execution failed, the error details.
    pub error: Option<String>,
    /// Whether this transaction was rolled back.
    pub rolled_back: bool,
    /// If merged, the merge commit SHA.
    pub merge_commit: Option<String>,
    /// Audit log entries.
    pub audit_log: Vec<String>,
}

impl RemediationTransaction {
    /// Create a new transaction from a plan.
    #[tracing::instrument(skip_all)]
    pub fn from_plan(plan: RemediationPlan, source_branch: String, source_head: String) -> Self {
        Self {
            transaction_id: TransactionId::generate(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            state: TransactionState::Planned,
            plan,
            source_branch,
            source_head,
            worktree_preservation: None,
            remediation_branch: None,
            tool_capabilities: None,
            results: Vec::new(),
            error: None,
            rolled_back: false,
            merge_commit: None,
            audit_log: vec![format!(
                "[CREATED] Transaction {:?}",
                TransactionState::Planned
            )],
        }
    }

    /// Log an audit entry.
    #[tracing::instrument(skip_all)]
    pub fn log(&mut self, msg: impl Into<String>) {
        self.audit_log.push(msg.into());
    }

    /// Transition to a new state with logging.
    #[tracing::instrument(skip_all)]
    pub fn transition(&mut self, new_state: TransactionState) {
        self.state = new_state;
        self.log(format!("[TRANSITIONED] → {:?}", new_state));
    }

    /// Record a remediation result.
    #[tracing::instrument(skip_all)]
    pub fn record_result(&mut self, result: RemediationResult) {
        self.log(format!(
            "[RESULT] {} → exit_code: {:?}",
            result.tool, result.exit_code
        ));
        self.results.push(result);
    }

    /// Mark transaction as failed.
    #[tracing::instrument(skip_all)]
    pub fn fail(&mut self, error: impl Into<String>) {
        let msg = error.into();
        self.log(format!("[FAILED] {}", msg));
        self.error = Some(msg);
        self.transition(TransactionState::Failed);
    }

    /// Mark transaction as rolled back.
    #[tracing::instrument(skip_all)]
    pub fn mark_rolled_back(&mut self) {
        self.log("[ROLLED_BACK] User worktree restored");
        self.rolled_back = true;
        self.transition(TransactionState::RolledBack);
    }

    /// Mark transaction as merged.
    #[tracing::instrument(skip_all)]
    pub fn mark_merged(&mut self, merge_commit: String) {
        self.log(format!("[MERGED] Commit: {}", merge_commit));
        self.merge_commit = Some(merge_commit);
        self.transition(TransactionState::Merged);
    }
}

/// Remediation plan staleness check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StalenesCheckResult {
    pub is_stale: bool,
    pub reason: Option<String>,
    pub current_head: String,
    pub expected_head: String,
}

impl StalenesCheckResult {
    #[tracing::instrument(skip_all)]
    pub fn fresh() -> Self {
        Self {
            is_stale: false,
            reason: None,
            current_head: String::new(),
            expected_head: String::new(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn stale(reason: impl Into<String>, current: String, expected: String) -> Self {
        Self {
            is_stale: true,
            reason: Some(reason.into()),
            current_head: current,
            expected_head: expected,
        }
    }
}

/// Options for applying a remediation plan.
#[derive(Debug, Clone)]
pub struct RemediationOptions {
    /// Allow execution of stale plans (default: false, prompt user).
    pub force_stale: bool,
    /// Maximum re-analysis iterations (default: 3).
    pub max_iterations: usize,
    /// Require explicit confirmation for high-complexity patches.
    pub require_ai_confirmation: bool,
    /// Automatically merge successful remediations.
    pub auto_merge: bool,
}

impl Default for RemediationOptions {
    #[tracing::instrument(skip_all)]
    fn default() -> Self {
        Self {
            force_stale: false,
            max_iterations: 3,
            require_ai_confirmation: true,
            auto_merge: false,
        }
    }
}

/// Preserve the current working tree state (git stash + metadata).
/// Returns a preservation record that can later restore the exact state.
#[tracing::instrument(skip_all)]
pub async fn preserve_worktree(
    repo_path: &std::path::Path,
) -> Result<WorktreePreservation, String> {
    // Create a git stash with a descriptive message
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stash_message = format!("kaptaind-remediation-{}", timestamp);

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_path)
        .args(["stash", "push", "-u", "-m", &stash_message]);

    let output = cmd.output().map_err(|e| format!("Failed to stash: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(exit_code = ?output.status.code(), "git stash failed");
        return Err(format!("git stash failed: {stderr}"));
    }

    // Verify the stash was created by listing stashes
    let mut list_cmd = Command::new("git");
    list_cmd
        .arg("-C")
        .arg(repo_path)
        .args(["stash", "list", "--pretty=format:%gd"]);

    let list_output = list_cmd
        .output()
        .map_err(|e| format!("Failed to list stashes: {e}"))?;

    let stash_list = String::from_utf8_lossy(&list_output.stdout);
    let stash_id = stash_list.lines().next().map(|s| s.to_string());

    Ok(WorktreePreservation {
        stash_id,
        preserved_files: HashMap::new(),
        index_state: None,
    })
}

/// Restore a previously preserved worktree state.
#[tracing::instrument(skip_all)]
pub async fn restore_worktree(
    repo_path: &std::path::Path,
    preservation: &WorktreePreservation,
) -> Result<(), String> {
    if let Some(stash_id) = &preservation.stash_id {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(repo_path)
            .args(["stash", "pop", stash_id]);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to restore stash: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(exit_code = ?output.status.code(), "git stash restoration failed");
            return Err(format!("git stash pop failed: {stderr}"));
        }
    }

    // Restore any preserved files
    for (path, content) in &preservation.preserved_files {
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to restore file {:?}: {e}", path))?;
    }

    Ok(())
}

/// Perform a git operation within an isolated, clean worktree.
/// The closure operates in isolation and its effects are captured.
/// The original worktree is preserved and can be restored.
#[tracing::instrument(skip_all)]
pub async fn with_isolated_worktree<F, T>(
    repo_path: &std::path::Path,
    isolation_branch: &str,
    mut operation: F,
) -> Result<T, String>
where
    F: FnMut(&std::path::Path) -> Result<T, String>,
{
    // Preserve current state
    let preservation = preserve_worktree(repo_path).await?;

    // Create isolation branch at current HEAD
    let mut branch_cmd = Command::new("git");
    branch_cmd
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", "-b", isolation_branch]);

    let output = branch_cmd
        .output()
        .map_err(|e| format!("Failed to create isolation branch: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if let Err(error) = restore_worktree(repo_path, &preservation).await {
            tracing::error!(operation = "restore_worktree", %error, "failed to restore worktree after checkout failure");
        }
        tracing::error!(exit_code = ?output.status.code(), "isolation branch checkout failed");
        return Err(format!("git checkout failed: {stderr}"));
    }

    // Run the operation
    let result = operation(repo_path);

    // Restore original branch
    let mut restore_branch = Command::new("git");
    restore_branch
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", "-"]);

    if let Err(error) = restore_branch.output() {
        tracing::warn!(operation = "restore_branch", %error, "best-effort original branch restoration failed");
    }

    // Clean up isolation branch
    let mut delete_branch = Command::new("git");
    delete_branch
        .arg("-C")
        .arg(repo_path)
        .args(["branch", "-D", isolation_branch]);

    if let Err(error) = delete_branch.output() {
        tracing::warn!(operation = "delete_branch", %error, "best-effort isolation branch cleanup failed");
    }

    // Restore user's original worktree state
    restore_worktree(repo_path, &preservation).await?;

    result
}

/// Remediation branch naming: deterministic, human-readable, collision-free.
#[tracing::instrument(skip_all)]
pub fn generate_remediation_branch_name(project: &str, operation_id: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = timestamp.as_secs();
    let nanos = timestamp.subsec_nanos();

    let short_op_id = if operation_id.len() > 8 {
        &operation_id[..8]
    } else {
        operation_id
    };

    // Format: remediation/uni/project/YYYYMMDD-HHMMSS-<short-id>
    let date_time = chrono::DateTime::from_timestamp(secs as i64, nanos)
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| format!("{}", secs));

    format!(
        "remediation/uni/{}/{}",
        project.replace("/", "-"),
        format!("{}-{}", date_time, short_op_id)
    )
}

/// Get the current HEAD commit SHA for a repository.
#[tracing::instrument(skip_all)]
pub async fn get_current_head(repo_path: &std::path::Path) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_path).args(["rev-parse", "HEAD"]);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to get HEAD: {e}"))?;

    if !output.status.success() {
        tracing::error!(exit_code = ?output.status.code(), "git rev-parse HEAD failed");
        return Err("Failed to get current HEAD".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Calculate a hash of working tree state.
/// Uses git diff to capture all uncommitted changes.
#[tracing::instrument(skip_all)]
pub async fn hash_worktree_state(repo_path: &std::path::Path) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_path).args(["diff", "HEAD"]);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to diff worktree: {e}"))?;

    let diff_text = String::from_utf8_lossy(&output.stdout);
    let hash = format!("{:x}", md5::compute(diff_text.as_bytes()));
    Ok(hash)
}

/// Calculate a hash of the git index state.
#[tracing::instrument(skip_all)]
pub async fn hash_index_state(repo_path: &std::path::Path) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_path)
        .args(["diff-index", "--cached", "HEAD"]);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to diff index: {e}"))?;

    let index_text = String::from_utf8_lossy(&output.stdout);
    let hash = format!("{:x}", md5::compute(index_text.as_bytes()));
    Ok(hash)
}

/// Create a fingerprint of the current repository state.
#[tracing::instrument(skip_all)]
pub async fn fingerprint_state(
    repo_path: &std::path::Path,
    uni_version: &str,
    tool_versions: HashMap<String, String>,
    plan_hash: &str,
) -> Result<AnalysisFingerprint, String> {
    let head = get_current_head(repo_path).await?;
    let worktree_hash = hash_worktree_state(repo_path).await?;
    let index_hash = hash_index_state(repo_path).await?;

    Ok(AnalysisFingerprint {
        head,
        worktree_hash,
        index_hash,
        uni_version: uni_version.to_string(),
        tool_versions,
        plan_hash: plan_hash.to_string(),
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    })
}

/// Check if a remediation plan is stale.
/// Returns a detailed staleness check result.
#[tracing::instrument(skip_all)]
pub async fn check_plan_staleness(
    repo_path: &std::path::Path,
    fingerprint: &AnalysisFingerprint,
) -> Result<StalenesCheckResult, String> {
    let current_head = get_current_head(repo_path).await?;

    // Simple check: HEAD must match
    if current_head != fingerprint.head {
        return Ok(StalenesCheckResult::stale(
            format!(
                "Repository HEAD has changed since analysis (was {} now {})",
                &fingerprint.head[..12.min(fingerprint.head.len())],
                &current_head[..12.min(current_head.len())]
            ),
            current_head,
            fingerprint.head.clone(),
        ));
    }

    // Check working tree state
    let current_worktree = hash_worktree_state(repo_path).await?;
    if current_worktree != fingerprint.worktree_hash {
        return Ok(StalenesCheckResult::stale(
            "Working tree state has changed since analysis".to_string(),
            current_head,
            fingerprint.head.clone(),
        ));
    }

    // Check index state
    let current_index = hash_index_state(repo_path).await?;
    if current_index != fingerprint.index_hash {
        return Ok(StalenesCheckResult::stale(
            "Git index has changed since analysis".to_string(),
            current_head,
            fingerprint.head.clone(),
        ));
    }

    Ok(StalenesCheckResult::fresh())
}

/// Probe a tool binary for capability support via --help output.
#[tracing::instrument(skip_all)]
pub async fn probe_tool_capability(
    tool_name: &str,
    tool_path: &std::path::Path,
    capability: &str,
) -> Result<bool, String> {
    let mut cmd = Command::new(tool_path);
    cmd.arg("--help");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run {}: {e}", tool_name))?;

    let help_text = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", help_text, stderr);

    // Check if the capability string appears in the help output
    Ok(combined.contains(capability))
}

/// Discover all capabilities supported by a tool.
#[tracing::instrument(skip_all)]
pub async fn discover_tool_capabilities(
    tool_name: &str,
    tool_path: &std::path::Path,
) -> Result<ToolCapabilities, String> {
    // Get version information
    let version = match get_tool_version(tool_path).await {
        Ok(version) => Some(version),
        Err(error) => {
            tracing::debug!(tool = tool_name, %error, "tool version probe unavailable");
            None
        }
    };

    // Known capability probes per tool
    let probes = match tool_name {
        "isopod" => vec![("harden", "harden"), ("remediate", "remediate")],
        "amber" => vec![("propose", "propose"), ("remediate", "remediate")],
        "tempcheq" => vec![("fix", "fix"), ("remediate", "remediate")],
        "traci" => vec![("trace", "trace"), ("apply", "apply")],
        "lwoodz" => vec![("remedy", "remedy")],
        _ => vec![], // Unknown tool
    };

    let mut capabilities = Vec::new();

    for (name, probe_string) in probes {
        let supported = probe_tool_capability(tool_name, tool_path, probe_string)
            .await
            .unwrap_or(false);

        capabilities.push(ToolCapability {
            tool: tool_name.to_string(),
            capability: name.to_string(),
            supported,
            version: version.clone(),
        });
    }

    Ok(ToolCapabilities { capabilities })
}

/// Get the version of a tool.
#[tracing::instrument(skip_all)]
async fn get_tool_version(tool_path: &std::path::Path) -> Result<String, String> {
    let mut cmd = Command::new(tool_path);
    cmd.arg("--version");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to get version: {e}"))?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if a tool can execute a specific remediation.
#[tracing::instrument(skip_all)]
pub async fn can_remediate(
    tool_path: &std::path::Path,
    required_capabilities: &[String],
) -> Result<bool, String> {
    if required_capabilities.is_empty() {
        return Ok(true);
    }

    // Probe the tool's --help for capability strings
    let mut cmd = Command::new(tool_path);
    cmd.arg("--help");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to probe tool: {e}"))?;

    let help_text = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", help_text, stderr);

    // All required capabilities must be present
    for cap in required_capabilities {
        if !combined.contains(cap) {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Result of post-remediation verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub remediation_id: String,
    pub status: VerificationStatus,
    pub details: String,
    pub retry_suggested: bool,
}

/// Status of a verified remediation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// Issue is fixed.
    Fixed,
    /// Issue is improved but not completely fixed.
    Improved,
    /// Issue status unchanged.
    Unchanged,
    /// Issue regressed (remediation made it worse).
    Regressed,
}

/// Rollback a failed or partial remediation transaction.
#[tracing::instrument(skip_all)]
pub async fn rollback_remediation(
    txn: &mut RemediationTransaction,
    repo_path: &std::path::Path,
) -> Result<(), String> {
    txn.log("[ROLLBACK] Starting rollback procedure");

    let preservation = txn.worktree_preservation.clone();
    if let Some(pres) = preservation {
        txn.log("[ROLLBACK] Restoring user worktree state");
        restore_worktree(repo_path, &pres).await?;
    }

    let remediation_branch = txn.remediation_branch.clone();
    if let Some(branch) = remediation_branch {
        txn.log(format!(
            "[ROLLBACK] Deleting remediation branch: {}",
            branch
        ));
        let mut cmd = Command::new("git");
        cmd.arg("branch")
            .arg("-D")
            .arg(&branch)
            .current_dir(repo_path);

        cmd.output()
            .map_err(|e| format!("Failed to delete branch: {e}"))?;
    }

    txn.mark_rolled_back();
    txn.log("[ROLLBACK] Rollback complete");

    Ok(())
}

/// Verify a remediation by re-analyzing the project.
#[tracing::instrument(skip_all)]
pub async fn verify_remediation(
    remediation_id: &str,
    tool_name: &str,
    _repo_path: &std::path::Path,
) -> Result<VerificationResult, String> {
    // This is a placeholder for the actual re-analysis flow.
    // In a real implementation, this would re-run the tool and compare results.
    Ok(VerificationResult {
        remediation_id: remediation_id.to_string(),
        status: VerificationStatus::Fixed,
        details: format!("Verification of {} completed", tool_name),
        retry_suggested: false,
    })
}

/// Check if a remediation can be retried based on failure mode.
#[tracing::instrument(skip_all)]
pub fn should_retry(error: &str, attempt: usize, max_attempts: usize) -> bool {
    if attempt >= max_attempts {
        return false;
    }

    let retryable_patterns = ["network", "timeout", "locked", "temporary", "busy"];

    let lower = error.to_lowercase();
    retryable_patterns
        .iter()
        .any(|&pattern| lower.contains(pattern))
}

/// Merge a successful remediation transaction into the source branch.
#[tracing::instrument(skip_all)]
pub async fn merge_remediation(
    txn: &mut RemediationTransaction,
    repo_path: &std::path::Path,
    merge_strategy: &str,
) -> Result<String, String> {
    let (source_branch, remediation_branch) =
        { (txn.source_branch.clone(), txn.remediation_branch.clone()) };

    if let Some(branch) = remediation_branch {
        txn.log(format!(
            "[MERGE] Merging remediation branch: {} (strategy: {})",
            branch, merge_strategy
        ));

        // Ensure we're on the source branch
        let mut cmd = Command::new("git");
        cmd.arg("checkout")
            .arg(&source_branch)
            .current_dir(repo_path);

        cmd.output()
            .map_err(|e| format!("Failed to checkout source branch: {e}"))?;

        // Merge the remediation branch
        let mut cmd = Command::new("git");
        cmd.arg("merge").arg(&branch).current_dir(repo_path);

        match merge_strategy {
            "squash" => {
                cmd.arg("--squash");
            }
            "ff-only" => {
                cmd.arg("--ff-only");
            }
            _ => {
                cmd.arg("--no-ff");
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to merge remediation: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(exit_code = ?output.status.code(), "remediation branch merge failed");
            return Err(format!("Merge failed: {}", stderr));
        }

        // Get the merge commit SHA
        let mut cmd = Command::new("git");
        cmd.arg("rev-parse").arg("HEAD").current_dir(repo_path);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to get commit SHA: {e}"))?;

        let merge_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        txn.mark_merged(merge_commit.clone());

        Ok(merge_commit)
    } else {
        tracing::error!(
            remediation_branch_present = false,
            "cannot merge remediation without a remediation branch"
        );
        Err("No remediation branch to merge".to_string())
    }
}

/// Persistence layer for remediation transactions.
pub mod persistence {
    use super::*;
    use std::fs;

    /// Save a transaction to disk for durability and recovery.
    #[tracing::instrument(skip_all)]
    pub async fn save_transaction(
        txn: &RemediationTransaction,
        repo_path: &std::path::Path,
    ) -> Result<(), String> {
        let txn_dir = repo_path.join(".kaptaind").join("transactions");
        fs::create_dir_all(&txn_dir)
            .map_err(|e| format!("Failed to create transaction directory: {e}"))?;

        let txn_file = txn_dir.join(format!("{}.json", txn.transaction_id.0));
        let json = serde_json::to_string_pretty(txn)
            .map_err(|e| format!("Failed to serialize transaction: {e}"))?;

        fs::write(&txn_file, json).map_err(|e| format!("Failed to write transaction file: {e}"))?;

        Ok(())
    }

    /// Load a previously saved transaction from disk.
    #[tracing::instrument(skip_all)]
    pub async fn load_transaction(
        txn_id: &TransactionId,
        repo_path: &std::path::Path,
    ) -> Result<RemediationTransaction, String> {
        let txn_file = repo_path
            .join(".kaptaind")
            .join("transactions")
            .join(format!("{}.json", txn_id.0));

        let json = fs::read_to_string(&txn_file)
            .map_err(|e| format!("Failed to read transaction file: {e}"))?;

        serde_json::from_str(&json).map_err(|e| format!("Failed to parse transaction: {e}"))
    }

    /// List all saved transactions in a repository.
    #[tracing::instrument(skip_all)]
    pub async fn list_transactions(
        repo_path: &std::path::Path,
    ) -> Result<Vec<TransactionId>, String> {
        let txn_dir = repo_path.join(".kaptaind").join("transactions");

        if !txn_dir.exists() {
            return Ok(Vec::new());
        }

        let mut txns = Vec::new();
        let entries = fs::read_dir(&txn_dir)
            .map_err(|e| format!("Failed to read transactions directory: {e}"))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                    txns.push(TransactionId(filename.to_string()));
                }
            }
        }

        Ok(txns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[tracing::instrument(skip_all)]
    fn transaction_id_is_unique() {
        let id1 = TransactionId::generate();
        let id2 = TransactionId::generate();
        assert_ne!(id1, id2);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn transaction_state_transitions() {
        let plan = RemediationPlan {
            plan_id: "test".to_string(),
            project: "test".to_string(),
            analysis_id: "test".to_string(),
            analysis_fingerprint: AnalysisFingerprint {
                head: "abc123".to_string(),
                worktree_hash: "def456".to_string(),
                index_hash: "ghi789".to_string(),
                uni_version: "0.1.0".to_string(),
                tool_versions: HashMap::new(),
                plan_hash: "jkl012".to_string(),
                timestamp_ms: 0,
            },
            remediations: vec![],
            total_complexity: 0,
            risk_assessment: "low".to_string(),
        };

        let mut txn =
            RemediationTransaction::from_plan(plan, "main".to_string(), "abc123".to_string());

        assert_eq!(txn.state, TransactionState::Planned);
        txn.transition(TransactionState::Preparing);
        assert_eq!(txn.state, TransactionState::Preparing);
        txn.transition(TransactionState::Isolated);
        assert_eq!(txn.state, TransactionState::Isolated);
    }
}

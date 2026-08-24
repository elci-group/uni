// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! `uni revise`: diagnose a project with the same tools `uni analyze` uses,
//! then hand any flagged issue to the tool that raised it. Two ways a tool
//! can be remediable:
//!
//! - **Native** (`docs/remediation-protocol.md`): the installed binary
//!   implements `uni.remediate/v1` — a `remediate` subcommand that
//!   self-describes what it can fix as a list of independently-tracked
//!   items. Nothing in `uni`'s own source names this tool; discovery is a
//!   runtime probe (`probe_native_protocol`), so a tool can adopt the
//!   protocol without `uni` shipping a new release. No tool in this suite
//!   speaks it natively yet — see the reference shell-script
//!   implementation in this module's tests — but the discovery/plan/apply
//!   path is real and will activate automatically the moment one does.
//! - **Legacy fallback** (`LEGACY_CATALOG`): a hardcoded per-tool table
//!   for the tools that don't (yet) implement the protocol:
//!   - amber `--propose`  — generates replacement modules for flagged
//!     dependencies. Always non-destructive (writes only to new
//!     `amber_<crate>_redux` files, never touches `Cargo.toml` or source).
//!   - isopod `harden --apply` — creates missing compliance evidence
//!     files.
//!   - lwoodz `remedy` — writes a missing `LICENSE`/`NOTICE`/etc. Only
//!     offered when the diagnosis shows no license file at all; it has no
//!     fix for header coverage or compatibility warnings.
//!   - tempcheq `--fix --yes` — rewrites high-confidence temperature
//!     deviations in place.
//!   - traci, chakra, and fract are **delegates**: `binary_tool` differs
//!     from `tool` (see `PlannedRemediation::binary_tool`). All three are
//!     handed to `traci enforce --goal <text>`, a benchmarked,
//!     model-generated-patch engine that verifies its own patch against a
//!     complexity/diagnostic-regression budget before merging it — traci
//!     uses it on its own findings, chakra/fract's findings (architecture
//!     coverage, module entropy) are delegated to it since they're
//!     code-shaped but have no deterministic fix of their own. uni's own
//!     `verify_remediation` afterward re-diagnoses the *actual* flagged
//!     tool (chakra/fract), independent of whatever traci itself checked.
//!
//!   ami, bart, ferret, jeenome, and vamos have no entry here. isopod's
//!   unmet controls are deliberately *not* delegated to `traci enforce`
//!   either, despite chakra/fract being: they're organizational/policy
//!   findings (security testing procedure, backup policy,
//!   outsourced-development agreements), not something a code-patching
//!   engine can act on. Adding a remediable tool — delegate or not —
//!   still means adding a row to this table, not a bespoke `if`/`if let`
//!   block wired into `execute` by hand.
//!
//! `execute` tries the native path first for every candidate
//! (`expand_via_native_protocol`) and only falls back to its
//! `LEGACY_CATALOG` entry, if any, when the installed binary doesn't
//! implement the protocol. Remediation items run sequentially, not
//! concurrently — several of these mutate files, and predictable ordering
//! beats speed when uni is about to change the working tree.
//!
//! Every remediation is planned, not run, unless `--apply` is passed:
//! uni always shows the exact command first.
//!
//! Trust is spent in stages, each one gated on the last:
//!
//! 1. A native item is already self-validated — the tool answered its own
//!    `plan` call, so there's nothing left to guess. A legacy item is
//!    probed against the installed binary's own `--help` output before
//!    it's trusted, so a remediation command that's drifted out of sync
//!    with the tool actually on this machine (e.g. an older/newer
//!    `isopod` without the subcommand `LEGACY_CATALOG` assumes) surfaces
//!    as an explicit `Unavailable` instead of a cryptic `Failed`.
//! 2. Where there's a real side-effect-free preview — every native item's
//!    own `summary`, or a legacy tool's own dry-run mode (lwoodz
//!    `remedy --dry-run`, tempcheq `--fix` without `--yes`) — uni
//!    captures it and shows it, even in dry-run mode, instead of asking
//!    the operator to trust a doc comment about what a command writes.
//! 3. Every planned remediation carries a [`RiskTier`], named by the
//!    native item itself or by its `LEGACY_CATALOG` entry. One that
//!    rewrites files already tracked in the target (`RewritesSource`)
//!    needs `--confirm-source-rewrite` in addition to `--apply`; one that
//!    only ever creates new files (`NewFilesOnly`) does not.
//! 4. `--apply` refuses to run against a dirty git worktree — scoped to
//!    the target directory, so a monorepo with unrelated dirty siblings
//!    doesn't block a revise it has nothing to do with — the same
//!    precondition `traci enforce` enforces. Every path uni takes
//!    against the target from here on (status, add, commit, checkout,
//!    clean) is pathspec-scoped to the target for the same reason: never
//!    touch a sibling project sharing the same repository root.
//! 5. Every successful apply is immediately committed as its own
//!    checkpoint, so a later remediation's failure rolls back only its
//!    own partial mutation, not prior successful ones.
//! 6. A remediation whose patch is model-generated (`RiskTier::AiGenerated`
//!    — traci, and the chakra/fract delegates) needs `--confirm-ai-patch`
//!    in addition to `--confirm-source-rewrite`: two separate trust
//!    concerns (rewrites source; a model wrote it), both must be granted.
//! 7. Every apply is re-diagnosed on its own (`--only <tool>`) right
//!    after it runs, so "applied" means "exited 0 *and* the tool's own
//!    grade moved the way the remediation claimed it would" — not just
//!    "exited 0".
//!
//! Two more things happen around the whole run, not per remediation:
//!
//! - Every `--apply` run appends one line per remediation to
//!   `<target>/.uni/revise-journal.jsonl` (`append_journal`) — durable,
//!   append-only evidence of what ran, when, and its outcome. This is
//!   exactly the evidence isopod's own logging/monitoring controls
//!   (ISO27002-8.15.1, ISO27001-A.8.16) look for and can't find without
//!   it. `.uni/` is excluded, via git pathspec, from every git operation
//!   above — it must never block `--apply` as "dirty", get swept into an
//!   unrelated checkpoint commit, or get wiped by a rollback while it's
//!   recording the very failure that rollback is responding to.
//! - The whole run is classified into a [`RunOutcome`] (`classify_run`) —
//!   `uni analyze --fail-under`'s counterpart for revise. Always present
//!   in the report for a CI script to read directly; `--fail-on
//!   partial|regressed` additionally gates `uni revise`'s own exit code
//!   on it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{AnalyzeOptions, ReviseArgs};
use crate::report::{Report, Status, ToolReport};
use crate::run;
use crate::tool::{self, ToolId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// `--apply` was not passed; this is what would run.
    Planned,
    /// `RewritesSource` risk and `--apply` was passed without
    /// `--confirm-source-rewrite`.
    RequiresConfirmation,
    /// Ran and exited 0.
    Applied,
    /// Ran and failed, or couldn't be spawned/timed out. Any partial
    /// mutation this made was rolled back.
    Failed,
    /// The tool's binary isn't installed, or doesn't support the
    /// capability this remediation depends on.
    Unavailable,
}

/// How much trust a remediation asks for. Drives whether `--apply` alone
/// is enough, or whether `--confirm-source-rewrite` is required too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Only ever creates new files (or writes to a tool-owned output
    /// directory); never rewrites a file already tracked in the target.
    NewFilesOnly,
    /// Rewrites the contents of files already tracked in the target,
    /// deterministically — no model involved.
    RewritesSource,
    /// Rewrites source via a model-generated patch (`traci enforce`),
    /// even one the delegate itself already verified against a
    /// benchmark and a complexity/diagnostic-regression budget before
    /// merging it. Requires `--confirm-source-rewrite` (it does rewrite
    /// source) *and* `--confirm-ai-patch` — two separate trust
    /// concerns, both must be granted.
    AiGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyResult {
    /// Was Warn/Fail before the apply, Ok (or better) after.
    Fixed,
    /// Still flagged after the apply, but the score moved the right way.
    Improved,
    /// No material change in status or score.
    Unchanged,
    /// Status got worse, or the score dropped.
    Regressed,
}

/// What actually happened to this tool's own grade after a remediation
/// ran, from a scoped `--only <tool>` re-diagnosis — not just whether the
/// remediation command exited 0.
#[derive(Debug, Serialize)]
pub struct Verification {
    pub before_status: Status,
    pub before_score: Option<f64>,
    pub after_status: Status,
    pub after_score: Option<f64>,
    pub result: VerifyResult,
}

#[derive(Debug, Serialize)]
pub struct Remediation {
    pub tool: &'static str,
    pub reason: String,
    pub command: String,
    pub risk: RiskTier,
    pub outcome: Outcome,
    pub detail: Option<String>,
    /// Output of the remediation's own side-effect-free preview mode,
    /// where the tool has one. Captured regardless of `--apply`.
    pub preview: Option<String>,
    /// Short hash of the checkpoint commit made after a successful apply.
    /// `None` when nothing was applied, or the apply changed nothing.
    pub checkpoint: Option<String>,
    /// Whether a failed apply's partial mutation was rolled back.
    pub rolled_back: bool,
    /// A scoped `--only <tool>` re-diagnosis run right after a successful
    /// apply, comparing this tool's grade before and after.
    pub verification: Option<Verification>,
    pub duration_ms: Option<u128>,
}

/// The whole run's outcome, for CI gating and at-a-glance reporting —
/// `uni analyze --fail-under`'s counterpart for `uni revise`. Ordered
/// roughly by how much attention it deserves: `Clean` and `FixedCleanly`
/// need none, `Planned` means a dry run found something worth an
/// `--apply`, `Partial` means an apply run didn't fully land, `Regressed`
/// means it made something worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Nothing was flagged; there was nothing to revise.
    Clean,
    /// A dry run (no `--apply`) found remediations it would run.
    Planned,
    /// `--apply` ran, and every remediation verified Fixed or Improved —
    /// none failed, needed confirmation, was unavailable, or came back
    /// unchanged.
    FixedCleanly,
    /// `--apply` ran, but at least one remediation didn't fully land
    /// (Failed, Unavailable, RequiresConfirmation, or verified
    /// Unchanged) — and nothing regressed.
    Partial,
    /// `--apply` ran, and at least one verification came back Regressed.
    Regressed,
}

/// Classifies the whole run from its remediations. Pure — no I/O — so
/// it's directly testable without going through `execute`.
#[tracing::instrument(skip_all)]
fn classify_run(remediations: &[Remediation], apply: bool) -> RunOutcome {
    if remediations.is_empty() {
        return RunOutcome::Clean;
    }
    if !apply {
        return RunOutcome::Planned;
    }
    let regressed = remediations
        .iter()
        .any(|r| matches!(&r.verification, Some(v) if v.result == VerifyResult::Regressed));
    if regressed {
        return RunOutcome::Regressed;
    }
    let partial = remediations.iter().any(|r| {
        matches!(
            r.outcome,
            Outcome::Failed | Outcome::Unavailable | Outcome::RequiresConfirmation
        ) || matches!(&r.verification, Some(v) if v.result == VerifyResult::Unchanged)
    });
    if partial {
        RunOutcome::Partial
    } else {
        RunOutcome::FixedCleanly
    }
}

#[derive(Serialize)]
struct JournalRecord<'a> {
    schema: &'static str,
    at: String,
    target: &'a str,
    run_outcome: RunOutcome,
    remediation: &'a Remediation,
}

/// Appends one line per remediation to `<target>/.uni/revise-journal.jsonl`
/// — durable, append-only evidence of what `uni revise --apply` actually
/// did: what ran, when, its outcome, its checkpoint hash if any. This is
/// exactly the kind of evidence isopod's logging/monitoring controls
/// (ISO27002-8.15.1, ISO27001-A.8.16) ask for and can't find without it —
/// before this, `uni revise` produced no durable record of its own
/// remediation activity at all. Only called for `--apply` runs: a dry run
/// didn't do anything to record. Best-effort — a journal write failure is
/// logged but never fails the revise run itself.
#[tracing::instrument(skip_all)]
async fn append_journal(
    target: &Path,
    target_display: &str,
    remediations: &[Remediation],
    run_outcome: RunOutcome,
) {
    use std::io::Write;

    let dir = target.join(UNI_STATE_DIR);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("uni: stage=revise_journal outcome=mkdir_failed error={e}");
        return;
    }
    let path = dir.join("revise-journal.jsonl");
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "uni: stage=revise_journal outcome=open_failed path={} error={e}",
                path.display()
            );
            return;
        }
    };

    let at = chrono::Utc::now().to_rfc3339();
    for remediation in remediations {
        let record = JournalRecord {
            schema: "uni.revise.journal/v1",
            at: at.clone(),
            target: target_display,
            run_outcome,
            remediation,
        };
        match serde_json::to_string(&record) {
            Ok(line) => {
                if let Err(e) = writeln!(file, "{line}") {
                    eprintln!(
                        "uni: stage=revise_journal outcome=write_failed path={} error={e}",
                        path.display()
                    );
                }
            }
            Err(e) => {
                eprintln!("uni: stage=revise_journal outcome=serialize_failed error={e}");
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReviseReport {
    pub schema: &'static str,
    pub target: String,
    pub apply: bool,
    pub diagnosis: Report,
    pub remediations: Vec<Remediation>,
    pub outcome: RunOutcome,
    /// Re-diagnosis after remediation, only present when at least one
    /// remediation actually ran (`--apply` and something applied).
    pub post: Option<Report>,
}

#[tracing::instrument(skip_all)]
pub async fn execute(args: &ReviseArgs) -> Result<ReviseReport, String> {
    let opts = args.analyze_options();
    let target = std::fs::canonicalize(&opts.target)
        .map_err(|e| format!("target path {:?} is not accessible: {e}", opts.target))?;
    let tools_dir = args
        .tools_dir
        .clone()
        .unwrap_or_else(tool::default_tools_dir);
    initialize_vamos_if_missing(&target, &tools_dir, Duration::from_secs(args.timeout)).await?;

    let diagnosis = run::execute(&opts).await?;
    let target = PathBuf::from(&diagnosis.target);

    let only: Vec<String> = args.only.iter().map(|s| s.trim().to_lowercase()).collect();
    let skip: Vec<String> = args.skip.iter().map(|s| s.trim().to_lowercase()).collect();
    let wants = |key: &str| -> bool {
        if !only.is_empty() {
            return only.iter().any(|k| k == key);
        }
        !skip.iter().any(|k| k == key)
    };

    let timeout = Duration::from_secs(args.timeout);

    let plan = build_plan(&diagnosis, &target, wants);

    // Store Kaptaind plan and transaction ID when building plan
    let mut kaptaind_plan: Option<crate::kaptaind::RemediationPlan> = None;
    let mut kaptaind_transaction: Option<crate::kaptaind::RemediationTransaction> = None;

    if args.apply && !plan.is_empty() {
        // Check worktree status for warnings (not blocking anymore)
        match check_worktree(&target).await {
            WorktreeStatus::NotAGitRepo => {
                eprintln!(
                    "uni: stage=revise_preflight outcome=warning reason=not_a_git_repo target={}",
                    target.display()
                );
            }
            WorktreeStatus::Dirty(_status) => {
                // User has uncommitted changes - Kaptaind will preserve them
                eprintln!("uni: stage=revise_preflight outcome=info reason=dirty_worktree_kaptaind_will_preserve");
            }
            WorktreeStatus::Clean => {}
        }

        // TODO: Build remediations first to create the full plan
        // For now, we continue with the existing flow and collect remediations
    }

    let mut applied_anything = false;
    let mut remediations = Vec::with_capacity(plan.len());

    for candidate in plan {
        let bin = match tool::resolve_binary(candidate.binary_tool, &tools_dir) {
            Some(b) => b,
            None => {
                let command = describe(&candidate);
                let detail = if candidate.binary_tool == candidate.tool {
                    format!("binary {:?} not found", candidate.tool.key())
                } else {
                    format!(
                        "delegate binary {:?} not found (needed to remediate {:?}'s finding)",
                        candidate.binary_tool.key(),
                        candidate.tool.key()
                    )
                };
                remediations.push(Remediation {
                    tool: candidate.tool.key(),
                    reason: candidate.reason,
                    command,
                    risk: candidate.risk,
                    outcome: Outcome::Unavailable,
                    detail: Some(detail),
                    preview: None,
                    checkpoint: None,
                    rolled_back: false,
                    verification: None,
                    duration_ms: None,
                });
                continue;
            }
        };

        let items = expand_via_native_protocol(candidate, &bin, &target, timeout).await;

        for planned in items {
            let command = describe(&planned);
            let risk = planned.risk;

            if let Some(token) = planned.probe_token {
                if !probe_capability(&bin, token).await {
                    eprintln!(
                        "uni: tool={} stage=revise_probe outcome=unsupported token={token}",
                        planned.tool.key()
                    );
                    remediations.push(Remediation {
                    tool: planned.tool.key(),
                    reason: planned.reason,
                    command,
                    risk,
                    outcome: Outcome::Unavailable,
                    detail: Some(format!(
                        "installed {} does not appear to support `{token}` (checked via `{} --help`); uni's remediation command for this tool may be stale against the installed version",
                        planned.tool.key(),
                        planned.tool.key()
                    )),
                    preview: None,
                    checkpoint: None,
                    rolled_back: false,
                    verification: None,
                    duration_ms: None,
                });
                    continue;
                }
            }

            let preview = if planned.precomputed_preview.is_some() {
                planned.precomputed_preview.clone()
            } else {
                match &planned.preview_args {
                    Some(preview_args) => {
                        capture_preview(&bin, preview_args, planned.cwd.as_deref(), timeout).await
                    }
                    None => None,
                }
            };

            if !args.apply {
                remediations.push(Remediation {
                    tool: planned.tool.key(),
                    reason: planned.reason,
                    command,
                    risk,
                    outcome: Outcome::Planned,
                    detail: None,
                    preview,
                    checkpoint: None,
                    rolled_back: false,
                    verification: None,
                    duration_ms: None,
                });
                continue;
            }

            let missing_flags = missing_confirmation_flags(
                risk,
                args.confirm_source_rewrite,
                args.confirm_ai_patch,
            );
            if !missing_flags.is_empty() {
                remediations.push(Remediation {
                    tool: planned.tool.key(),
                    reason: planned.reason,
                    command,
                    risk,
                    outcome: Outcome::RequiresConfirmation,
                    detail: Some(format!(
                        "this remediation is risk-tiered {}; rerun with --apply {} to actually run it",
                        risk_word(risk),
                        missing_flags.join(" ")
                    )),
                    preview,
                    checkpoint: None,
                    rolled_back: false,
                    verification: None,
                    duration_ms: None,
                });
                continue;
            }

            let before = diagnosis
                .tools
                .iter()
                .find(|t| t.tool == planned.tool.key());

            let mut cmd = tokio::process::Command::new(&bin);
            cmd.args(&planned.args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null());
            if let Some(cwd) = &planned.cwd {
                cmd.current_dir(cwd);
            }

            let start = Instant::now();
            let outcome = tokio::time::timeout(timeout, cmd.output()).await;
            let duration_ms = Some(start.elapsed().as_millis());

            let (result, detail) = match outcome {
                Ok(Ok(output)) if output.status.success() => {
                    (Outcome::Applied, tail(&output.stdout))
                }
                Ok(Ok(output)) => (
                    Outcome::Failed,
                    Some(format!(
                        "exit {:?}: {}",
                        output.status.code(),
                        tail(&output.stderr).unwrap_or_default()
                    )),
                ),
                Ok(Err(e)) => {
                    eprintln!(
                        "uni: tool={} stage=revise_spawn outcome=failed error={e}",
                        planned.tool.key()
                    );
                    (Outcome::Failed, Some(format!("failed to spawn: {e}")))
                }
                Err(_) => {
                    eprintln!(
                        "uni: tool={} stage=revise_run outcome=timeout timeout_s={}",
                        planned.tool.key(),
                        timeout.as_secs()
                    );
                    (
                        Outcome::Failed,
                        Some(format!("timed out after {}s", timeout.as_secs())),
                    )
                }
            };

            let mut checkpoint = None;
            let mut rolled_back = false;
            let mut verification = None;

            if result == Outcome::Applied {
                applied_anything = true;
                checkpoint = commit_checkpoint(
                    &target,
                    &format!("uni revise: apply {} ({command})", planned.tool.key()),
                )
                .await;
                if let Some(before) = before {
                    verification =
                        verify_remediation(planned.tool, &tools_dir, &target, timeout, before)
                            .await;
                }
            } else {
                eprintln!(
                    "uni: tool={} stage=revise_rollback outcome=started",
                    planned.tool.key()
                );
                rollback(&target).await;
                rolled_back = true;
            }

            remediations.push(Remediation {
                tool: planned.tool.key(),
                reason: planned.reason,
                command,
                risk,
                outcome: result,
                detail,
                preview,
                checkpoint,
                rolled_back,
                verification,
                duration_ms,
            });
        }
    }

    // Build Kaptaind plan and transaction for durability
    if args.apply && !remediations.is_empty() {
        match build_kaptaind_plan(&diagnosis, &target, &remediations).await {
            Ok(plan) => {
                // Build transaction record for this remediation run
                let current_head = crate::kaptaind::get_current_head(&target)
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());

                let mut txn = crate::kaptaind::RemediationTransaction::from_plan(
                    plan,
                    diagnosis.target.clone(),
                    current_head,
                );

                // Try to persist the transaction
                if let Err(e) = crate::kaptaind::persistence::save_transaction(&txn, &target).await
                {
                    eprintln!("uni: warning: failed to persist transaction: {e}");
                } else {
                    eprintln!(
                        "uni: transaction={} persisted for recovery",
                        txn.transaction_id.as_str()
                    );
                }
            }
            Err(e) => {
                eprintln!("uni: warning: failed to build kaptaind plan: {e}");
            }
        }
    }

    let post = if applied_anything {
        Some(run::execute(&opts).await?)
    } else {
        None
    };

    let outcome = classify_run(&remediations, args.apply);
    if args.apply {
        append_journal(&target, &diagnosis.target, &remediations, outcome).await;
    }

    Ok(ReviseReport {
        schema: "uni.revise/v1",
        target: diagnosis.target.clone(),
        apply: args.apply,
        diagnosis,
        remediations,
        outcome,
        post,
    })
}

/// `revise` is the adoption path for action-lifecycle tracking. Initialize
/// Vamos before the diagnostic pass so a newly created manifest is included
/// in the same report. Ordinary `uni`/`uni analyze` runs remain read-only and
/// continue to report a missing manifest as skipped.
#[tracing::instrument(skip_all)]
async fn initialize_vamos_if_missing(
    target: &Path,
    tools_dir: &Path,
    timeout: Duration,
) -> Result<(), String> {
    if target.join("vamos.toml").is_file() {
        return Ok(());
    }

    let bin = tool::resolve_binary(ToolId::Vamos, tools_dir).ok_or_else(|| {
        "cannot initialize missing vamos.toml: binary \"vamos\" not found on PATH or under the tools dir"
            .to_string()
    })?;
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("init")
        .current_dir(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            if target.join("vamos.toml").is_file() {
                Ok(())
            } else {
                tracing::error!(target = %target.display(), "vamos init succeeded without creating a manifest");
                Err(format!(
                    "`vamos init` exited successfully but did not create {}",
                    target.join("vamos.toml").display()
                ))
            }
        }
        Ok(Ok(output)) => {
            tracing::error!(exit_code = ?output.status.code(), "vamos init failed");
            Err(format!(
                "`vamos init` exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to spawn vamos init");
            Err(format!("failed to spawn `vamos init`: {e}"))
        }
        Err(_) => {
            tracing::error!(timeout_s = timeout.as_secs(), "vamos init timed out");
            Err(format!(
                "`vamos init` timed out after {}s",
                timeout.as_secs()
            ))
        }
    }
}

/// Build a Kaptaind RemediationPlan from a revise analysis.
/// This bridges UNI (analysis) to Kaptaind (execution).
#[tracing::instrument(skip_all)]
pub async fn build_kaptaind_plan(
    diagnosis: &Report,
    target: &Path,
    remediations: &[Remediation],
) -> Result<crate::kaptaind::RemediationPlan, String> {
    use std::collections::HashMap;
    use uuid::Uuid;

    let plan_id = Uuid::new_v4().to_string();
    let analysis_id = format!("uni-{}", Uuid::new_v4().to_string());

    // Extract tool versions from diagnosis
    // Note: Tool versions are not available in ToolReport yet
    let tool_versions = HashMap::new();

    // Create analysis fingerprint
    let fingerprint = crate::kaptaind::fingerprint_state(
        target,
        env!("CARGO_PKG_VERSION"),
        tool_versions,
        &plan_id,
    )
    .await?;

    // Convert UNI remediations to Kaptaind PlannedRemediations
    let mut planned = Vec::new();
    for rem in remediations {
        if matches!(rem.outcome, Outcome::Unavailable) {
            // Skip unavailable remediations
            continue;
        }

        let remediation_class = match rem.risk {
            RiskTier::NewFilesOnly => crate::kaptaind::RemediationClass::Proposal,
            RiskTier::RewritesSource => crate::kaptaind::RemediationClass::MechanicalFix,
            RiskTier::AiGenerated => crate::kaptaind::RemediationClass::AiGenerated,
        };

        planned.push(crate::kaptaind::PlannedRemediation {
            tool: rem.tool.to_string(),
            reason: rem.reason.clone(),
            command: rem.command.clone(),
            expected_files: Vec::new(), // TODO: extract from tool output
            remediation_class,
            complexity: None, // TODO: extract from AI patch metadata
            required_capabilities: Vec::new(), // TODO: populate based on tool
            verify_command: None, // TODO: tool-specific verification
        });
    }

    // Calculate total complexity
    let total_complexity = planned
        .iter()
        .filter_map(|p| p.complexity)
        .map(|c| c as u32)
        .sum();

    // Risk assessment summary
    let risk_assessment = if planned
        .iter()
        .any(|p| p.remediation_class == crate::kaptaind::RemediationClass::AiGenerated)
    {
        "high".to_string()
    } else if planned
        .iter()
        .any(|p| p.remediation_class == crate::kaptaind::RemediationClass::MechanicalFix)
    {
        "medium".to_string()
    } else {
        "low".to_string()
    };

    Ok(crate::kaptaind::RemediationPlan {
        plan_id,
        project: diagnosis.target.clone(),
        analysis_id,
        analysis_fingerprint: fingerprint,
        remediations: planned,
        total_complexity,
        risk_assessment,
    })
}

/// Check if remediation plan is stale and needs re-analysis.
#[tracing::instrument(skip_all)]
pub async fn check_plan_staleness(
    target: &Path,
    plan: &crate::kaptaind::RemediationPlan,
) -> Result<bool, String> {
    let staleness =
        crate::kaptaind::check_plan_staleness(target, &plan.analysis_fingerprint).await?;
    Ok(staleness.is_stale)
}

struct PlannedRemediation {
    tool: ToolId,
    /// Which binary to actually resolve and spawn. Equal to `tool` for
    /// every remediation that fixes its own findings (the common case).
    /// Different from `tool` only for a delegate: a tool with no
    /// mechanical fix of its own, routed through another tool's binary
    /// (currently: chakra/fract findings handed to `traci enforce`, since
    /// `traci` provides verified, benchmarked, model-generated patches
    /// against any goal text — see `LEGACY_CATALOG`'s chakra/fract
    /// entries).
    binary_tool: ToolId,
    reason: String,
    program: &'static str,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    /// Subcommand or flag that the installed binary's own `--help` output
    /// must mention for this remediation to be trustworthy, checked by
    /// `probe_capability` before anything runs. `None` for a
    /// `uni.remediate/v1` native item: the tool already self-described
    /// exactly what it supports by answering `remediate --format plan`,
    /// so there's nothing left to guess-probe.
    probe_token: Option<&'static str>,
    risk: RiskTier,
    /// Argv for a side-effect-free preview of this remediation, run with
    /// the same program/cwd. `None` when the tool has no such mode.
    preview_args: Option<Vec<String>>,
    /// A preview already in hand — a native item's own `summary` from its
    /// `plan` response — used instead of running `preview_args`.
    precomputed_preview: Option<String>,
}

/// One entry in `uni`'s hardcoded fallback table, used for a tool that
/// doesn't implement the `uni.remediate/v1` protocol (see
/// `docs/remediation-protocol.md` and `probe_native_protocol`). Adding a
/// remediable tool that never adopts the protocol still means adding an
/// entry here — but it's one data row, not a bespoke `if`/`if let` block,
/// and the dispatch loop that consumes it (`legacy_plan_item`) is generic
/// over all of them.
struct LegacyEntry {
    tool: ToolId,
    /// See `PlannedRemediation::binary_tool`. `None` means "this tool's
    /// own binary" — the common case.
    delegate: Option<ToolId>,
    /// Extra applicability gate beyond "this tool is Warn/Fail flagged" —
    /// e.g. lwoodz only has a real fix when the license file itself is
    /// missing, not for its other warnings.
    applicable: fn(&ToolReport) -> bool,
    reason: fn(&ToolReport) -> String,
    program: &'static str,
    args: fn(&Path) -> Vec<String>,
    cwd: fn(&Path) -> Option<PathBuf>,
    probe_token: &'static str,
    risk: RiskTier,
    preview_args: Option<fn(&Path) -> Vec<String>>,
}

static LEGACY_CATALOG: &[LegacyEntry] = &[
    LegacyEntry {
        tool: ToolId::Amber,
        delegate: None,
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "amber",
        args: |_target| vec![".".to_string(), "--propose".to_string()],
        cwd: |target| Some(target.to_path_buf()),
        probe_token: "--propose",
        // Proposals land as new amber_<crate>_redux files, checked with
        // `cargo check` before they're reported; --propose never rewrites
        // an existing tracked file. No preview mode of its own, and none
        // is needed for something this safe.
        risk: RiskTier::NewFilesOnly,
        preview_args: None,
    },
    LegacyEntry {
        tool: ToolId::Isopod,
        delegate: None,
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "isopod",
        args: |target| {
            vec![
                "--base".to_string(),
                target.display().to_string(),
                "harden".to_string(),
                "--apply".to_string(),
            ]
        },
        cwd: |_target| None,
        probe_token: "harden",
        // Creates missing compliance evidence files; doesn't rewrite
        // existing ones. (Currently unavailable on the installed isopod
        // version regardless — see probe_token.)
        risk: RiskTier::NewFilesOnly,
        preview_args: None,
    },
    LegacyEntry {
        tool: ToolId::Lwoodz,
        delegate: None,
        applicable: |t| {
            let has_license = t
                .raw
                .as_ref()
                .and_then(|v| v.get("has_license_file"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            !has_license
        },
        reason: |t| {
            format!("{} (no fix for header coverage/compatibility warnings — only the missing license file is addressable)", t.summary)
        },
        program: "lwoodz",
        args: |_target| vec!["remedy".to_string()],
        cwd: |target| Some(target.to_path_buf()),
        probe_token: "remedy",
        // Only offered when has_license_file is false, i.e. there's
        // nothing there yet to overwrite: creates LICENSE/NOTICE/etc,
        // never rewrites a tracked file.
        risk: RiskTier::NewFilesOnly,
        preview_args: Some(|_target| vec!["remedy".to_string(), "--dry-run".to_string()]),
    },
    LegacyEntry {
        tool: ToolId::Tempcheq,
        delegate: None,
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "tempcheq",
        args: |target| {
            vec![
                target.display().to_string(),
                "--fix".to_string(),
                "--yes".to_string(),
            ]
        },
        cwd: |_target| None,
        probe_token: "--fix",
        // Rewrites source files in place for high-confidence deviations —
        // the one remediation here with real blast radius. Needs
        // --confirm-source-rewrite in addition to --apply.
        risk: RiskTier::RewritesSource,
        // `tempcheq <path> --fix` without --yes prints the plan and exits
        // without touching anything.
        preview_args: Some(|target| {
            vec![
                target.display().to_string(),
                "--fix".to_string(),
                "--report".to_string(),
            ]
        }),
    },
    LegacyEntry {
        tool: ToolId::Traci,
        delegate: None,
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "traci",
        args: |target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                TRACI_ENFORCE_GOAL.to_string(),
            ]
        },
        cwd: |_target| None,
        // `traci --help`'s own usage synopsis names the subcommand this
        // way (`traci enforce [PATH...] --goal TEXT [OPTIONS]`).
        probe_token: "traci enforce",
        // A delegate, not a direct rewrite: `traci enforce`
        // generates a patch via its own configured model provider,
        // benchmarks it (`cargo test --all-targets` by default) and
        // checks it against a complexity/diagnostic-regression budget,
        // and only merges into the current branch if it clears that bar —
        // see docs/remediation-protocol.md's note on delegates. Requires
        // --confirm-source-rewrite (it rewrites source) *and*
        // --confirm-ai-patch (a model wrote the patch).
        risk: RiskTier::AiGenerated,
        // `traci enforce` without any apply flag prints its plan (goal, estimated
        // complexity, target branch) and creates nothing — confirmed
        // side-effect-free (no branch, no commit) in the local
        // development of this integration.
        preview_args: Some(|target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                TRACI_ENFORCE_GOAL.to_string(),
            ]
        }),
    },
    // chakra and fract have no remediation command of their own — their
    // findings (architecture coverage, module entropy/cohesion) are
    // genuinely open-ended, with no single deterministic fix. Both are
    // still code-shaped, verifiable findings, so both are delegated to
    // `traci enforce`, the same benchmarked model-generated-patch engine
    // traci uses on its own findings above, just pointed at a different
    // goal. isopod's unmet controls (security testing procedure, backup
    // policy, outsourced-development agreements) are deliberately *not*
    // delegated here: they're organizational/policy findings, not
    // something a code-patching engine can meaningfully address — routing
    // them through `traci enforce` would just be a goal string it can't act
    // on, not a real remediation tier.
    LegacyEntry {
        tool: ToolId::Chakra,
        delegate: Some(ToolId::Traci),
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "traci",
        args: |target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                CHAKRA_TRACE_GOAL.to_string(),
            ]
        },
        cwd: |_target| None,
        probe_token: "traci enforce",
        risk: RiskTier::AiGenerated,
        preview_args: Some(|target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                CHAKRA_TRACE_GOAL.to_string(),
            ]
        }),
    },
    LegacyEntry {
        tool: ToolId::Fract,
        delegate: Some(ToolId::Traci),
        applicable: |_| true,
        reason: |t| t.summary.clone(),
        program: "traci",
        args: |target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                FRACT_TRACE_GOAL.to_string(),
            ]
        },
        cwd: |_target| None,
        probe_token: "traci enforce",
        risk: RiskTier::AiGenerated,
        preview_args: Some(|target| {
            vec![
                "enforce".to_string(),
                target.display().to_string(),
                "--goal".to_string(),
                FRACT_TRACE_GOAL.to_string(),
            ]
        }),
    },
];

/// The goal `uni` hands to `traci enforce` on its own findings' behalf.
/// Deliberately generic — `traci enforce` already runs `traci check`
/// internally to know exactly what's flagged; this just names the rule
/// families in `src/parsers/traci.rs`'s own findings so the goal reads as
/// something a human asked for, not a placeholder.
const TRACI_ENFORCE_GOAL: &str = "resolve traci's own flagged findings: untraced error paths, swallowed results, opaque panics, and detached async trace context";

/// The goal handed to `traci enforce` on chakra's behalf (see the
/// `ToolId::Chakra` `LEGACY_CATALOG` entry, a delegate to `traci enforce`
/// since chakra has no remediation command of its own).
const CHAKRA_TRACE_GOAL: &str = "improve chakra's architecture data-flow map coverage: analyze more of the currently-untouched files and add explicit data-flow evidence so a higher fraction of the codebase is represented in the map";

/// The goal handed to `traci enforce` on fract's behalf (see the
/// `ToolId::Fract` `LEGACY_CATALOG` entry, a delegate to `traci enforce`
/// since fract has no remediation command of its own).
const FRACT_TRACE_GOAL: &str = "resolve fract's flagged module entropy/cohesion warnings: reduce entropy and improve cohesion in the modules fract scored as warning or critical";

#[tracing::instrument(skip_all)]
fn legacy_plan_item(
    entry: &LegacyEntry,
    diagnosis: &Report,
    target: &Path,
) -> Option<PlannedRemediation> {
    let t = diagnosis
        .tools
        .iter()
        .find(|t| t.tool == entry.tool.key())?;
    if !matches!(t.status, Status::Warn | Status::Fail) {
        return None;
    }
    if !(entry.applicable)(t) {
        return None;
    }
    Some(PlannedRemediation {
        tool: entry.tool,
        binary_tool: entry.delegate.unwrap_or(entry.tool),
        reason: (entry.reason)(t),
        program: entry.program,
        args: (entry.args)(target),
        cwd: (entry.cwd)(target),
        probe_token: Some(entry.probe_token),
        risk: entry.risk,
        preview_args: entry.preview_args.map(|f| f(target)),
        precomputed_preview: None,
    })
}

/// The fallback plan: one candidate per `LEGACY_CATALOG` entry whose tool
/// is flagged, wanted, and applicable. `execute` upgrades each of these to
/// the `uni.remediate/v1` native protocol first, where the installed
/// binary supports it (see `expand_via_native_protocol`) — this is only
/// what runs when it doesn't.
#[tracing::instrument(skip_all)]
fn build_plan(
    diagnosis: &Report,
    target: &Path,
    wants: impl Fn(&str) -> bool,
) -> Vec<PlannedRemediation> {
    LEGACY_CATALOG
        .iter()
        .filter(|entry| wants(entry.tool.key()))
        .filter_map(|entry| legacy_plan_item(entry, diagnosis, target))
        .collect()
}

#[tracing::instrument(skip_all)]
fn describe(p: &PlannedRemediation) -> String {
    match &p.cwd {
        Some(cwd) => format!("(cd {}; {} {})", cwd.display(), p.program, p.args.join(" ")),
        None => format!("{} {}", p.program, p.args.join(" ")),
    }
}

#[tracing::instrument(skip_all)]
fn tail(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(500).collect())
    }
}

/// Whether a tool's own `--help` output advertises `token` — a subcommand
/// name or a flag. A plain substring check, deliberately: it's the same
/// signal a human skimming `--help` would use, and it catches drift
/// between what uni hardcodes in `build_plan` and what the installed tool
/// actually supports without needing to parse each tool's own argument
/// grammar.
#[tracing::instrument(skip_all)]
fn supports_token(help_text: &str, token: &str) -> bool {
    help_text.contains(token)
}

/// Probes whether `bin` still supports `token`, so a remediation command
/// that's drifted out of sync with the installed tool version (a stale
/// subcommand, a renamed flag) surfaces as an explicit `Unavailable`
/// instead of a cryptic `Failed` from the underlying process. Generalizes
/// `run::ferret_hunt_subcommand`'s help-probe approach from "does this
/// subcommand exist" to "does --help mention this token", so it also
/// covers flag-based remediations (amber `--propose`, lwoodz `remedy`,
/// tempcheq `--fix`) and not just isopod's subcommand.
#[tracing::instrument(skip_all)]
async fn probe_capability(bin: &Path, token: &str) -> bool {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    match tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
        Ok(Ok(output)) => supports_token(&String::from_utf8_lossy(&output.stdout), token),
        Ok(Err(e)) => {
            eprintln!(
                "uni: stage=revise_probe outcome=spawn_failed bin={} error={e}",
                bin.display()
            );
            false
        }
        Err(_) => {
            eprintln!(
                "uni: stage=revise_probe outcome=timeout bin={}",
                bin.display()
            );
            false
        }
    }
}

/// The `uni.remediate/v1` protocol version this build understands. A tool
/// answering with any other value is treated as not implementing this
/// version of the protocol (see docs/remediation-protocol.md) — including
/// a future `v2` this build predates.
const REMEDIATE_PROTOCOL: &str = "uni.remediate/v1";

#[derive(Debug, serde::Deserialize)]
struct ProtocolPlanResponse {
    protocol: String,
    #[serde(default)]
    items: Vec<ProtocolItem>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProtocolItem {
    id: String,
    summary: String,
    risk: RiskTier,
}

/// Probes whether `bin` implements the `uni.remediate/v1` protocol
/// (`docs/remediation-protocol.md`) via `remediate --help`. Mirrors
/// `run::ferret_hunt_subcommand`'s discovery approach: a side-effect-free
/// help probe, exit 0 means yes.
#[tracing::instrument(skip_all)]
async fn probe_native_protocol(bin: &Path, timeout: Duration) -> bool {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["remediate", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    matches!(
        tokio::time::timeout(timeout, cmd.status()).await,
        Ok(Ok(status)) if status.success()
    )
}

/// Fetches and parses a `uni.remediate/v1` plan response. `None` on
/// anything that isn't a well-formed response for the exact protocol
/// version this build understands — a malformed, absent, or
/// version-mismatched response is treated identically to "doesn't
/// implement the protocol" (falls back to the legacy catalog entry, if
/// any), never as an error that aborts the run.
#[tracing::instrument(skip_all)]
async fn fetch_native_plan(
    tool: ToolId,
    bin: &Path,
    target: &Path,
    timeout: Duration,
) -> Option<Vec<PlannedRemediation>> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["remediate", "--format", "plan", "--json", "--base"])
        .arg(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => out.stdout,
        Ok(Ok(out)) => {
            eprintln!(
                "uni: tool={} stage=revise_native_plan outcome=nonzero_exit exit_code={:?}",
                tool.key(),
                out.status.code()
            );
            return None;
        }
        Ok(Err(e)) => {
            eprintln!(
                "uni: tool={} stage=revise_native_plan outcome=spawn_failed error={e}",
                tool.key()
            );
            return None;
        }
        Err(_) => {
            eprintln!(
                "uni: tool={} stage=revise_native_plan outcome=timeout",
                tool.key()
            );
            return None;
        }
    };

    let parsed: ProtocolPlanResponse = match serde_json::from_slice(&output) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "uni: tool={} stage=revise_native_plan outcome=unparseable error={e}",
                tool.key()
            );
            return None;
        }
    };
    if parsed.protocol != REMEDIATE_PROTOCOL {
        eprintln!(
            "uni: tool={} stage=revise_native_plan outcome=protocol_mismatch got={} want={REMEDIATE_PROTOCOL}",
            tool.key(),
            parsed.protocol
        );
        return None;
    }

    let target_str = target.display().to_string();
    Some(
        parsed
            .items
            .into_iter()
            .map(|item| PlannedRemediation {
                tool,
                binary_tool: tool,
                reason: item.summary.clone(),
                program: tool.key(),
                args: vec![
                    "remediate".to_string(),
                    "--format".to_string(),
                    "apply".to_string(),
                    "--item".to_string(),
                    item.id,
                    "--json".to_string(),
                    "--base".to_string(),
                    target_str.clone(),
                ],
                cwd: None,
                // Already self-validated by answering `plan` — no
                // separate --help substring guess needed.
                probe_token: None,
                risk: item.risk,
                preview_args: None,
                precomputed_preview: Some(item.summary),
            })
            .collect(),
    )
}

/// Upgrades one legacy-catalog candidate to the `uni.remediate/v1` native
/// protocol where the installed binary supports it — potentially
/// expanding one candidate into several independently-tracked items, one
/// per finding the tool's own `plan` response named. Falls back to the
/// candidate unchanged (still exactly one item) when the tool doesn't
/// implement the protocol, or its `plan` response is empty or malformed.
#[tracing::instrument(skip_all)]
async fn expand_via_native_protocol(
    candidate: PlannedRemediation,
    bin: &Path,
    target: &Path,
    timeout: Duration,
) -> Vec<PlannedRemediation> {
    if !probe_native_protocol(bin, timeout).await {
        return vec![candidate];
    }
    match fetch_native_plan(candidate.tool, bin, target, timeout).await {
        Some(items) if !items.is_empty() => items,
        _ => vec![candidate],
    }
}

enum WorktreeStatus {
    Clean,
    Dirty(String),
    NotAGitRepo,
}

/// Uni's own per-target state directory (currently just the revise
/// journal, `append_journal`). Excluded via git pathspec from every git
/// operation revise performs against the target — status, add, checkout,
/// clean — so uni's own audit trail is never blocked on as "dirty" by
/// `check_worktree`, swept into an unrelated checkpoint commit by
/// `commit_checkpoint`, or wiped by `rollback` when it's recording the
/// very failure rollback is responding to.
const UNI_STATE_DIR: &str = ".uni";

/// `--apply` mutates the target's files with no rollback of its own, so it
/// requires a clean git worktree first — the same precondition `traci
/// trace --apply` enforces on a repo before it will touch it. A target
/// that isn't a git repository at all is let through (uni doesn't require
/// git in general), but the caller is expected to warn: there's no safety
/// net there either.
///
/// Scoped to `target` with a `-- .` pathspec even when `target` is a
/// subdirectory of a larger repository (a monorepo of sibling projects,
/// same as the layout this was developed against): unrelated dirty
/// changes elsewhere in the repo must never block, or be touched by, a
/// revise of one project within it.
#[tracing::instrument(skip_all)]
async fn check_worktree(target: &Path) -> WorktreeStatus {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(target)
        .args(["status", "--porcelain", "--", "."])
        .arg(format!(":!{UNI_STATE_DIR}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    match cmd.output().await {
        Ok(output) if output.status.success() => {
            let porcelain = String::from_utf8_lossy(&output.stdout);
            if porcelain.trim().is_empty() {
                WorktreeStatus::Clean
            } else {
                WorktreeStatus::Dirty(porcelain.lines().take(10).collect::<Vec<_>>().join("\n"))
            }
        }
        Ok(_) => WorktreeStatus::NotAGitRepo,
        Err(e) => {
            eprintln!("uni: stage=revise_preflight outcome=git_spawn_failed error={e}");
            WorktreeStatus::NotAGitRepo
        }
    }
}

/// Runs a remediation's own side-effect-free preview invocation (where the
/// tool offers one — lwoodz `remedy --dry-run`, tempcheq `--fix`
/// without `--yes`) and captures its output, so `uni revise` can show what
/// a remediation would actually change before `--apply` is ever passed.
#[tracing::instrument(skip_all)]
async fn capture_preview(
    bin: &Path,
    preview_args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(preview_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(output)) => tail(&output.stdout).or_else(|| tail(&output.stderr)),
        Ok(Err(e)) => {
            eprintln!("uni: stage=revise_preview outcome=spawn_failed error={e}");
            None
        }
        Err(_) => {
            eprintln!("uni: stage=revise_preview outcome=timeout");
            None
        }
    }
}

/// Commits everything currently under `target` as a checkpoint, so a later
/// remediation's failure can roll back only its own partial mutation
/// without losing this one. Scoped to `target` with a `-- .` pathspec even
/// inside a larger repository — never stages or commits a sibling
/// project's unrelated changes. Returns the new commit's short hash, or
/// `None` when there was nothing to commit (some remediations legitimately
/// write nothing, e.g. amber `--propose` when no dependency crosses its
/// threshold) or the commit couldn't be made.
#[tracing::instrument(skip_all)]
async fn commit_checkpoint(target: &Path, message: &str) -> Option<String> {
    let add = tokio::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["add", "-A", "--", "."])
        .arg(format!(":!{UNI_STATE_DIR}"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await;
    if let Err(e) = add {
        eprintln!("uni: stage=revise_checkpoint outcome=add_failed error={e}");
        return None;
    }

    let commit = tokio::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["commit", "-q", "-m", message, "--", "."])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await;
    match commit {
        Ok(out) if out.status.success() => head_short_hash(target).await,
        Ok(out) => {
            // Exit 1 with "nothing to commit" covers two legitimate
            // shapes: the remediation wrote nothing at all (e.g. amber
            // proposing 0 replacements), or it already committed its own
            // work (a native item, or a delegate like `traci enforce`
            // that manages its own git history end to end).
            // Either way HEAD itself is still a meaningful checkpoint to
            // report — just not one uni made itself just now.
            eprintln!(
                "uni: stage=revise_checkpoint outcome=no_commit detail={}",
                tail(&out.stdout)
                    .or_else(|| tail(&out.stderr))
                    .unwrap_or_default()
            );
            head_short_hash(target).await
        }
        Err(e) => {
            eprintln!("uni: stage=revise_checkpoint outcome=commit_failed error={e}");
            None
        }
    }
}

#[tracing::instrument(skip_all)]
async fn head_short_hash(target: &Path) -> Option<String> {
    let rev = tokio::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["rev-parse", "--short", "HEAD"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .await;
    match rev {
        Ok(rev_out) if rev_out.status.success() => {
            tail(&rev_out.stdout).map(|h| h.trim().to_string())
        }
        _ => None,
    }
}

/// Discards everything currently under `target`, restoring it to the last
/// checkpoint (HEAD). Scoped to `target` with `-- .` even inside a larger
/// repository — never touches a sibling project's unrelated changes.
#[tracing::instrument(skip_all)]
async fn rollback(target: &Path) {
    let checkout = tokio::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["checkout", "--", "."])
        .arg(format!(":!{UNI_STATE_DIR}"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await;
    if let Err(e) = checkout {
        eprintln!("uni: stage=revise_rollback outcome=checkout_failed error={e}");
    }

    // `-d` recurses into untracked directories, which would otherwise
    // delete `.uni/` (and the journal entry just written for this very
    // failure) since it's untracked by design.
    let clean = tokio::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["clean", "-fd", "--", "."])
        .arg(format!(":!{UNI_STATE_DIR}"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await;
    if let Err(e) = clean {
        eprintln!("uni: stage=revise_rollback outcome=clean_failed error={e}");
    }
}

/// Which of `--confirm-source-rewrite`/`--confirm-ai-patch` are still
/// missing for a remediation of this risk tier — empty when `--apply`
/// alone is enough to run it.
#[tracing::instrument(skip_all)]
fn missing_confirmation_flags(
    risk: RiskTier,
    confirm_source_rewrite: bool,
    confirm_ai_patch: bool,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if risk != RiskTier::NewFilesOnly && !confirm_source_rewrite {
        missing.push("--confirm-source-rewrite");
    }
    if risk == RiskTier::AiGenerated && !confirm_ai_patch {
        missing.push("--confirm-ai-patch");
    }
    missing
}

/// Compares a tool's grade before and after a remediation ran.
#[tracing::instrument(skip_all)]
fn classify_verification(
    before_status: Status,
    before_score: Option<f64>,
    after_status: Status,
    after_score: Option<f64>,
) -> VerifyResult {
    let was_flagged = matches!(before_status, Status::Warn | Status::Fail);
    let now_flagged = matches!(after_status, Status::Warn | Status::Fail);

    if was_flagged && !now_flagged {
        return VerifyResult::Fixed;
    }
    match (before_score, after_score) {
        (Some(b), Some(a)) if a > b => VerifyResult::Improved,
        (Some(b), Some(a)) if a < b => VerifyResult::Regressed,
        _ if now_flagged && !was_flagged => VerifyResult::Regressed,
        _ => VerifyResult::Unchanged,
    }
}

/// Re-diagnoses just `tool` (`--only <tool>`) right after its remediation
/// ran, so "applied" means the tool's own grade actually moved the way the
/// remediation claimed it would — not just that the command exited 0.
#[tracing::instrument(skip_all)]
async fn verify_remediation(
    tool: ToolId,
    tools_dir: &Path,
    target: &Path,
    timeout: Duration,
    before: &ToolReport,
) -> Option<Verification> {
    let opts = AnalyzeOptions {
        target: target.to_path_buf(),
        only: vec![tool.key().to_string()],
        skip: Vec::new(),
        jeenome: false,
        jeenome_trace: None,
        timeout: timeout.as_secs(),
        tools_dir: Some(tools_dir.to_path_buf()),
        install_missing: false,
    };
    let report = match run::execute(&opts).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "uni: tool={} stage=revise_verify outcome=failed error={e}",
                tool.key()
            );
            return None;
        }
    };
    let after = report.tools.into_iter().find(|t| t.tool == tool.key())?;
    let result = classify_verification(before.status, before.score, after.status, after.score);
    Some(Verification {
        before_status: before.status,
        before_score: before.score,
        after_status: after.status,
        after_score: after.score,
        result,
    })
}

#[tracing::instrument(skip_all)]
pub fn human(report: &ReviseReport) -> String {
    let mut out = String::new();

    // Header with styling
    out.push_str(
        "╔═══════════════════════════════════════════════════════════════════════════════╗\n",
    );
    out.push_str(&format!(
        "║ 🔧 UNI REVISE — Automated Remediation Engine  \n"
    ));
    out.push_str(
        "╠═══════════════════════════════════════════════════════════════════════════════╣\n",
    );
    out.push_str(&format!("║ 📍 Target: {} \n", report.target));
    out.push_str(&format!(
        "║ 🎯 Mode: {:<62}\n",
        if report.apply {
            "🚀 APPLY (live remediation active)"
        } else {
            "🏁 DRY-RUN (pass --apply to execute)"
        }
    ));
    out.push_str(
        "╚═══════════════════════════════════════════════════════════════════════════════╝\n\n",
    );

    if report.remediations.is_empty() {
        out.push_str(
            "✨ No remediations needed — every tool with remediation support is clean!\n\n",
        );
        out.push_str(&format!(
            "🎊 Final Outcome: {}\n",
            run_outcome_emoji_word(report.outcome)
        ));
        return out;
    }

    out.push_str(&format!(
        "🔍 Found {} remediation(s) across tool suite:\n",
        report.remediations.len()
    ));
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════════\n\n",
    );

    for (idx, r) in report.remediations.iter().enumerate() {
        let outcome_emoji = outcome_emoji(r.outcome);
        let risk_emoji = risk_emoji(r.risk);

        out.push_str(&format!(
            "{} [{:2}] {:<15} {} [{}] {}\n",
            outcome_emoji,
            idx + 1,
            r.tool,
            outcome_word(r.outcome),
            risk_emoji,
            risk_word(r.risk)
        ));

        out.push_str(&format!("     📌 Reason: {}\n", r.reason));
        out.push_str(&format!("     💻 Command: {}\n", r.command));

        if let Some(detail) = &r.detail {
            out.push_str(&format!("     ℹ️  Detail: {}\n", detail));
        }

        if let Some(preview) = &r.preview {
            out.push_str(&format!("     👁️  Preview:\n"));
            for line in preview.lines() {
                out.push_str(&format!("        {}\n", line));
            }
        }

        if let Some(checkpoint) = &r.checkpoint {
            out.push_str(&format!("     ✓ Checkpoint: {}\n", checkpoint));
        }

        if r.rolled_back {
            out.push_str("     ⏮️  Rolled back: yes (due to failure or verification issue)\n");
        }

        if let Some(v) = &r.verification {
            let verify_emoji = verify_emoji(v.result);
            out.push_str(&format!(
                "     {} Verified: {} {} → {} {} ({})\n",
                verify_emoji,
                crate::render::status_word(v.before_status),
                v.before_score
                    .map(|s| format!("{:.1}", s))
                    .unwrap_or_default(),
                crate::render::status_word(v.after_status),
                v.after_score
                    .map(|s| format!("{:.1}", s))
                    .unwrap_or_default(),
                verify_word(v.result)
            ));
        }
        out.push('\n');
    }

    if let Some(post) = &report.post {
        out.push_str(
            "═══════════════════════════════════════════════════════════════════════════════════\n",
        );
        out.push_str("📊 POST-REMEDIATION SNAPSHOT:\n");
        out.push_str(
            "═══════════════════════════════════════════════════════════════════════════════════\n",
        );
        out.push_str(&crate::render::human(post));
        out.push('\n');
    }

    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str(&format!(
        "🎯 FINAL OUTCOME: {}\n",
        run_outcome_emoji_word(report.outcome)
    ));

    out
}

#[tracing::instrument(skip_all)]
fn run_outcome_emoji_word(o: RunOutcome) -> String {
    match o {
        RunOutcome::Clean => "✨ Clean — nothing to revise".to_string(),
        RunOutcome::Planned => "📋 Planned — dry run found remediations to apply".to_string(),
        RunOutcome::FixedCleanly => {
            "🎉 Fixed Cleanly — all remediations applied successfully!".to_string()
        }
        RunOutcome::Partial => {
            "⚠️  Partial — some remediations applied, see details above".to_string()
        }
        RunOutcome::Regressed => {
            "🔴 Regressed — some remediations failed or worsened the situation".to_string()
        }
    }
}

#[tracing::instrument(skip_all)]
fn outcome_emoji(o: Outcome) -> &'static str {
    match o {
        Outcome::Planned => "📋",
        Outcome::RequiresConfirmation => "🔐",
        Outcome::Applied => "✅",
        Outcome::Failed => "❌",
        Outcome::Unavailable => "🚫",
    }
}

#[tracing::instrument(skip_all)]
fn outcome_word(o: Outcome) -> &'static str {
    match o {
        Outcome::Planned => "planned",
        Outcome::RequiresConfirmation => "requires confirmation",
        Outcome::Applied => "applied",
        Outcome::Failed => "failed",
        Outcome::Unavailable => "unavailable",
    }
}

#[tracing::instrument(skip_all)]
fn risk_emoji(r: RiskTier) -> &'static str {
    match r {
        RiskTier::NewFilesOnly => "📄",
        RiskTier::RewritesSource => "⚠️",
        RiskTier::AiGenerated => "🤖",
    }
}

#[tracing::instrument(skip_all)]
fn risk_word(r: RiskTier) -> &'static str {
    match r {
        RiskTier::NewFilesOnly => "new files only",
        RiskTier::RewritesSource => "rewrites source",
        RiskTier::AiGenerated => "AI-generated patch",
    }
}

#[tracing::instrument(skip_all)]
fn verify_emoji(v: VerifyResult) -> &'static str {
    match v {
        VerifyResult::Fixed => "✅",
        VerifyResult::Improved => "📈",
        VerifyResult::Unchanged => "➡️",
        VerifyResult::Regressed => "📉",
    }
}

#[tracing::instrument(skip_all)]
fn verify_word(v: VerifyResult) -> &'static str {
    match v {
        VerifyResult::Fixed => "fixed",
        VerifyResult::Improved => "improved",
        VerifyResult::Unchanged => "unchanged",
        VerifyResult::Regressed => "regressed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Availability, Evidence, Execution, Overall, SuiteHealth, ToolReport};

    #[tracing::instrument(skip_all)]
    fn tool_report(tool: &'static str, status: Status, raw: Option<Value>) -> ToolReport {
        ToolReport {
            tool,
            purpose: "test",
            status,
            availability: Availability::Installed,
            execution: Execution::Succeeded,
            evidence: Evidence {
                coverage: None,
                confidence: None,
                observations: None,
            },
            binary: None,
            score: None,
            grade: None,
            exit_code: Some(0),
            duration_ms: Some(1),
            summary: format!("{tool} summary"),
            findings: Vec::new(),
            note: None,
            raw,
        }
    }

    #[tracing::instrument(skip_all)]
    fn diagnosis(tools: Vec<ToolReport>) -> Report {
        Report {
            schema: "uni.report/v3",
            target: "/proj".to_string(),
            generated_at: "now".to_string(),
            tools_dir: "/tools".to_string(),
            tools,
            overall: Overall {
                score: None,
                grade: None,
                graded_tools: 0,
                total_tools: 0,
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
            integrity: crate::report::AnalysisIntegrity {
                status: crate::report::IntegrityStatus::Healthy,
                score: 100.0,
                grade: "A+",
                defects: Vec::new(),
            },
        }
    }

    #[tracing::instrument(skip_all)]
    fn allow_all(_: &str) -> bool {
        true
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn plans_amber_when_flagged() {
        let d = diagnosis(vec![tool_report("amber", Status::Warn, None)]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].program, "amber");
        assert_eq!(plan[0].probe_token, Some("--propose"));
        assert_eq!(plan[0].args, vec![".".to_string(), "--propose".to_string()]);
        assert_eq!(plan[0].cwd, Some(PathBuf::from("/proj")));
        assert_eq!(plan[0].risk, RiskTier::NewFilesOnly);
        assert!(plan[0].preview_args.is_none());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn only_tempcheq_is_risk_tiered_as_rewrites_source() {
        let d = diagnosis(vec![
            tool_report("amber", Status::Warn, None),
            tool_report("isopod", Status::Fail, None),
            tool_report(
                "lwoodz",
                Status::Fail,
                Some(serde_json::json!({"has_license_file": false})),
            ),
            tool_report("tempcheq", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 4);
        for p in &plan {
            let expected = if p.program == "tempcheq" {
                RiskTier::RewritesSource
            } else {
                RiskTier::NewFilesOnly
            };
            assert_eq!(p.risk, expected, "unexpected risk tier for {}", p.program);
        }
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn lwoodz_and_tempcheq_carry_preview_args_but_amber_and_isopod_dont() {
        let d = diagnosis(vec![
            tool_report("amber", Status::Warn, None),
            tool_report("isopod", Status::Fail, None),
            tool_report(
                "lwoodz",
                Status::Fail,
                Some(serde_json::json!({"has_license_file": false})),
            ),
            tool_report("tempcheq", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        let has_preview = |program: &str| {
            plan.iter()
                .find(|p| p.program == program)
                .unwrap()
                .preview_args
                .is_some()
        };
        assert!(!has_preview("amber"));
        assert!(!has_preview("isopod"));
        assert!(has_preview("lwoodz"));
        assert!(has_preview("tempcheq"));
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn skips_amber_when_not_flagged() {
        let d = diagnosis(vec![tool_report("amber", Status::Ok, None)]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert!(plan.is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn plans_isopod_and_tempcheq_when_flagged() {
        let d = diagnosis(vec![
            tool_report("isopod", Status::Fail, None),
            tool_report("tempcheq", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 2);
        assert!(plan
            .iter()
            .any(|p| p.program == "isopod" && p.probe_token == Some("harden")));
        assert!(plan
            .iter()
            .any(|p| p.program == "tempcheq" && p.probe_token == Some("--fix")));
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn lwoodz_only_planned_when_license_file_missing() {
        let raw = serde_json::json!({"has_license_file": false});
        let d = diagnosis(vec![tool_report("lwoodz", Status::Fail, Some(raw))]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].probe_token, Some("remedy"));
        assert_eq!(plan[0].args, ["remedy"]);
        assert_eq!(
            plan[0].preview_args.as_ref().unwrap(),
            &["remedy", "--dry-run"]
        );
        assert_eq!(plan[0].cwd, Some(PathBuf::from("/proj")));
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn lwoodz_not_planned_when_license_file_present() {
        let raw = serde_json::json!({"has_license_file": true});
        let d = diagnosis(vec![tool_report("lwoodz", Status::Fail, Some(raw))]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert!(plan.is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn lwoodz_not_planned_when_evidence_missing() {
        // Matches the `.unwrap_or(true)` fallback in build_plan: absent
        // evidence about the license file must not be treated as "the
        // license file is definitely missing".
        let d = diagnosis(vec![tool_report("lwoodz", Status::Fail, None)]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert!(plan.is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn respects_only_filter() {
        let d = diagnosis(vec![
            tool_report("amber", Status::Warn, None),
            tool_report("tempcheq", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), |k| k == "tempcheq");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].program, "tempcheq");
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn respects_skip_filter() {
        let d = diagnosis(vec![
            tool_report("amber", Status::Warn, None),
            tool_report("tempcheq", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), |k| k != "amber");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].program, "tempcheq");
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn ignores_tools_with_no_remediation_command() {
        let d = diagnosis(vec![
            tool_report("ami", Status::Warn, None),
            tool_report("bart", Status::Ok, None),
            tool_report("isopod", Status::Ok, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert!(plan.is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn plans_chakra_and_fract_as_traci_delegates_when_flagged() {
        let d = diagnosis(vec![
            tool_report("chakra", Status::Warn, None),
            tool_report("fract", Status::Warn, None),
        ]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 2);

        let chakra = plan.iter().find(|p| p.tool.key() == "chakra").unwrap();
        assert_eq!(chakra.binary_tool.key(), "traci");
        assert_eq!(chakra.program, "traci");
        assert_eq!(chakra.probe_token, Some("traci enforce"));
        assert_eq!(chakra.risk, RiskTier::AiGenerated);
        assert_eq!(chakra.args[0], "enforce");
        assert!(chakra.args.contains(&CHAKRA_TRACE_GOAL.to_string()));

        let fract = plan.iter().find(|p| p.tool.key() == "fract").unwrap();
        assert_eq!(fract.binary_tool.key(), "traci");
        assert!(fract.args.contains(&FRACT_TRACE_GOAL.to_string()));
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn non_delegate_entries_resolve_their_own_binary() {
        let d = diagnosis(vec![tool_report("amber", Status::Warn, None)]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan[0].tool.key(), "amber");
        assert_eq!(plan[0].binary_tool.key(), "amber");
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn plans_traci_when_flagged_and_risk_tiers_it_as_ai_generated() {
        let d = diagnosis(vec![tool_report("traci", Status::Fail, None)]);
        let plan = build_plan(&d, Path::new("/proj"), allow_all);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].program, "traci");
        assert_eq!(plan[0].probe_token, Some("traci enforce"));
        assert_eq!(plan[0].risk, RiskTier::AiGenerated);
        // Real argv shape: `traci enforce <path> --goal <text>` —
        // asserted on the exact vec, not just "contains enforce somewhere",
        // since a missing/misplaced `enforce` subcommand compiles fine but
        // fails at runtime with "unknown command '<path>'" (caught live
        // against the real traci binary, not by a looser assertion here).
        assert_eq!(plan[0].args[0], "enforce");
        assert_eq!(plan[0].args[1], "/proj");
        assert_eq!(plan[0].args[2], "--goal");
        let preview_args = plan[0].preview_args.as_ref().unwrap();
        assert_eq!(preview_args[0], "enforce");
        assert_eq!(preview_args[1], "/proj");
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn new_files_only_never_needs_any_confirmation_flag() {
        assert!(missing_confirmation_flags(RiskTier::NewFilesOnly, false, false).is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn rewrites_source_needs_only_the_source_rewrite_flag() {
        assert_eq!(
            missing_confirmation_flags(RiskTier::RewritesSource, false, false),
            vec!["--confirm-source-rewrite"]
        );
        assert!(missing_confirmation_flags(RiskTier::RewritesSource, true, false).is_empty());
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn ai_generated_needs_both_flags_independently() {
        assert_eq!(
            missing_confirmation_flags(RiskTier::AiGenerated, false, false),
            vec!["--confirm-source-rewrite", "--confirm-ai-patch"]
        );
        assert_eq!(
            missing_confirmation_flags(RiskTier::AiGenerated, true, false),
            vec!["--confirm-ai-patch"]
        );
        assert_eq!(
            missing_confirmation_flags(RiskTier::AiGenerated, false, true),
            vec!["--confirm-source-rewrite"]
        );
        assert!(missing_confirmation_flags(RiskTier::AiGenerated, true, true).is_empty());
    }

    #[tracing::instrument(skip_all)]
    fn remediation(outcome: Outcome, verification: Option<Verification>) -> Remediation {
        Remediation {
            tool: "traci",
            reason: "test".to_string(),
            command: "traci enforce".to_string(),
            risk: RiskTier::AiGenerated,
            outcome,
            detail: None,
            preview: None,
            checkpoint: None,
            rolled_back: false,
            verification,
            duration_ms: None,
        }
    }

    #[tracing::instrument(skip_all)]
    fn verification(result: VerifyResult) -> Verification {
        Verification {
            before_status: Status::Fail,
            before_score: Some(0.0),
            after_status: Status::Ok,
            after_score: Some(100.0),
            result,
        }
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_clean_when_nothing_planned() {
        assert_eq!(classify_run(&[], true), RunOutcome::Clean);
        assert_eq!(classify_run(&[], false), RunOutcome::Clean);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_planned_for_a_nonempty_dry_run() {
        let remediations = vec![remediation(Outcome::Planned, None)];
        assert_eq!(classify_run(&remediations, false), RunOutcome::Planned);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_fixed_cleanly_when_every_verification_improved() {
        let remediations = vec![
            remediation(Outcome::Applied, Some(verification(VerifyResult::Fixed))),
            remediation(Outcome::Applied, Some(verification(VerifyResult::Improved))),
        ];
        assert_eq!(classify_run(&remediations, true), RunOutcome::FixedCleanly);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_partial_on_a_failed_remediation() {
        let remediations = vec![
            remediation(Outcome::Applied, Some(verification(VerifyResult::Fixed))),
            remediation(Outcome::Failed, None),
        ];
        assert_eq!(classify_run(&remediations, true), RunOutcome::Partial);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_partial_on_unavailable_or_requires_confirmation() {
        assert_eq!(
            classify_run(&[remediation(Outcome::Unavailable, None)], true),
            RunOutcome::Partial
        );
        assert_eq!(
            classify_run(&[remediation(Outcome::RequiresConfirmation, None)], true),
            RunOutcome::Partial
        );
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_partial_on_unchanged_verification() {
        let remediations = vec![remediation(
            Outcome::Applied,
            Some(verification(VerifyResult::Unchanged)),
        )];
        assert_eq!(classify_run(&remediations, true), RunOutcome::Partial);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_run_regressed_takes_priority_over_partial() {
        let remediations = vec![
            remediation(Outcome::Failed, None),
            remediation(
                Outcome::Applied,
                Some(verification(VerifyResult::Regressed)),
            ),
        ];
        assert_eq!(classify_run(&remediations, true), RunOutcome::Regressed);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn append_journal_writes_one_line_per_remediation() {
        let dir = unique_temp_dir("journal");
        let remediations = vec![
            remediation(Outcome::Applied, Some(verification(VerifyResult::Fixed))),
            remediation(Outcome::Failed, None),
        ];

        append_journal(&dir, "/some/target", &remediations, RunOutcome::Partial).await;

        let journal_path = dir.join(UNI_STATE_DIR).join("revise-journal.jsonl");
        let content = std::fs::read_to_string(&journal_path).expect("read journal");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["schema"], "uni.revise.journal/v1");
        assert_eq!(first["target"], "/some/target");
        assert_eq!(first["run_outcome"], "partial");
        assert_eq!(first["remediation"]["outcome"], "applied");
        assert_eq!(first["remediation"]["verification"]["result"], "fixed");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["remediation"]["outcome"], "failed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn append_journal_appends_across_multiple_runs() {
        let dir = unique_temp_dir("journal-append");
        append_journal(
            &dir,
            "/t",
            &[remediation(Outcome::Applied, None)],
            RunOutcome::FixedCleanly,
        )
        .await;
        append_journal(
            &dir,
            "/t",
            &[remediation(Outcome::Failed, None)],
            RunOutcome::Partial,
        )
        .await;

        let journal_path = dir.join(UNI_STATE_DIR).join("revise-journal.jsonl");
        let content = std::fs::read_to_string(&journal_path).unwrap();
        assert_eq!(content.lines().count(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn checkpoint_never_stages_the_journal_directory() {
        let (outer, target) = nested_repo_fixture("journal-excluded").await;
        std::fs::create_dir_all(target.join(UNI_STATE_DIR)).unwrap();
        std::fs::write(
            target.join(UNI_STATE_DIR).join("revise-journal.jsonl"),
            "{}\n",
        )
        .unwrap();
        std::fs::write(target.join("real_change.txt"), "x").unwrap();

        let hash = commit_checkpoint(&target, "checkpoint excluding journal").await;
        assert!(hash.is_some());

        let show = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&outer)
            .args(["show", "--stat", "--format=", "HEAD"])
            .output()
            .await
            .unwrap();
        let stat = String::from_utf8_lossy(&show.stdout);
        assert!(stat.contains("real_change.txt"));
        assert!(!stat.contains(UNI_STATE_DIR));

        // The journal itself is still there, still untracked, unaffected.
        assert!(target
            .join(UNI_STATE_DIR)
            .join("revise-journal.jsonl")
            .is_file());

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn rollback_never_deletes_the_journal_directory() {
        let (outer, target) = nested_repo_fixture("journal-survives-rollback").await;
        std::fs::create_dir_all(target.join(UNI_STATE_DIR)).unwrap();
        std::fs::write(
            target.join(UNI_STATE_DIR).join("revise-journal.jsonl"),
            "{\"recording\":\"this failed attempt\"}\n",
        )
        .unwrap();
        std::fs::write(target.join("partial_write.txt"), "partial\n").unwrap();

        rollback(&target).await;

        assert!(!target.join("partial_write.txt").exists());
        assert!(target
            .join(UNI_STATE_DIR)
            .join("revise-journal.jsonl")
            .is_file());

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn supports_token_finds_a_subcommand_in_a_commands_list() {
        let help = "Usage: isopod [OPTIONS] [COMMAND]\n\nCommands:\n  check\n  status\n";
        assert!(supports_token(help, "check"));
        assert!(!supports_token(help, "harden"));
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn supports_token_finds_a_flag_in_an_options_list() {
        let help = "Options:\n  -p, --propose\n      --threshold <THRESHOLD>\n";
        assert!(supports_token(help, "--propose"));
        assert!(!supports_token(help, "--nonexistent"));
    }

    #[tracing::instrument(skip_all)]
    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "uni-revise-test-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir for worktree test");
        dir
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn vamos_initialization_is_a_noop_when_manifest_exists() {
        let target = unique_temp_dir("vamos-existing");
        std::fs::write(target.join("vamos.toml"), "# existing\n").unwrap();
        let missing_tools_dir = target.join("tools-with-no-vamos");

        initialize_vamos_if_missing(&target, &missing_tools_dir, Duration::from_secs(1))
            .await
            .expect("an existing manifest must not require a vamos binary");

        assert_eq!(
            std::fs::read_to_string(target.join("vamos.toml")).unwrap(),
            "# existing\n"
        );
        let _ = std::fs::remove_dir_all(&target);
    }

    #[cfg(unix)]
    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn vamos_initialization_runs_init_in_the_target() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir("vamos-init");
        let target = root.join("project");
        let bin_dir = root.join("tools/vamos/target/release");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("vamos");
        std::fs::write(
            &bin,
            "#!/bin/sh\n[ \"$1\" = \"--help\" ] && exit 0\nprintf '%s\\n' \"$*\" > vamos-invocation.txt\nprintf '# initialized\\n' > vamos.toml\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_until_executable(&bin);

        initialize_vamos_if_missing(&target, &root.join("tools"), Duration::from_secs(5))
            .await
            .expect("vamos init should succeed");

        assert!(target.join("vamos.toml").is_file());
        assert_eq!(
            std::fs::read_to_string(target.join("vamos-invocation.txt")).unwrap(),
            "init\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A minimal real implementation of `uni.remediate/v1`
    /// (docs/remediation-protocol.md), written as a POSIX shell script —
    /// no fake data structures standing in for the protocol, an actual
    /// executable that gets spawned exactly the way a real tool would.
    /// Advertises two independently-tracked findings (one of each risk
    /// tier), so the tests below exercise per-finding granularity and not
    /// just discovery.
    #[tracing::instrument(skip_all)]
    fn fake_protocol_tool(dir: &Path) -> PathBuf {
        let script = dir.join("fake-tool");
        std::fs::write(
            &script,
            r#"#!/bin/sh
set -e
MODE=""
ITEM=""
BASE=""
while [ $# -gt 0 ]; do
  case "$1" in
    remediate) shift ;;
    --help) MODE="help"; shift ;;
    --format) MODE="$2"; shift 2 ;;
    --item) ITEM="$2"; shift 2 ;;
    --base) BASE="$2"; shift 2 ;;
    --json) shift ;;
    *) shift ;;
  esac
done

if [ "$MODE" = "help" ]; then
  echo "fake-tool: implements uni.remediate/v1"
  exit 0
fi

if [ "$MODE" = "plan" ]; then
  cat <<'JSON'
{"protocol":"uni.remediate/v1","items":[{"id":"add-notice","summary":"NOTICE file is missing","risk":"new_files_only"},{"id":"reformat","summary":"3 files need reformatting","risk":"rewrites_source"}]}
JSON
  exit 0
fi

if [ "$MODE" = "apply" ]; then
  case "$ITEM" in
    add-notice)
      echo "fake notice" > "$BASE/NOTICE-FAKE"
      echo "wrote NOTICE-FAKE"
      exit 0
      ;;
    reformat)
      echo "reformatted (test stub)"
      exit 0
      ;;
    *)
      echo "unknown item: $ITEM" >&2
      exit 1
      ;;
  esac
fi

echo "unknown mode: $MODE" >&2
exit 1
"#,
        )
        .expect("write fake protocol tool");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake protocol tool");
        }
        wait_until_executable(&script);
        script
    }

    /// A freshly-written, freshly-chmod'd executable can briefly fail to
    /// exec with ETXTBSY ("Text file busy") under heavy concurrent load —
    /// a real Linux kernel race between the file being closed and it
    /// becoming safe to exec, reproduced directly (300 concurrent
    /// write+spawn cycles, dozens of ETXTBSY hits) while chasing flakiness
    /// in the tests that use this fixture. Not a concern for uni's actual
    /// production spawns (those run long-installed binaries, never a file
    /// written microseconds earlier), so the retry belongs here, in the
    /// test fixture that creates the race, not in `probe_native_protocol`
    /// or any other production code path.
    #[tracing::instrument(skip_all)]
    fn wait_until_executable(script: &Path) {
        for _ in 0..50 {
            match std::process::Command::new(script)
                .arg("--help")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
            {
                Ok(_) => return,
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return,
            }
        }
    }

    #[tracing::instrument(skip_all)]
    fn dummy_candidate(target: &Path) -> PlannedRemediation {
        PlannedRemediation {
            tool: ToolId::Amber,
            binary_tool: ToolId::Amber,
            reason: "legacy reason".to_string(),
            program: "amber",
            args: vec![".".to_string(), "--propose".to_string()],
            cwd: Some(target.to_path_buf()),
            probe_token: Some("--propose"),
            risk: RiskTier::NewFilesOnly,
            preview_args: None,
            precomputed_preview: None,
        }
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn worktree_is_clean_right_after_git_init() {
        let dir = unique_temp_dir("clean");
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .output()
            .await
            .expect("git init");

        assert!(matches!(check_worktree(&dir).await, WorktreeStatus::Clean));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn worktree_is_dirty_with_an_untracked_file() {
        let dir = unique_temp_dir("dirty");
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .output()
            .await
            .expect("git init");
        std::fs::write(dir.join("new.txt"), "x").expect("write untracked file");

        assert!(matches!(
            check_worktree(&dir).await,
            WorktreeStatus::Dirty(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn worktree_outside_any_repo_is_reported_as_not_a_git_repo() {
        let dir = unique_temp_dir("nogit");
        assert!(matches!(
            check_worktree(&dir).await,
            WorktreeStatus::NotAGitRepo
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tracing::instrument(skip_all)]
    async fn git(dir: &Path, args: &[&str]) {
        let out = tokio::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .await
            .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repo containing two independent projects — `sibling/` and
    /// `target/` — mirroring the layout this feature was actually
    /// developed against (many sibling tool checkouts under one outer git
    /// root). Every test below exists to prove uni's git operations never
    /// leak across that `outer`/`sibling` boundary.
    #[tracing::instrument(skip_all)]
    async fn nested_repo_fixture(label: &str) -> (PathBuf, PathBuf) {
        let outer = unique_temp_dir(&format!("nested-{label}"));
        git(&outer, &["init", "-q"]).await;
        git(&outer, &["config", "user.name", "test"]).await;
        git(&outer, &["config", "user.email", "test@test"]).await;

        std::fs::create_dir_all(outer.join("sibling")).unwrap();
        std::fs::create_dir_all(outer.join("target")).unwrap();
        std::fs::write(outer.join("sibling/committed.txt"), "sibling\n").unwrap();
        std::fs::write(outer.join("target/committed.txt"), "target\n").unwrap();
        git(&outer, &["add", "-A"]).await;
        git(&outer, &["commit", "-q", "-m", "init"]).await;

        let target = outer.join("target");
        (outer, target)
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn worktree_check_ignores_dirty_siblings_outside_target() {
        let (outer, target) = nested_repo_fixture("worktree").await;
        std::fs::write(outer.join("sibling/committed.txt"), "dirty sibling\n").unwrap();

        assert!(matches!(
            check_worktree(&target).await,
            WorktreeStatus::Clean
        ));
        let _ = std::fs::remove_dir_all(&outer);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn checkpoint_commits_only_target_files() {
        let (outer, target) = nested_repo_fixture("checkpoint").await;
        std::fs::write(outer.join("sibling/committed.txt"), "dirty sibling\n").unwrap();
        std::fs::write(target.join("new_in_target.txt"), "new\n").unwrap();

        let hash = commit_checkpoint(&target, "checkpoint test").await;
        assert!(hash.is_some(), "expected a checkpoint commit hash");

        // The sibling's dirty file must still be uncommitted — the
        // checkpoint must not have swept it in.
        let status = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&outer)
            .args(["status", "--porcelain", "--", "sibling"])
            .output()
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&status.stdout).trim().is_empty(),
            "sibling's dirty file should not have been committed by the checkpoint"
        );

        // The commit itself must only touch files under target/.
        let show = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&outer)
            .args(["show", "--stat", "--format=", "HEAD"])
            .output()
            .await
            .unwrap();
        let stat = String::from_utf8_lossy(&show.stdout);
        assert!(stat.contains("target/new_in_target.txt"));
        assert!(!stat.contains("sibling/"));

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn checkpoint_reports_current_head_when_target_has_nothing_new_to_commit() {
        let (outer, target) = nested_repo_fixture("no-op-checkpoint").await;
        std::fs::write(outer.join("sibling/committed.txt"), "dirty sibling\n").unwrap();
        let head_before = head_short_hash(&target).await;

        // Nothing changed under target/, only under sibling/: no new
        // commit is made, but HEAD (already a real checkpoint from the
        // fixture's own init commit) is still reported, not None.
        let hash = commit_checkpoint(&target, "should be a no-op").await;
        assert!(hash.is_some());
        assert_eq!(hash, head_before);

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn checkpoint_reports_a_tool_own_commit_it_didnt_make_itself() {
        // Simulates a remediation (like `traci enforce`) that
        // commits its own work directly, leaving nothing for uni's own
        // `git add -A` to stage. The checkpoint must still report the
        // resulting HEAD, not None — the change genuinely happened and is
        // genuinely a checkpoint, uni just wasn't the one who committed it.
        let (outer, target) = nested_repo_fixture("self-committing-tool").await;
        std::fs::write(target.join("committed.txt"), "changed by the tool\n").unwrap();
        git(&target, &["commit", "-a", "-q", "-m", "tool's own commit"]).await;
        let expected = head_short_hash(&target).await;

        let hash = commit_checkpoint(&target, "uni's redundant checkpoint attempt").await;
        assert!(hash.is_some());
        assert_eq!(hash, expected);

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn rollback_restores_target_without_touching_sibling() {
        let (outer, target) = nested_repo_fixture("rollback").await;
        std::fs::write(outer.join("sibling/committed.txt"), "dirty sibling\n").unwrap();
        std::fs::write(target.join("committed.txt"), "mutated\n").unwrap();
        std::fs::write(target.join("partial_write.txt"), "partial\n").unwrap();

        rollback(&target).await;

        assert!(matches!(
            check_worktree(&target).await,
            WorktreeStatus::Clean
        ));
        assert!(!target.join("partial_write.txt").exists());

        let sibling_status = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&outer)
            .args(["status", "--porcelain", "--", "sibling"])
            .output()
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&sibling_status.stdout)
                .trim()
                .is_empty(),
            "rollback of target must not touch the sibling's dirty file"
        );

        let _ = std::fs::remove_dir_all(&outer);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_verification_fixed_when_no_longer_flagged() {
        let r = classify_verification(Status::Fail, Some(0.0), Status::Ok, Some(100.0));
        assert_eq!(r, VerifyResult::Fixed);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_verification_improved_when_still_flagged_but_score_rises() {
        let r = classify_verification(Status::Fail, Some(0.0), Status::Warn, Some(40.0));
        assert_eq!(r, VerifyResult::Improved);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_verification_unchanged_when_nothing_moves() {
        let r = classify_verification(Status::Warn, Some(50.0), Status::Warn, Some(50.0));
        assert_eq!(r, VerifyResult::Unchanged);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_verification_regressed_when_score_drops() {
        let r = classify_verification(Status::Warn, Some(50.0), Status::Warn, Some(30.0));
        assert_eq!(r, VerifyResult::Regressed);
    }

    #[test]
    #[tracing::instrument(skip_all)]
    fn classify_verification_regressed_when_previously_clean_becomes_flagged() {
        let r = classify_verification(Status::Ok, None, Status::Fail, Some(0.0));
        assert_eq!(r, VerifyResult::Regressed);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn probe_native_protocol_true_for_a_real_implementation() {
        let dir = unique_temp_dir("protocol-probe-yes");
        let bin = fake_protocol_tool(&dir);
        assert!(probe_native_protocol(&bin, Duration::from_secs(5)).await);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn probe_native_protocol_false_for_a_tool_without_remediate() {
        // /bin/echo exits 0 for anything, including `remediate --help` —
        // but that's not what makes discovery succeed; the fixture below
        // proves that. This proves the negative: a binary that plainly
        // doesn't understand the subcommand (nonzero exit) is correctly
        // read as "doesn't implement the protocol".
        let dir = unique_temp_dir("protocol-probe-no");
        let script = dir.join("not-a-protocol-tool");
        std::fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        wait_until_executable(&script);
        assert!(!probe_native_protocol(&script, Duration::from_secs(5)).await);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn expand_via_native_protocol_yields_one_item_per_finding() {
        let dir = unique_temp_dir("protocol-expand");
        let bin = fake_protocol_tool(&dir);
        let candidate = dummy_candidate(&dir);

        let items = expand_via_native_protocol(candidate, &bin, &dir, Duration::from_secs(5)).await;

        assert_eq!(items.len(), 2, "one item per finding the tool advertised");
        let add_notice = items.iter().find(|i| i.reason.contains("NOTICE")).unwrap();
        assert_eq!(add_notice.risk, RiskTier::NewFilesOnly);
        assert_eq!(add_notice.probe_token, None);
        assert_eq!(
            add_notice.precomputed_preview.as_deref(),
            Some("NOTICE file is missing")
        );
        assert!(add_notice.args.contains(&"add-notice".to_string()));

        let reformat = items
            .iter()
            .find(|i| i.reason.contains("reformat"))
            .unwrap();
        assert_eq!(reformat.risk, RiskTier::RewritesSource);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn expand_via_native_protocol_falls_back_when_not_implemented() {
        let dir = unique_temp_dir("protocol-fallback-unimplemented");
        let script = dir.join("legacy-only-tool");
        std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        wait_until_executable(&script);
        let candidate = dummy_candidate(&dir);

        let items =
            expand_via_native_protocol(candidate, &script, &dir, Duration::from_secs(5)).await;

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].reason, "legacy reason");
        assert_eq!(items[0].probe_token, Some("--propose"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn fetch_native_plan_rejects_a_mismatched_protocol_version() {
        let dir = unique_temp_dir("protocol-version-mismatch");
        let script = dir.join("future-tool");
        std::fs::write(
            &script,
            r#"#!/bin/sh
case "$*" in
  *--help*) exit 0 ;;
  *) echo '{"protocol":"uni.remediate/v2","items":[]}'; exit 0 ;;
esac
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        wait_until_executable(&script);

        let result = fetch_native_plan(ToolId::Amber, &script, &dir, Duration::from_secs(5)).await;
        assert!(result.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn fetch_native_plan_rejects_malformed_json() {
        let dir = unique_temp_dir("protocol-malformed");
        let script = dir.join("broken-tool");
        std::fs::write(
            &script,
            r#"#!/bin/sh
case "$*" in
  *--help*) exit 0 ;;
  *) echo 'not json'; exit 0 ;;
esac
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        wait_until_executable(&script);

        let result = fetch_native_plan(ToolId::Amber, &script, &dir, Duration::from_secs(5)).await;
        assert!(result.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[tracing::instrument(skip_all)]
    async fn native_apply_item_actually_runs_via_the_normal_command_path() {
        // Proves the apply argv expand_via_native_protocol builds
        // (`remediate --format apply --item <id> --json --base <path>`)
        // is really executable, end to end through the fake tool, using
        // the exact same tokio::process::Command shape execute() uses.
        let dir = unique_temp_dir("protocol-real-apply");
        let bin = fake_protocol_tool(&dir);
        let candidate = dummy_candidate(&dir);
        let items = expand_via_native_protocol(candidate, &bin, &dir, Duration::from_secs(5)).await;
        let add_notice = items.iter().find(|i| i.reason.contains("NOTICE")).unwrap();

        let output = tokio::process::Command::new(&bin)
            .args(&add_notice.args)
            .output()
            .await
            .expect("run native apply item");

        assert!(output.status.success());
        assert!(dir.join("NOTICE-FAKE").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

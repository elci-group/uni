// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Correctness validation via `cargo check` and `cargo test --no-run`.
//!
//! These checks are treated as observed facts, independent of UNI's analytic
//! tool scores. A failure here is a hard gate that blocks adoption.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;
use tokio::time::timeout;

use crate::experiments::report::{CorrectnessValidation, ValidationCheck};
use crate::tool;

/// Run correctness checks in `worktree`. Returns a structured result even when
/// checks fail, so the experiment can report the failure rather than crashing.
pub async fn validate(worktree: &Path, timeout_secs: u64) -> CorrectnessValidation {
    let manifest = worktree.join("Cargo.toml");
    if !manifest.is_file() {
        return CorrectnessValidation {
            cargo_check: skipped_check("cargo check", "no Cargo.toml in worktree"),
            cargo_test: skipped_check("cargo test", "no Cargo.toml in worktree"),
        };
    }

    let cargo = tool::resolve_binary_by_name("cargo").unwrap_or_else(|| PathBuf::from("cargo"));
    let check = run_cargo(&cargo, worktree, &["check"], timeout_secs).await;
    let test = run_cargo(&cargo, worktree, &["test", "--no-run"], timeout_secs).await;

    CorrectnessValidation {
        cargo_check: check,
        cargo_test: test,
    }
}

async fn run_cargo(
    cargo: &Path,
    worktree: &Path,
    args: &[&str],
    timeout_secs: u64,
) -> ValidationCheck {
    let command = format!("cargo {}", args.join(" "));
    let command_label = command.clone();
    let mut cmd = Command::new(cargo);
    cmd.current_dir(worktree)
        .args(args)
        .env("CARGO_TARGET_DIR", worktree.join("target-uni-experiment"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let start = Instant::now();
    let result = timeout(Duration::from_secs(timeout_secs), cmd.output()).await;
    let duration_ms = start.elapsed().as_millis();

    match result {
        Ok(Ok(output)) => {
            let success = output.status.success();
            let summary = if success {
                format!("{} passed", command_label)
            } else {
                format!("{} failed", command_label)
            };
            let detail = Some(compact_output(&output));
            ValidationCheck {
                command,
                success,
                duration_ms: Some(duration_ms),
                exit_code: output.status.code(),
                summary,
                detail,
            }
        }
        Ok(Err(e)) => ValidationCheck {
            command,
            success: false,
            duration_ms: Some(duration_ms),
            exit_code: None,
            summary: format!("failed to spawn {command_label}: {e}"),
            detail: None,
        },
        Err(_) => ValidationCheck {
            command,
            success: false,
            duration_ms: Some(duration_ms * 1000),
            exit_code: None,
            summary: format!("{command_label} timed out after {timeout_secs}s"),
            detail: None,
        },
    }
}

fn skipped_check(command: &str, reason: &str) -> ValidationCheck {
    ValidationCheck {
        command: command.to_string(),
        success: true,
        duration_ms: None,
        exit_code: None,
        summary: format!("skipped: {reason}"),
        detail: None,
    }
}

fn compact_output(output: &std::process::Output) -> String {
    let text = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    String::from_utf8_lossy(text)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(20)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(800)
        .collect()
}

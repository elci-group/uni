// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! CLI interface for Kaptaind remediation transaction management.

use crate::kaptaind::{
    persistence, TransactionId, TransactionState,
};
use std::path::Path;

/// Kaptaind CLI commands.
#[derive(Debug)]
pub enum KaptaindCommand {
    /// List all remediation transactions.
    List,
    /// Inspect a specific transaction.
    Inspect { transaction_id: String },
    /// Resume an interrupted remediation.
    Resume { transaction_id: String },
    /// Abort a transaction.
    Abort { transaction_id: String },
    /// Rollback a completed remediation.
    Rollback { transaction_id: String },
}

/// Execute a Kaptaind CLI command.
pub async fn execute_command(
    cmd: KaptaindCommand,
    repo_path: &Path,
) -> Result<String, String> {
    match cmd {
        KaptaindCommand::List => list_transactions(repo_path).await,
        KaptaindCommand::Inspect { transaction_id } => {
            inspect_transaction(repo_path, &transaction_id).await
        }
        KaptaindCommand::Resume { transaction_id } => {
            resume_transaction(repo_path, &transaction_id).await
        }
        KaptaindCommand::Abort { transaction_id } => {
            abort_transaction(repo_path, &transaction_id).await
        }
        KaptaindCommand::Rollback { transaction_id } => {
            rollback_transaction(repo_path, &transaction_id).await
        }
    }
}

/// List all remediation transactions in the repository.
async fn list_transactions(repo_path: &Path) -> Result<String, String> {
    let txns = persistence::list_transactions(repo_path).await?;

    if txns.is_empty() {
        return Ok("✨ No remediation transactions found".to_string());
    }

    let mut output = "📋 Remediation Transactions:\n".to_string();
    output.push_str("━".repeat(60).as_str());
    output.push('\n');

    for txn_id in txns {
        if let Ok(txn) = persistence::load_transaction(&txn_id, repo_path).await {
            output.push_str(&format!(
                "  📌 {} | State: {:?}\n",
                txn_id.as_str(),
                txn.state
            ));
        }
    }

    output.push_str("━".repeat(60).as_str());
    Ok(output)
}

/// Inspect a specific transaction in detail.
async fn inspect_transaction(repo_path: &Path, transaction_id: &str) -> Result<String, String> {
    let txn_id = TransactionId::from_str(transaction_id);
    let txn = persistence::load_transaction(&txn_id, repo_path).await?;

    let mut output = format!("🔍 Transaction Details: {}\n", txn_id.as_str());
    output.push_str("━".repeat(60).as_str());
    output.push('\n');

    output.push_str(&format!("📊 State: {:?}\n", txn.state));
    output.push_str(&format!("🌿 Source Branch: {}\n", txn.source_branch));
    output.push_str(&format!("📍 Source HEAD: {}\n", txn.source_head));
    output.push_str(&format!("🕐 Created: {}\n", txn.created_at));

    if let Some(ref branch) = txn.remediation_branch {
        output.push_str(&format!("🔀 Remediation Branch: {}\n", branch));
    }

    if let Some(ref error) = txn.error {
        output.push_str(&format!("❌ Error: {}\n", error));
    }

    if txn.rolled_back {
        output.push_str("↩️  Rolled Back: Yes\n");
    }

    if let Some(ref commit) = txn.merge_commit {
        output.push_str(&format!("✅ Merge Commit: {}\n", commit));
    }

    output.push_str("\n📝 Audit Log:\n");
    output.push_str("─".repeat(60).as_str());
    output.push('\n');
    for (i, entry) in txn.audit_log.iter().enumerate() {
        output.push_str(&format!("  [{}] {}\n", i + 1, entry));
    }

    output.push_str("━".repeat(60).as_str());
    Ok(output)
}

/// Resume an interrupted remediation transaction.
async fn resume_transaction(repo_path: &Path, transaction_id: &str) -> Result<String, String> {
    let txn_id = TransactionId::from_str(transaction_id);
    let mut txn = persistence::load_transaction(&txn_id, repo_path).await?;

    if txn.state == TransactionState::Failed || txn.state == TransactionState::Aborted {
        txn.log("[RESUME] Attempting to resume transaction");
        persistence::save_transaction(&txn, repo_path).await?;
        Ok(format!(
            "✅ Transaction {} resumption initiated",
            txn_id.as_str()
        ))
    } else {
        Err(format!(
            "❌ Cannot resume transaction in state: {:?}",
            txn.state
        ))
    }
}

/// Abort a transaction.
async fn abort_transaction(repo_path: &Path, transaction_id: &str) -> Result<String, String> {
    let txn_id = TransactionId::from_str(transaction_id);
    let mut txn = persistence::load_transaction(&txn_id, repo_path).await?;

    txn.transition(TransactionState::Aborted);
    txn.log("[ABORT] Transaction aborted by user");

    persistence::save_transaction(&txn, repo_path).await?;
    Ok(format!("✅ Transaction {} aborted", txn_id.as_str()))
}

/// Rollback a completed remediation transaction.
async fn rollback_transaction(repo_path: &Path, transaction_id: &str) -> Result<String, String> {
    let txn_id = TransactionId::from_str(transaction_id);
    let mut txn = persistence::load_transaction(&txn_id, repo_path).await?;

    if txn.state != TransactionState::Merged && txn.state != TransactionState::Ready {
        return Err(format!(
            "❌ Cannot rollback transaction in state: {:?}",
            txn.state
        ));
    }

    crate::kaptaind::rollback_remediation(&mut txn, repo_path).await?;
    persistence::save_transaction(&txn, repo_path).await?;

    Ok(format!("✅ Transaction {} rolled back", txn_id.as_str()))
}

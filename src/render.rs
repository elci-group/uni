// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use std::ops::Range;

use crate::report::{Execution, IntegrityStatus, Report, Status};

pub struct HumanReport {
    pub text: String,
    pub fract_section: Option<Range<usize>>,
}

pub fn human(report: &Report) -> String {
    human_report(report).text
}

pub fn human_report(report: &Report) -> HumanReport {
    let mut out = String::new();
    let mut fract_section = None;

    // Header with emoji and styling
    out.push_str(
        "╔═══════════════════════════════════════════════════════════════════════════════╗\n",
    );
    out.push_str(&format!("║ 📊 UNI ANALYSIS REPORT — {} \n", report.target));
    out.push_str(
        "╠═══════════════════════════════════════════════════════════════════════════════╣\n",
    );
    out.push_str(&format!("║ 🕐 Generated: {:<66}\n", report.generated_at));
    out.push_str(
        "╚═══════════════════════════════════════════════════════════════════════════════╝\n\n",
    );

    let provisional = if report.overall.provisional {
        " ⚠️ PROVISIONAL"
    } else {
        ""
    };

    // Overall health indicator
    let health_emoji = match report.overall.score.map(|s| s).unwrap_or(0.0) {
        s if s >= 90.0 => "🟢",
        s if s >= 70.0 => "🟡",
        s if s >= 50.0 => "🟠",
        _ => "🔴",
    };

    out.push_str(&format!(
        "{} PROJECT HEALTH: {}{}\n",
        health_emoji,
        report
            .overall
            .score
            .map(|score| format!("{} ({score:.1}/100)", report.overall.grade.unwrap_or("—")))
            .unwrap_or_else(|| "ungraded".to_string()),
        provisional
    ));
    out.push_str(&format!(
        "{} ANALYSIS INTEGRITY: {} {} ({:.1}/100)\n",
        integrity_emoji(report.integrity.status),
        integrity_word(report.integrity.status),
        report.integrity.grade,
        report.integrity.score
    ));

    out.push_str(&format!(
        "  📦 Tool Availability:     {}/{} available ({}%)\n",
        report.suite.available_tools,
        report.suite.required_tools,
        ((report.suite.available_tools as f64 / report.suite.required_tools as f64) * 100.0).floor()
            as i32
    ));
    out.push_str(&format!(
        "  ⚙️  Tool Execution:        {}/{} executed ({}%)\n",
        report.suite.executed_tools,
        report.suite.required_tools,
        ((report.suite.executed_tools as f64 / report.suite.required_tools as f64) * 100.0).floor()
            as i32
    ));
    out.push_str(&format!(
        "  ✅ Valid Results:         {}/{} valid ({}%)\n",
        report.suite.valid_results,
        report.suite.required_tools,
        ((report.suite.valid_results as f64 / report.suite.required_tools as f64) * 100.0).floor()
            as i32
    ));
    out.push_str(&format!(
        "  🔍 Analysis Coverage:     {}\n",
        percent(report.suite.analysis_coverage)
    ));
    out.push_str(&format!(
        "  🎯 Confidence Level:      {}\n\n",
        percent(report.suite.confidence)
    ));

    if !report.integrity.defects.is_empty() {
        out.push_str("  ANALYSIS DEFECTS (not project findings):\n");
        for defect in &report.integrity.defects {
            out.push_str(&format!("      • {defect}\n"));
        }
        out.push('\n');
    }

    // Tools table with emojis
    out.push_str(
        "┌─────────────────────────────────────────────────────────────────────────────────┐\n",
    );
    out.push_str("│ 🔧 ANALYSIS TOOLS SUMMARY\n");
    out.push_str(
        "├────────────┬──────────────┬───────┬─────────┬─────────────────────────────────┤\n",
    );
    out.push_str(&format!(
        "│ {:10} │ {:12} │ {:5} │ {:7} │ {:31} │\n",
        "TOOL", "STATUS", "GRADE", "SCORE", "PURPOSE"
    ));
    out.push_str(
        "├────────────┼──────────────┼───────┼─────────┼─────────────────────────────────┤\n",
    );

    for t in &report.tools {
        let status_icon = status_emoji(t.status);
        let status_text = status_word(t.status);
        out.push_str(&format!(
            "│ {:<10} │ {} {:<9} │ {:<5} │ {:>7} │ {:<31} │\n",
            t.tool,
            status_icon,
            status_text,
            t.grade.unwrap_or("—"),
            t.score
                .map(|s| format!("{s:.1}"))
                .unwrap_or_else(|| "—".to_string()),
            t.purpose.chars().take(31).collect::<String>()
        ));
    }
    out.push_str(
        "└────────────┴──────────────┴───────┴─────────┴─────────────────────────────────┘\n\n",
    );

    // Detailed findings for each tool
    out.push_str("📝 DETAILED FINDINGS BY TOOL:\n");
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════════\n\n",
    );

    for t in &report.tools {
        let section_start = out.len();
        if matches!(t.status, Status::Skipped | Status::Unavailable) {
            if let Some(note) = &t.note {
                out.push_str(&format!("  ⊘ {}: {note}\n", t.tool));
            }
            continue;
        }

        let tool_icon = status_emoji(t.status);
        out.push_str(&format!("  {} {}: {}\n", tool_icon, t.tool, t.summary));

        if !t.findings.is_empty() {
            for f in &t.findings {
                out.push_str(&format!("      • {}\n", f));
            }
        }

        if let Some(note) = &t.note {
            out.push_str(&format!("      ℹ️  Note: {}\n", note));
        }
        out.push('\n');

        if t.tool == "fract" && t.execution == Execution::Succeeded {
            fract_section = Some(section_start..out.len());
        }
    }

    // Overall summary with progress bar
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════════\n",
    );
    match report.overall.score {
        Some(score) => {
            let bar = progress_bar(score / 100.0);
            out.push_str(&format!(
                "🏆 OVERALL GRADE: {} ({score:.1}/100)\n",
                report.overall.grade.unwrap_or("—")
            ));
            out.push_str(&format!("   Progress: {}\n", bar));
            out.push_str(&format!(
                "   Graded Tools: {}/{}\n",
                report.overall.graded_tools, report.overall.total_tools
            ));
        }
        None => {
            out.push_str("⚠️  OVERALL: Ungraded — no tool produced a numeric score\n");
        }
    }

    out.push('\n');
    HumanReport {
        text: out,
        fract_section,
    }
}

fn status_emoji(s: Status) -> &'static str {
    match s {
        Status::Ok => "✅",
        Status::Warn => "⚠️",
        Status::Fail => "🔎",
        Status::Error => "💥",
        Status::Skipped => "⊘",
        Status::Unavailable => "🚫",
        Status::NotApplicable => "◯",
        Status::NoData => "❓",
    }
}

pub fn status_word(s: Status) -> &'static str {
    match s {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Fail => "findings",
        Status::Error => "error",
        Status::Skipped => "skipped",
        Status::Unavailable => "unavailable",
        Status::NotApplicable => "n/a",
        Status::NoData => "no_data",
    }
}

fn integrity_emoji(status: IntegrityStatus) -> &'static str {
    match status {
        IntegrityStatus::Healthy => "🟢",
        IntegrityStatus::Degraded => "🟠",
        IntegrityStatus::Failed => "🔴",
    }
}

fn integrity_word(status: IntegrityStatus) -> &'static str {
    match status {
        IntegrityStatus::Healthy => "HEALTHY",
        IntegrityStatus::Degraded => "DEGRADED",
        IntegrityStatus::Failed => "FAILED",
    }
}

fn percent(value: Option<f64>) -> String {
    value
        .map(|value| {
            let pct = value * 100.0;
            let emoji = if pct >= 90.0 {
                "🟢"
            } else if pct >= 70.0 {
                "🟡"
            } else if pct >= 50.0 {
                "🟠"
            } else {
                "🔴"
            };
            format!("{} {:.1}%", emoji, pct)
        })
        .unwrap_or_else(|| "— N/A".to_string())
}

fn progress_bar(ratio: f64) -> String {
    let filled = (ratio * 20.0).round() as usize;
    let empty = 20 - filled;
    let bar = format!(
        "[{}{}] {:.1}%",
        "█".repeat(filled),
        "░".repeat(empty),
        ratio * 100.0
    );
    bar
}

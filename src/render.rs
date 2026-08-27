// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use std::io::{self, IsTerminal};
use std::ops::Range;

use crate::report::{Execution, IntegrityStatus, Report, Status, ToolReport};

#[derive(Clone, Copy)]
struct Style {
    color: bool,
}

impl Style {
    fn stdout() -> Self {
        Self {
            color: io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && !matches!(std::env::var("TERM").as_deref(), Ok("dumb")),
        }
    }

    fn paint(self, code: Option<&str>, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        match (self.color, code) {
            (true, Some(code)) => format!("\x1b[{code}m{text}\x1b[0m"),
            _ => text.to_string(),
        }
    }

    fn tool_name(self, tool: &str, text: impl AsRef<str>) -> String {
        self.paint(tool_accent(tool), text)
    }

    fn status(self, tool: &ToolReport, text: impl AsRef<str>) -> String {
        self.paint(tool_status_code(tool), text)
    }

    fn finding(self, tool: &ToolReport, finding: &str, index: usize) -> String {
        self.paint(finding_code(tool, finding, index), finding)
    }
}

fn tool_accent(tool: &str) -> Option<&'static str> {
    match tool {
        "amber" => Some("1;36"),
        "ami" => Some("1;36"),
        "bart" => Some("1;34"),
        "chakra" => Some("1;38;2;203;166;247"),
        "ferret" => Some("1;36"),
        "fract" => Some("1;38;5;147"),
        "isopod" => Some("1;36"),
        "jeenome" => Some("36"),
        "traci" => Some("36"),
        // These tools intentionally have no native ANSI presentation.
        "lwoodz" | "tempcheq" | "vamos" => None,
        _ => None,
    }
}

fn tool_status_code(tool: &ToolReport) -> Option<&'static str> {
    if matches!(tool.tool, "lwoodz" | "vamos") {
        return None;
    }
    match tool.tool {
        "bart" if tool.status == Status::Ok => Some("1;34"),
        "chakra" if tool.status == Status::Ok => Some("1;38;2;203;166;247"),
        "ferret" => match tool.status {
            Status::Fail | Status::Error => Some("1;31"),
            Status::Warn => Some("1;33"),
            Status::Ok | Status::Skipped | Status::NotApplicable | Status::NoData => Some("2"),
            Status::Unavailable => Some("1;31"),
        },
        "fract" => match tool.status {
            Status::Ok => Some("38;5;76"),
            Status::Warn => Some("38;5;208"),
            Status::Fail | Status::Error | Status::Unavailable => Some("1;38;5;196"),
            Status::Skipped | Status::NotApplicable | Status::NoData => Some("38;5;245"),
        },
        "traci" => traci_status_code(tool),
        _ => match tool.status {
            Status::Ok => Some("1;32"),
            Status::Warn => Some("1;33"),
            Status::Fail | Status::Error | Status::Unavailable => Some("1;31"),
            Status::Skipped | Status::NotApplicable | Status::NoData => Some("2"),
        },
    }
}

fn traci_status_code(tool: &ToolReport) -> Option<&'static str> {
    let details = format!("{} {}", tool.summary, tool.findings.join(" ")).to_lowercase();
    if details.contains("critical") {
        Some("1;35")
    } else if details.contains("error") || matches!(tool.status, Status::Fail | Status::Error) {
        Some("1;31")
    } else if details.contains("warning") || tool.status == Status::Warn {
        Some("1;33")
    } else if details.contains("info") {
        Some("1;34")
    } else if tool.status == Status::Ok {
        Some("1;32")
    } else {
        Some("2")
    }
}

fn finding_code(tool: &ToolReport, finding: &str, index: usize) -> Option<&'static str> {
    let finding = finding.to_lowercase();
    match tool.tool {
        "amber" if finding.contains("security_block") || finding.contains("block") => Some("1;31"),
        "amber" if finding.contains("propose") || finding.contains("review") => Some("33"),
        "amber" if finding.contains("proceed") => Some("32"),
        "amber" => Some("93"),
        "ami" => Some("34"),
        "bart" => Some(["34", "36", "32", "33", "35"][index % 5]),
        "chakra" if finding.contains("observed") => Some("1;32"),
        "chakra" if finding.contains("derived") => Some("1;36"),
        "chakra" if finding.contains("inferred") => Some("1;35"),
        "chakra" if finding.contains("projected") => Some("1;34"),
        "chakra" => Some("1;33"),
        "ferret" if finding.contains("[critical]") || finding.contains("[major]") => Some("1;31"),
        "ferret" if finding.contains("[minor]") => Some("1;33"),
        "ferret" if finding.contains("[info]") => Some("2"),
        "ferret" => Some("36"),
        "fract" if finding.contains("critical") => Some("1;38;5;196"),
        "fract" if finding.contains("warning") => Some("38;5;208"),
        "fract" => Some("38;5;153"),
        "isopod" if finding.contains("pass") => Some("32"),
        "isopod" if finding.contains("fail") => Some("31"),
        "isopod" if finding.contains("warn") => Some("33"),
        "isopod" if finding.contains("unknown") => Some("2"),
        "jeenome" if finding.contains("filesystem") => Some("34"),
        "jeenome" if finding.contains("network") => Some("32"),
        "jeenome" if finding.contains("process") => Some("35"),
        "jeenome" if finding.contains("memory") => Some("33"),
        "jeenome" if finding.contains("signal") || finding.contains("error") => Some("31"),
        "jeenome" if finding.contains("timing") => Some("36"),
        "tempcheq" => tool_status_code(tool),
        "traci" if finding.contains("critical") => Some("1;35"),
        "traci" if finding.contains("error") => Some("1;31"),
        "traci" if finding.contains("warning") => Some("1;33"),
        "traci" if finding.contains("info") => Some("1;34"),
        "lwoodz" | "vamos" => None,
        _ => tool_status_code(tool),
    }
}

fn pad_right(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn pad_left(text: &str, width: usize) -> String {
    format!("{text:>width$}")
}

pub struct HumanReport {
    pub text: String,
    pub fract_section: Option<Range<usize>>,
}

pub fn human(report: &Report) -> String {
    human_report(report).text
}

pub fn human_report(report: &Report) -> HumanReport {
    human_report_styled(report, Style::stdout())
}

fn human_report_styled(report: &Report, style: Style) -> HumanReport {
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
        let tool_cell = style.tool_name(t.tool, pad_right(t.tool, 10));
        let status_cell = style.status(t, pad_right(&format!("{status_icon} {status_text}"), 12));
        let grade_cell = style.status(t, pad_right(t.grade.unwrap_or("—"), 5));
        let score = t
            .score
            .map(|s| format!("{s:.1}"))
            .unwrap_or_else(|| "—".to_string());
        let score_cell = style.status(t, pad_left(&score, 7));
        out.push_str(&format!(
            "│ {} │ {} │ {} │ {} │ {:<31} │\n",
            tool_cell,
            status_cell,
            grade_cell,
            score_cell,
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
                out.push_str(&format!(
                    "  {} {}: {}\n",
                    style.status(t, "⊘"),
                    style.tool_name(t.tool, t.tool),
                    style.status(t, note)
                ));
            }
            continue;
        }

        let tool_icon = status_emoji(t.status);
        out.push_str(&format!(
            "  {} {}: {}\n",
            style.status(t, tool_icon),
            style.tool_name(t.tool, t.tool),
            style.status(t, &t.summary)
        ));

        if !t.findings.is_empty() {
            for (index, f) in t.findings.iter().enumerate() {
                out.push_str(&format!("      • {}\n", style.finding(t, f, index)));
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

/// A cohort run can cover ~100 repos, so this is deliberately a compact
/// one-line-per-repo table rather than the full per-tool detail block
/// `human_report` renders for a single project — that detail lives in each
/// repo's own JSON file under `--cohort-out` instead.
pub fn human_cohort_report(report: &crate::report::CohortReport) -> String {
    use crate::report::CohortRepoStatus;

    let mut out = String::new();
    out.push_str(&format!(
        "uni cohort — {} ({} discovered, {} locally available)\n",
        report.org, report.discovered, report.locally_available
    ));
    out.push_str(&format!(
        "batch size {}, {}s between cycles\n\n",
        report.cycle.batch_size, report.cycle.cycle_seconds
    ));

    let name_width = report
        .repos
        .iter()
        .map(|r| r.repo.len())
        .max()
        .unwrap_or(4)
        .max(4);
    out.push_str(&format!(
        "{}  {}  {}  {}\n",
        pad_right("REPO", name_width),
        pad_right("GRADE", 5),
        pad_left("SCORE", 6),
        "INTEGRITY"
    ));

    for repo in &report.repos {
        let (grade, score) = match (repo.overall_grade, repo.overall_score) {
            (Some(g), Some(s)) => (g.to_string(), format!("{s:.1}")),
            _ => ("—".to_string(), "—".to_string()),
        };
        let integrity = match (repo.status, repo.integrity_status) {
            (CohortRepoStatus::NotLocallyAvailable, _) => "not locally available".to_string(),
            (_, Some(status)) => format!("{} {}", integrity_emoji(status), integrity_word(status)),
            (_, None) => "—".to_string(),
        };
        out.push_str(&format!(
            "{}  {}  {}  {}\n",
            pad_right(&repo.repo, name_width),
            pad_right(&grade, 5),
            pad_left(&score, 6),
            integrity
        ));
    }

    out.push_str(&format!(
        "\n{} graded, mean score {}\n",
        report.rollup.graded_count,
        report
            .rollup
            .mean_score
            .map(|s| format!("{s:.1}"))
            .unwrap_or_else(|| "N/A".to_string())
    ));
    if !report.rollup.worst.is_empty() {
        out.push_str(&format!("worst: {}\n", report.rollup.worst.join(", ")));
    }
    if !report.rollup.integrity_failures.is_empty() {
        out.push_str(&format!(
            "integrity issues: {}\n",
            report.rollup.integrity_failures.join(", ")
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Availability, Evidence};

    fn tool_report(tool: &'static str, status: Status, findings: Vec<String>) -> ToolReport {
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
            summary: "summary".to_string(),
            findings,
            note: None,
            raw: None,
        }
    }

    #[test]
    fn native_brand_accents_are_distinct() {
        let style = Style { color: true };
        assert_eq!(style.tool_name("bart", "bart"), "\x1b[1;34mbart\x1b[0m");
        assert_eq!(
            style.tool_name("chakra", "chakra"),
            "\x1b[1;38;2;203;166;247mchakra\x1b[0m"
        );
        assert_eq!(
            style.tool_name("fract", "fract"),
            "\x1b[1;38;5;147mfract\x1b[0m"
        );
    }

    #[test]
    fn findings_use_the_source_tools_severity_palettes() {
        let style = Style { color: true };
        let amber = tool_report("amber", Status::Warn, vec![]);
        let ferret = tool_report("ferret", Status::Fail, vec![]);
        let traci = tool_report("traci", Status::Warn, vec![]);
        assert_eq!(
            style.finding(&amber, "serde: security_block", 0),
            "\x1b[1;31mserde: security_block\x1b[0m"
        );
        assert_eq!(
            style.finding(&ferret, "[Minor] large hunk", 0),
            "\x1b[1;33m[Minor] large hunk\x1b[0m"
        );
        assert_eq!(
            style.finding(&traci, "[observability/error] TRC001", 0),
            "\x1b[1;31m[observability/error] TRC001\x1b[0m"
        );
    }

    #[test]
    fn plain_native_tools_and_disabled_color_emit_no_ansi() {
        let lwoodz = tool_report("lwoodz", Status::Error, vec![]);
        assert_eq!(Style { color: true }.status(&lwoodz, "error"), "error");
        assert_eq!(
            Style { color: false }.tool_name("chakra", "chakra"),
            "chakra"
        );
    }
}

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use std::ops::Range;

use form3::ansi::{self, AnsiColor, Attr, Color};
use form3::term::TermInfo;

use crate::report::{Execution, IntegrityStatus, Report, Status, ToolReport};

#[derive(Clone, Copy)]
struct Style {
    color: bool,
}

impl Style {
    fn stdout() -> Self {
        Self {
            color: TermInfo::detect().supports_color(),
        }
    }
    /// Parses one of this module's SGR code strings — always one of a bare
    /// standard/bright number (`"33"`), a bare attribute (`"1"`, `"2"`), a
    /// `;`-joined combination of those (`"1;36"`), a 256-color foreground
    /// (`"38;5;147"`), a 256-color foreground plus bold (`"1;38;5;196"`), or
    /// a truecolor foreground (`"38;2;203;166;247"`) — into the structured
    /// color/attributes `form3::ansi` renders from. This is the same
    /// decomposition Fract's own `report::style::parse_sgr` performs on its
    /// glass palette, so Uni's mechanism for turning a native tool's SGR
    /// string into terminal bytes now matches the source tool's own.
    fn parse_sgr(code: &str) -> (Option<AnsiColor>, Vec<Attr>) {
        const STANDARD: [Color; 8] = [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::White,
        ];

        let mut color = None;
        let mut attrs = Vec::new();
        let parts: Vec<&str> = code.split(';').collect();
        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "1" => attrs.push(Attr::Bold),
                "2" => attrs.push(Attr::Dim),
                "3" => attrs.push(Attr::Italic),
                "4" => attrs.push(Attr::Underline),
                "38" if parts.get(i + 1) == Some(&"5") => {
                    if let Some(n) = parts.get(i + 2).and_then(|s| s.parse().ok()) {
                        color = Some(AnsiColor::Indexed(n));
                    }
                    i += 2;
                }
                "38" if parts.get(i + 1) == Some(&"2") => {
                    if let (Some(r), Some(g), Some(b)) = (
                        parts.get(i + 2).and_then(|s| s.parse().ok()),
                        parts.get(i + 3).and_then(|s| s.parse().ok()),
                        parts.get(i + 4).and_then(|s| s.parse().ok()),
                    ) {
                        color = Some(AnsiColor::Rgb(r, g, b));
                    }
                    i += 4;
                }
                n => {
                    if let Ok(code) = n.parse::<u8>() {
                        let (base, bright) = match code {
                            30..=37 => (code - 30, false),
                            90..=97 => (code - 90, true),
                            _ => {
                                i += 1;
                                continue;
                            }
                        };
                        color = Some(AnsiColor::Standard(STANDARD[base as usize], bright));
                    }
                }
            }
            i += 1;
        }
        (color, attrs)
    }

    fn paint(self, code: Option<&str>, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        match (self.color, code) {
            (true, Some(code)) => {
                let (color, attrs) = Self::parse_sgr(code);
                let mut out = String::new();
                if let Some(color) = &color {
                    out.push_str(&ansi::fg(color));
                }
                for attr in attrs {
                    out.push_str(&ansi::sgr(attr));
                }
                out.push_str(text);
                out.push_str(ansi::reset());
                out
            }
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
    /// A tool's metaphorical analog, dimmed and italicized so it reads as an
    /// aside next to the tool's (accented) name rather than competing with
    /// it.
    fn metaphor(self, text: impl AsRef<str>) -> String {
        self.paint(Some("2;3"), text)
    }
    /// A section heading: bold, no color.
    fn heading(self, text: impl AsRef<str>) -> String {
        self.paint(Some("1"), text)
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
        "scrawny" => Some("33"),
        "wilder" => Some("32"),
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
        "wilder" if finding.contains("[critical/") => Some("1;31"),
        "wilder" if finding.contains("[high/") => Some("31"),
        "wilder" if finding.contains("[medium/") => Some("33"),
        "wilder" => Some("2"),
        "scrawny" => Some("33"),
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
        "viva-palestina" if finding.contains("[exclude]") => Some("1;31"),
        "viva-palestina" if finding.contains("[review]") => Some("33"),
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

/// Fixed category order per tool, for the tools whose finding strings carry
/// an inferable category (a severity tag, a keep/replace verdict, an event
/// kind). Order matches each tool's own severity/priority ranking, so a
/// grouped section reads worst-first.
fn category_order(tool: &str) -> &'static [&'static str] {
    match tool {
        "ferret" => &["Critical", "Major", "Minor", "Info"],
        "traci" => &["Critical", "Error", "Warning", "Info"],
        "amber" => &["Keep (security-block)", "Propose (replace)"],
        "isopod" => &["Fail", "Warn"],
        "viva-palestina" => &["Exclude", "Review"],
        "jeenome" => &[
            "Signal/Error",
            "Filesystem",
            "Network",
            "Process",
            "Memory",
            "Timing",
        ],
        _ => &[],
    }
}

/// Classifies a single finding string into one of `category_order(tool)`'s
/// labels, from the same substrings `finding_code` already keys its ANSI
/// color on — grouping and coloring read the same signal, so a category
/// header and its bullets are never coded differently.
fn categorize(tool: &str, finding: &str) -> Option<&'static str> {
    let lower = finding.to_lowercase();
    match tool {
        "ferret" => {
            if lower.starts_with("[critical]") {
                Some("Critical")
            } else if lower.starts_with("[major]") {
                Some("Major")
            } else if lower.starts_with("[minor]") {
                Some("Minor")
            } else if lower.starts_with("[info]") {
                Some("Info")
            } else {
                None
            }
        }
        "traci" => {
            if lower.contains("observability/critical") {
                Some("Critical")
            } else if lower.contains("observability/error") {
                Some("Error")
            } else if lower.contains("observability/warning") {
                Some("Warning")
            } else if lower.contains("observability/info") {
                Some("Info")
            } else {
                None
            }
        }
        "amber" => {
            if lower.contains("security_block") {
                Some("Keep (security-block)")
            } else if lower.contains("propose") {
                Some("Propose (replace)")
            } else {
                None
            }
        }
        "isopod" => {
            if finding.starts_with("FAIL:") {
                Some("Fail")
            } else if finding.starts_with("WARN:") {
                Some("Warn")
            } else {
                None
            }
        }
        "viva-palestina" => {
            if lower.contains("[exclude]") {
                Some("Exclude")
            } else if lower.contains("[review]") {
                Some("Review")
            } else {
                None
            }
        }
        "jeenome" => {
            if lower.contains("signal") || lower.contains("error") {
                Some("Signal/Error")
            } else if lower.contains("filesystem") {
                Some("Filesystem")
            } else if lower.contains("network") {
                Some("Network")
            } else if lower.contains("process") {
                Some("Process")
            } else if lower.contains("memory") {
                Some("Memory")
            } else if lower.contains("timing") {
                Some("Timing")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Groups a tool's findings by category, preserving each finding's original
/// order within its category. `None` when fewer than two categories actually
/// have members — a single populated bucket is just the flat list with extra
/// ceremony, so grouping only kicks in once it separates something.
fn grouped_findings<'a>(
    tool: &str,
    findings: &'a [String],
) -> Option<Vec<(&'static str, Vec<&'a String>)>> {
    let order = category_order(tool);
    if order.is_empty() {
        return None;
    }
    let mut groups: Vec<(&'static str, Vec<&'a String>)> =
        order.iter().map(|&c| (c, Vec::new())).collect();
    for f in findings {
        if let Some(cat) = categorize(tool, f) {
            if let Some(g) = groups.iter_mut().find(|(c, _)| *c == cat) {
                g.1.push(f);
            }
        }
    }
    groups.retain(|(_, items)| !items.is_empty());
    if groups.len() < 2 {
        None
    } else {
        Some(groups)
    }
}

/// Renders a compact two-column box table of `(label, count)` rows, indented
/// to sit under a tool's finding bullets.
fn count_table(header: &str, rows: &[(String, usize)]) -> String {
    let label_width = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(header.chars().count());
    let rule = "─".repeat(label_width + 2);
    let mut out = String::new();
    out.push_str(&format!("      ┌{rule}┬───────┐\n"));
    out.push_str(&format!("      │ {header:<label_width$} │ COUNT │\n"));
    out.push_str(&format!("      ├{rule}┼───────┤\n"));
    for (label, count) in rows {
        out.push_str(&format!("      │ {label:<label_width$} │ {count:>5} │\n"));
    }
    out.push_str(&format!("      └{rule}┴───────┘\n"));
    out
}

/// Chakra's findings are already one aggregate line per data-flow provenance
/// (`"N flow(s) with provenance=X"`) rather than individual events — a table
/// of provenance vs. flow count says the same thing a bulleted list would,
/// more legibly, so this replaces the bullets instead of grouping them.
fn chakra_provenance_table(findings: &[String]) -> Option<String> {
    let rows: Vec<(String, usize)> = findings
        .iter()
        .filter_map(|f| {
            let (count, provenance) = f.split_once(" flow(s) with provenance=")?;
            Some((provenance.to_string(), count.parse().ok()?))
        })
        .collect();
    if rows.len() < 2 {
        return None;
    }
    Some(count_table("PROVENANCE", &rows))
}

pub struct HumanReport {
    pub text: String,
    pub fract_section: Option<Range<usize>>,
}

pub fn human(report: &Report) -> String {
    technical_report(report).text
}

/// The detailed, jargon-heavy rendering: tool names, raw finding strings,
/// severity tables, ANSI accents. Opt in via `--technical`; pairs well with
/// `--json` for programmatic/model consumption. See [`plain_report`] for the
/// concise, plain-language default.
pub fn technical_report(report: &Report) -> HumanReport {
    human_report_styled(report, Style::stdout())
}

/// Deterministic tool → plain-language area mapping, so translating the
/// technical report into the friendly default never needs a generative
/// call — it's a fixed lookup, same as `tool_accent`/`category_order` above.
fn friendly_label(tool: &'static str) -> &'static str {
    match tool {
        "amber" => "Dependency health",
        "ami" => "Project profile completeness",
        "bart" => "File organization",
        "chakra" => "Architecture & data flow",
        "ferret" => "Code review findings",
        "fract" => "Code structure & duplication",
        "isopod" => "Security compliance (ISO 27001)",
        "jeenome" => "Runtime behavior",
        "lwoodz" => "License compliance",
        "tempcheq" => "AI response consistency",
        "traci" => "Monitoring & observability",
        "vamos" => "Task completion accuracy",
        "viva-palestina" => "Vendor & dependency ethics",
        other => other,
    }
}

fn friendly_emoji(status: Status) -> &'static str {
    match status {
        Status::Ok => "✅",
        Status::Warn => "⚠️",
        Status::Fail => "❗",
        Status::Error => "💥",
        Status::Skipped | Status::NotApplicable => "⏭️",
        Status::Unavailable => "🚫",
        Status::NoData => "❔",
    }
}

fn friendly_status_word(status: Status) -> &'static str {
    match status {
        Status::Ok => "looks good",
        Status::Warn => "a few notes",
        Status::Fail => "needs attention",
        Status::Error => "couldn't complete",
        Status::Skipped => "skipped",
        Status::Unavailable => "not available",
        Status::NotApplicable => "not applicable",
        Status::NoData => "no data yet",
    }
}

/// Turns a tool's findings into a short plain-English count, reusing the
/// same category buckets the technical report groups by (so the two views
/// never disagree about how many "major" vs "minor" items there are) —
/// still no generative call, just a different rendering of the same counts.
fn finding_count_phrase(t: &ToolReport) -> Option<String> {
    if t.findings.is_empty() {
        return None;
    }
    if let Some(groups) = grouped_findings(t.tool, &t.findings) {
        let parts: Vec<String> = groups
            .iter()
            .map(|(label, items)| format!("{} {}", items.len(), label.to_lowercase()))
            .collect();
        Some(format!("({})", parts.join(", ")))
    } else {
        let n = t.findings.len();
        Some(format!("({n} item{})", if n == 1 { "" } else { "s" }))
    }
}

fn overall_headline(score: Option<f64>) -> (&'static str, &'static str) {
    match score {
        Some(s) if s >= 90.0 => ("Excellent — this project is in great shape", "🟢"),
        Some(s) if s >= 70.0 => ("Good — a few things worth a look", "🟡"),
        Some(s) if s >= 50.0 => ("Fair — several areas need attention", "🟠"),
        Some(_) => (
            "Needs attention — multiple significant issues found",
            "🔴",
        ),
        None => ("Not enough data to score this project yet", "❔"),
    }
}

/// The concise, plain-language default: what a non-technical reader needs
/// to know (is the project healthy, what areas need a look, can the
/// snapshot be trusted) without tool names, raw finding strings, or
/// severity tables. Every value here already exists on `Report` — this is
/// purely a deterministic re-rendering, not a second analysis pass, so it
/// adds no generative calls beyond whatever produced the report itself.
/// See [`technical_report`] for the detailed breakdown (`--technical`).
pub fn plain_report(report: &Report) -> HumanReport {
    let mut out = String::new();

    out.push_str(&format!("Project report — {}\n", report.target));
    out.push_str(&format!("Generated {}\n\n", report.generated_at));

    let (headline, emoji) = overall_headline(report.overall.score);
    match report.overall.score {
        Some(score) => out.push_str(&format!("{emoji} Overall: {headline} ({score:.0}/100)\n")),
        None => out.push_str(&format!("{emoji} Overall: {headline}\n")),
    }
    if report.overall.provisional {
        out.push_str(
            "   Note: some checks are missing or incomplete, so this could change once they run.\n",
        );
    }
    out.push('\n');

    out.push_str("What we found, by area:\n");
    for t in &report.tools {
        if matches!(t.status, Status::Skipped | Status::Unavailable) {
            continue;
        }
        let counts = finding_count_phrase(t)
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {} {} — {}{counts}\n",
            friendly_emoji(t.status),
            friendly_label(t.tool),
            friendly_status_word(t.status),
        ));
    }
    out.push('\n');

    if report.integrity.status != IntegrityStatus::Healthy {
        out.push_str(&format!(
            "{} Heads up: {} analysis tool(s) had trouble running, so this snapshot may be incomplete.\n\n",
            integrity_emoji(report.integrity.status),
            report.integrity.defects.len(),
        ));
    }

    out.push_str("Run with --technical for the detailed breakdown, or --json for machine-readable output.\n");

    HumanReport {
        text: out,
        fract_section: None,
    }
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
            let table = if t.tool == "chakra" {
                chakra_provenance_table(&t.findings)
            } else {
                None
            };
            match table {
                Some(table) => out.push_str(&table),
                None => match grouped_findings(t.tool, &t.findings) {
                    Some(groups) => {
                        let rows: Vec<(String, usize)> = groups
                            .iter()
                            .map(|(label, items)| (label.to_string(), items.len()))
                            .collect();
                        out.push_str(&count_table("CATEGORY", &rows));
                        for (label, items) in &groups {
                            out.push_str(&format!("      {label}:\n"));
                            for f in items {
                                let index = t.findings.iter().position(|x| &x == f).unwrap_or(0);
                                out.push_str(&format!(
                                    "        • {}\n",
                                    style.finding(t, f, index)
                                ));
                            }
                        }
                    }
                    None => {
                        for (index, f) in t.findings.iter().enumerate() {
                            out.push_str(&format!("      • {}\n", style.finding(t, f, index)));
                        }
                    }
                },
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

    #[test]
    fn ferret_findings_group_by_severity_when_mixed() {
        let findings = vec![
            "[Major] src/main.rs:10 — Avoid panic".to_string(),
            "[Minor] src/lib.rs:20 — Track this TODO".to_string(),
            "[Info] src/lib.rs:30 — Debug print".to_string(),
        ];
        let groups = grouped_findings("ferret", &findings).expect("mixed severities group");
        assert_eq!(
            groups.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec!["Major", "Minor", "Info"]
        );
        assert_eq!(groups[0].1, vec![&findings[0]]);
    }

    #[test]
    fn single_populated_category_does_not_group() {
        let findings = vec!["[Minor] a".to_string(), "[Minor] b".to_string()];
        assert!(grouped_findings("ferret", &findings).is_none());
    }

    #[test]
    fn tools_without_a_category_scheme_never_group() {
        let findings = vec!["src/big.rs: 40.0 KB".to_string()];
        assert!(grouped_findings("bart", &findings).is_none());
    }

    #[test]
    fn amber_groups_keep_versus_propose() {
        let findings = vec![
            "anyhow: security_block (replaceability 66)".to_string(),
            "clap: propose (replaceability 61)".to_string(),
        ];
        let groups = grouped_findings("amber", &findings).expect("two verdicts group");
        assert_eq!(
            groups.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec!["Keep (security-block)", "Propose (replace)"]
        );
    }

    #[test]
    fn viva_palestina_groups_exclude_versus_review() {
        let findings = vec![
            "Microsoft [EXCLUDE] runtime (Conf: 90.0%)".to_string(),
            "SimilarWeb [REVIEW] runtime (Conf: 70.0%)".to_string(),
        ];
        let groups = grouped_findings("viva-palestina", &findings).expect("two verdicts group");
        assert_eq!(
            groups.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec!["Exclude", "Review"]
        );
    }

    #[test]
    fn jeenome_groups_by_event_kind() {
        let findings = vec![
            "filesystem: open(/etc/passwd) denied".to_string(),
            "network: connect() to 10.0.0.1 refused".to_string(),
            "process: fork() spawned pid 4821".to_string(),
        ];
        let groups = grouped_findings("jeenome", &findings).expect("mixed event kinds group");
        assert_eq!(
            groups.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec!["Filesystem", "Network", "Process"]
        );
    }

    #[test]
    fn chakra_provenance_lines_become_a_table() {
        let findings = vec![
            "5 flow(s) with provenance=static".to_string(),
            "2 flow(s) with provenance=dynamic".to_string(),
        ];
        let table = chakra_provenance_table(&findings).expect("two provenance rows table");
        assert!(table.contains("PROVENANCE"));
        assert!(table.contains("static"));
        assert!(table.contains("dynamic"));
        assert!(table.contains('┌') && table.contains('┘'));
    }

    #[test]
    fn chakra_single_provenance_skips_the_table() {
        let findings = vec!["12 flow(s) with provenance=static".to_string()];
        assert!(chakra_provenance_table(&findings).is_none());
    }

    fn minimal_report(tool: ToolReport) -> crate::report::Report {
        crate::report::Report {
            schema: "uni.report/v3",
            target: "t".to_string(),
            generated_at: "now".to_string(),
            tools_dir: "/tmp".to_string(),
            tools: vec![tool],
            overall: crate::report::Overall {
                score: None,
                grade: None,
                graded_tools: 0,
                total_tools: 1,
                weights: vec![],
                provisional: false,
            },
            suite: crate::report::SuiteHealth {
                required_tools: 1,
                available_tools: 1,
                executed_tools: 1,
                valid_results: 1,
                analysis_coverage: None,
                confidence: None,
            },
            integrity: crate::report::AnalysisIntegrity {
                status: crate::report::IntegrityStatus::Healthy,
                score: 100.0,
                grade: "A+",
                defects: vec![],
            },
        }
    }

    #[test]
    fn grouped_findings_render_keeps_every_line_and_a_count_table() {
        let report = tool_report(
            "ferret",
            Status::Fail,
            vec![
                "[Major] a".to_string(),
                "[Minor] b".to_string(),
                "[Info] c".to_string(),
            ],
        );
        let text = human_report_styled(&minimal_report(report), Style { color: false }).text;
        assert!(text.contains("CATEGORY"));
        assert!(text.contains("Major:"));
        assert!(text.contains("[Major] a"));
        assert!(text.contains("[Minor] b"));
        assert!(text.contains("[Info] c"));
    }

    #[test]
    fn plain_report_omits_tool_names_and_raw_findings() {
        let report = tool_report(
            "ferret",
            Status::Fail,
            vec![
                "[Major] src/main.rs:10 — Avoid panic".to_string(),
                "[Minor] src/lib.rs:20 — Track this TODO".to_string(),
            ],
        );
        let text = plain_report(&minimal_report(report)).text;
        // Friendly area label and count summary are present...
        assert!(text.contains("Code review findings"));
        assert!(text.contains("1 major, 1 minor"));
        // ...but the raw tool key and finding text are not.
        assert!(!text.contains("ferret"));
        assert!(!text.contains("src/main.rs:10"));
        assert!(text.contains("--technical"));
    }

    #[test]
    fn plain_report_skips_tools_that_did_not_run() {
        let mut report = tool_report("ami", Status::Skipped, vec![]);
        report.note = Some("opt-in via --only ami".to_string());
        let text = plain_report(&minimal_report(report)).text;
        assert!(!text.contains("Project profile completeness"));
    }

    #[test]
    fn plain_report_headline_tracks_overall_score_band() {
        let mut report = minimal_report(tool_report("fract", Status::Ok, vec![]));
        report.overall.score = Some(95.0);
        assert!(plain_report(&report).text.contains("Excellent"));

        report.overall.score = Some(55.0);
        assert!(plain_report(&report).text.contains("Fair"));

        report.overall.score = Some(10.0);
        assert!(plain_report(&report).text.contains("Needs attention"));
    }

    #[test]
    fn plain_report_flags_incomplete_analysis() {
        let mut report = minimal_report(tool_report("fract", Status::Ok, vec![]));
        report.integrity.status = crate::report::IntegrityStatus::Degraded;
        report.integrity.defects = vec!["bart: crashed".to_string()];
        let text = plain_report(&report).text;
        assert!(text.contains("Heads up"));
        assert!(text.contains("may be incomplete"));
    }
}

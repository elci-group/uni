// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! The report shape. Every field here is a struct field (not a HashMap), and
//! `tools` is always built by walking `ToolId::ALL` in order — so the same
//! target, run twice, produces byte-identical JSON key/array ordering. Only
//! `generated_at` and each tool's `duration_ms` vary between runs.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Error,
    Skipped,
    Unavailable,
    NotApplicable,
    NoData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Installed,
    Installable,
    Incompatible,
    Unavailable,
    NotChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Execution {
    Succeeded,
    Failed,
    Skipped,
    NotRun,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub coverage: Option<f64>,
    pub confidence: Option<f64>,
    pub observations: Option<u64>,
}

/// Coverage below this fraction of the codebase means a tool's own
/// healthy/warning/finding status can no longer stand for a repository-wide
/// verdict (ELCI-DSEQ-EITR-001 §6-§7). Shared by `EvidenceState::from_tool`
/// and `Report::compute_overall`'s `provisional` check so the two never
/// disagree about what counts as "enough".
pub const MIN_SUFFICIENT_COVERAGE: f64 = 0.8;

/// The canonical nine-state evidence vocabulary from ELCI-DSEQ-EITR-001 §3.
/// This is a *view* computed from a `ToolReport`'s existing `Status` plus
/// `Evidence.coverage` — see `EvidenceState::from_tool` for why `Status`
/// itself is left unrenamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Healthy,
    Warning,
    Finding,
    Unknown,
    Inapplicable,
    Skipped,
    InsufficientCoverage,
    Blocked,
    Error,
}

impl EvidenceState {
    /// Maps uni's existing `Status` onto the canonical nine-state
    /// vocabulary. `Status` already carries seven of the nine states under
    /// matching or closely-analogous names — Ok→Healthy, Warn→Warning,
    /// Fail→Finding, Skipped→Skipped, NotApplicable→Inapplicable,
    /// NoData→Unknown, Unavailable→Blocked (a missing/incompatible binary is
    /// exactly a "prerequisite that prevented execution", §3.8) — and
    /// renaming it would ripple through every parser and every `match
    /// tool.status` in `render.rs`. `InsufficientCoverage` (§3.7) has no
    /// `Status` analogue, so it's derived here from `Evidence.coverage`
    /// instead, and only for tools that otherwise had a headline verdict to
    /// give (Ok/Warn/Fail) — a Skipped/Inapplicable/Blocked/Errored tool is
    /// that regardless of what coverage number it happens to carry.
    pub fn from_tool(t: &ToolReport) -> Self {
        match t.status {
            Status::Error => Self::Error,
            Status::Unavailable => Self::Blocked,
            Status::Skipped => Self::Skipped,
            Status::NotApplicable => Self::Inapplicable,
            Status::NoData => Self::Unknown,
            Status::Fail | Status::Warn | Status::Ok => {
                if t
                    .evidence
                    .coverage
                    .is_some_and(|c| c < MIN_SUFFICIENT_COVERAGE)
                {
                    Self::InsufficientCoverage
                } else {
                    match t.status {
                        Status::Fail => Self::Finding,
                        Status::Warn => Self::Warning,
                        _ => Self::Healthy,
                    }
                }
            }
        }
    }
}

/// Maps a 0-100 health score to a letter grade. Higher is healthier for
/// every tool's `score` by construction (parsers normalize to that
/// convention even when the underlying tool's own numbers point the other
/// way, e.g. amber's "replaceability" score).
pub fn letter_for(score: f64) -> &'static str {
    match score {
        s if s >= 97.0 => "A+",
        s if s >= 93.0 => "A",
        s if s >= 90.0 => "A-",
        s if s >= 87.0 => "B+",
        s if s >= 83.0 => "B",
        s if s >= 80.0 => "B-",
        s if s >= 77.0 => "C+",
        s if s >= 73.0 => "C",
        s if s >= 70.0 => "C-",
        s if s >= 67.0 => "D+",
        s if s >= 63.0 => "D",
        s if s >= 60.0 => "D-",
        _ => "F",
    }
}

#[derive(Debug)]
pub struct ToolReport {
    pub tool: &'static str,
    pub purpose: &'static str,
    pub status: Status,
    pub availability: Availability,
    pub execution: Execution,
    pub evidence: Evidence,
    pub binary: Option<String>,
    pub score: Option<f64>,
    pub grade: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    pub summary: String,
    pub findings: Vec<String>,
    pub note: Option<String>,
    pub raw: Option<serde_json::Value>,
}

impl ToolReport {
    pub fn evidence_state(&self) -> EvidenceState {
        EvidenceState::from_tool(self)
    }
}

/// Hand-written so `evidence_state` — computed from existing fields, not
/// stored — can ride along in JSON without turning it into a 30th
/// construction-site field across every parser (see `EvidenceState::from_tool`).
impl Serialize for ToolReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ToolReport", 16)?;
        s.serialize_field("tool", &self.tool)?;
        s.serialize_field("purpose", &self.purpose)?;
        s.serialize_field("status", &self.status)?;
        s.serialize_field("availability", &self.availability)?;
        s.serialize_field("execution", &self.execution)?;
        s.serialize_field("evidence", &self.evidence)?;
        s.serialize_field("evidence_state", &self.evidence_state())?;
        s.serialize_field("binary", &self.binary)?;
        s.serialize_field("score", &self.score)?;
        s.serialize_field("grade", &self.grade)?;
        s.serialize_field("exit_code", &self.exit_code)?;
        s.serialize_field("duration_ms", &self.duration_ms)?;
        s.serialize_field("summary", &self.summary)?;
        s.serialize_field("findings", &self.findings)?;
        s.serialize_field("note", &self.note)?;
        s.serialize_field("raw", &self.raw)?;
        s.end()
    }
}

#[derive(Debug, Serialize)]
pub struct Overall {
    pub score: Option<f64>,
    pub grade: Option<&'static str>,
    pub graded_tools: usize,
    pub total_tools: usize,
    /// (tool key, weight) pairs, in the same fixed alphabetical order as
    /// `tools`, so weights are auditable without re-deriving them.
    pub weights: Vec<(String, f64)>,
    pub provisional: bool,
    /// Mean `Evidence.coverage` across graded tools — §5: coverage MUST be
    /// visible wherever this score is displayed.
    pub coverage: Option<f64>,
    /// Mean `Evidence.confidence` across graded tools.
    pub confidence: Option<f64>,
    /// §6: `assessment_confidence <= evidence_confidence × coverage_confidence`.
    /// A human-readable band ("high"/"moderate"/"low"/"unknown") over that
    /// product, so a caller doesn't have to re-derive the threshold logic.
    pub confidence_label: &'static str,
}

/// One of the two independent repository-health axes from
/// ELCI-DSEQ-EITR-001 §10 (Engineering Health / Governance Health) — a plain
/// average of the member tools' scores, kept separate so neither axis can
/// dilute or be diluted by the other.
#[derive(Debug, Serialize)]
pub struct HealthDimension {
    pub score: Option<f64>,
    pub grade: Option<&'static str>,
    pub graded_tools: usize,
    pub total_tools: usize,
    pub tools: Vec<String>,
}

impl Default for HealthDimension {
    fn default() -> Self {
        HealthDimension {
            score: None,
            grade: None,
            graded_tools: 0,
            total_tools: 0,
            tools: Vec::new(),
        }
    }
}

/// The third §10 axis: how much to trust the other two, independent of what
/// they say. Deliberately not a 0-100 score — coverage/confidence and the
/// set of tools contributing to "unknown surface" are more legible on their
/// own than blended into one more number.
#[derive(Debug, Serialize)]
pub struct EvidenceDimension {
    pub coverage: Option<f64>,
    pub confidence: Option<f64>,
    pub confidence_label: &'static str,
    /// Tools whose `EvidenceState` is Unknown, InsufficientCoverage, Blocked
    /// or Error — the part of the repository this run couldn't establish
    /// anything about, as opposed to established-and-healthy or
    /// established-and-a-finding.
    pub unknown_surface: Vec<String>,
}

impl Default for EvidenceDimension {
    fn default() -> Self {
        EvidenceDimension {
            coverage: None,
            confidence: None,
            confidence_label: "unknown",
            unknown_surface: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Default)]
pub struct RepositoryDimensions {
    pub engineering: HealthDimension,
    pub governance: HealthDimension,
    pub evidence: EvidenceDimension,
}

#[derive(Debug, Serialize)]
pub struct SuiteHealth {
    pub required_tools: usize,
    pub available_tools: usize,
    pub executed_tools: usize,
    pub valid_results: usize,
    pub analysis_coverage: Option<f64>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Healthy,
    Degraded,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct AnalysisIntegrity {
    pub status: IntegrityStatus,
    pub score: f64,
    pub grade: &'static str,
    pub defects: Vec<String>,
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// §6: `assessment_confidence <= evidence_confidence × coverage_confidence`.
/// Missing coverage or confidence is treated as non-limiting (1.0) here
/// rather than zeroing the product — a tool that doesn't report a coverage
/// fraction at all isn't asserting "zero coverage", it's asserting nothing,
/// and `"unknown"` (not a fabricated "low") is how that absence surfaces.
fn confidence_label(coverage: Option<f64>, confidence: Option<f64>) -> &'static str {
    if coverage.is_none() && confidence.is_none() {
        return "unknown";
    }
    let effective = confidence.unwrap_or(1.0) * coverage.unwrap_or(1.0);
    if effective >= 0.8 {
        "high"
    } else if effective >= 0.5 {
        "moderate"
    } else {
        "low"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthCategory {
    Engineering,
    Governance,
}

/// §10's Engineering/Governance split, by tool. This is a judgment call
/// documented here rather than derived from anything structural:
///
/// - Engineering (architecture, code quality, data flow, dev stability):
///   fract, chakra, ferret, edwardian, scrawny, catskin, tempcheq, vamos,
///   jeenome.
/// - Governance (licensing, security, compliance, observability, policy):
///   lwoodz, isopod, traci, viva-palestina, amber (dependency risk/policy,
///   not architecture — it's scored on replaceability and vendor exposure,
///   not structure).
/// - Deliberately in neither: ami (market intelligence, not a health axis),
///   bart (informational filesystem size — §7.19 says it must not affect
///   scoring), wilder (evidence orchestration *about* the other tools, not
///   itself an engineering or governance signal). Their scores/states still
///   appear in `tools` and feed the Evidence dimension's coverage/confidence
///   average; they just aren't averaged into either health axis.
fn health_category(tool_key: &str) -> Option<HealthCategory> {
    match tool_key {
        "fract" | "chakra" | "ferret" | "edwardian" | "scrawny" | "catskin" | "tempcheq"
        | "vamos" | "jeenome" => Some(HealthCategory::Engineering),
        "lwoodz" | "isopod" | "traci" | "viva-palestina" | "amber" => {
            Some(HealthCategory::Governance)
        }
        _ => None,
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub target: String,
    pub generated_at: String,
    pub tools_dir: String,
    pub tools: Vec<ToolReport>,
    pub overall: Overall,
    pub suite: SuiteHealth,
    pub integrity: AnalysisIntegrity,
    /// §10: Engineering Health / Governance Health / Evidence Confidence,
    /// computed independently of `overall` rather than as a breakdown of it.
    pub dimensions: RepositoryDimensions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CohortRepoStatus {
    /// A per-project report was produced (regardless of its own grade).
    Graded,
    /// Discovered on GitHub but has no local checkout under the cohort root.
    NotLocallyAvailable,
}

#[derive(Debug, Serialize)]
pub struct CohortRepoEntry {
    pub repo: String,
    pub path: Option<String>,
    pub report_file: Option<String>,
    pub status: CohortRepoStatus,
    pub overall_score: Option<f64>,
    pub overall_grade: Option<&'static str>,
    pub integrity_status: Option<IntegrityStatus>,
}

#[derive(Debug, Serialize)]
pub struct CohortRollup {
    pub graded_count: usize,
    pub mean_score: Option<f64>,
    pub worst: Vec<String>,
    pub integrity_failures: Vec<String>,
}

impl CohortRollup {
    /// Lowest 5 graded repos by overall score, and every repo whose
    /// analysis integrity isn't healthy — the two things worth a human's
    /// attention first out of a cohort-sized result set.
    pub fn compute(repos: &[CohortRepoEntry]) -> Self {
        let mut graded: Vec<(&str, f64)> = repos
            .iter()
            .filter_map(|r| r.overall_score.map(|s| (r.repo.as_str(), s)))
            .collect();
        graded.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mean_score = if graded.is_empty() {
            None
        } else {
            Some(graded.iter().map(|(_, s)| s).sum::<f64>() / graded.len() as f64)
        };

        let worst = graded
            .iter()
            .take(5)
            .map(|(repo, score)| format!("{repo}: {score:.1}"))
            .collect();

        let integrity_failures = repos
            .iter()
            .filter(|r| {
                matches!(
                    r.integrity_status,
                    Some(IntegrityStatus::Degraded) | Some(IntegrityStatus::Failed)
                )
            })
            .map(|r| r.repo.clone())
            .collect();

        CohortRollup {
            graded_count: graded.len(),
            mean_score,
            worst,
            integrity_failures,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CohortCycle {
    pub batch_size: usize,
    pub cycle_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct CohortReport {
    pub schema: &'static str,
    pub org: String,
    pub generated_at: String,
    pub discovered: usize,
    pub locally_available: usize,
    pub cycle: CohortCycle,
    pub repos: Vec<CohortRepoEntry>,
    pub rollup: CohortRollup,
}

impl Report {
    /// A graded tool reporting `Status::Fail` caps the overall score here,
    /// regardless of what the average works out to. Without this, a single
    /// critical-axis failure (e.g. a missing license) can be diluted away by
    /// unrelated healthy scores (e.g. module cohesion) into a passing
    /// average — this floor keeps one hard failure visible in the grade
    /// instead of averaged out. 73.0 is the bottom of the "C" band, so a
    /// capped grade still reads as a real problem rather than a rounding
    /// artifact.
    const HARD_FAIL_CAP: f64 = 73.0;

    pub fn compute_overall(tools: &[ToolReport]) -> Overall {
        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;
        let mut weights = Vec::new();
        let mut hard_fail = false;
        let mut coverages = Vec::new();
        let mut confidences = Vec::new();

        for t in tools {
            if let Some(score) = t.score {
                weights.push((t.tool.to_string(), 1.0));
                weighted_sum += score;
                weight_total += 1.0;
                if matches!(t.status, Status::Fail) {
                    hard_fail = true;
                }
                if let Some(c) = t.evidence.coverage {
                    coverages.push(c);
                }
                if let Some(c) = t.evidence.confidence {
                    confidences.push(c);
                }
            }
        }

        let mut score = if weight_total > 0.0 {
            Some(weighted_sum / weight_total)
        } else {
            None
        };
        if hard_fail {
            score = score.map(|s| s.min(Self::HARD_FAIL_CAP));
        }

        let coverage = mean(&coverages);
        let confidence = mean(&confidences);
        let confidence_label = confidence_label(coverage, confidence);

        Overall {
            score,
            grade: score.map(letter_for),
            graded_tools: weights.len(),
            total_tools: tools.len(),
            weights,
            provisional: tools.iter().any(|t| {
                matches!(
                    t.status,
                    Status::Error | Status::Unavailable | Status::NoData
                ) || t
                    .evidence
                    .coverage
                    .is_some_and(|coverage| coverage < MIN_SUFFICIENT_COVERAGE)
            }),
            coverage,
            confidence,
            confidence_label,
        }
    }

    /// §10's three-dimension split: Engineering Health and Governance Health
    /// are computed independently (so neither can dilute the other the way a
    /// single flat average does), and Evidence Confidence is the same
    /// coverage/confidence math as `Overall`, applied across every tool
    /// rather than just the graded ones.
    pub fn compute_dimensions(tools: &[ToolReport]) -> RepositoryDimensions {
        let build = |category: HealthCategory| {
            let members: Vec<&ToolReport> = tools
                .iter()
                .filter(|t| health_category(t.tool) == Some(category))
                .collect();
            let graded: Vec<f64> = members.iter().filter_map(|t| t.score).collect();
            let score = mean(&graded);
            HealthDimension {
                score,
                grade: score.map(letter_for),
                graded_tools: graded.len(),
                total_tools: members.len(),
                tools: members.iter().map(|t| t.tool.to_string()).collect(),
            }
        };

        let coverage = mean(&tools.iter().filter_map(|t| t.evidence.coverage).collect::<Vec<_>>());
        let confidence = mean(&tools.iter().filter_map(|t| t.evidence.confidence).collect::<Vec<_>>());
        let unknown_surface = tools
            .iter()
            .filter(|t| {
                matches!(
                    t.evidence_state(),
                    EvidenceState::Unknown
                        | EvidenceState::InsufficientCoverage
                        | EvidenceState::Blocked
                        | EvidenceState::Error
                )
            })
            .map(|t| t.tool.to_string())
            .collect();

        RepositoryDimensions {
            engineering: build(HealthCategory::Engineering),
            governance: build(HealthCategory::Governance),
            evidence: EvidenceDimension {
                coverage,
                confidence,
                confidence_label: confidence_label(coverage, confidence),
                unknown_surface,
            },
        }
    }

    pub fn compute_suite(tools: &[ToolReport]) -> SuiteHealth {
        let required: Vec<_> = tools
            .iter()
            .filter(|t| !matches!(t.availability, Availability::NotChecked))
            .collect();
        let average = |values: Vec<f64>| {
            if values.is_empty() {
                None
            } else {
                Some(values.iter().sum::<f64>() / values.len() as f64)
            }
        };
        SuiteHealth {
            required_tools: required.len(),
            available_tools: required
                .iter()
                .filter(|t| matches!(t.availability, Availability::Installed))
                .count(),
            executed_tools: required
                .iter()
                .filter(|t| matches!(t.execution, Execution::Succeeded))
                .count(),
            valid_results: required
                .iter()
                .filter(|t| {
                    matches!(t.execution, Execution::Succeeded)
                        && !matches!(t.status, Status::Error)
                })
                .count(),
            analysis_coverage: average(
                required
                    .iter()
                    .filter_map(|t| t.evidence.coverage)
                    .collect(),
            ),
            confidence: average(
                required
                    .iter()
                    .filter_map(|t| t.evidence.confidence)
                    .collect(),
            ),
        }
    }

    pub fn compute_integrity(tools: &[ToolReport], suite: &SuiteHealth) -> AnalysisIntegrity {
        let defects: Vec<String> = tools
            .iter()
            .filter(|tool| {
                !matches!(tool.availability, Availability::NotChecked)
                    && (matches!(
                        tool.availability,
                        Availability::Incompatible | Availability::Unavailable
                    ) || matches!(tool.execution, Execution::Failed)
                        || matches!(tool.status, Status::Error))
            })
            .map(|tool| format!("{}: {}", tool.tool, tool.summary))
            .collect();
        let score = if suite.required_tools == 0 {
            100.0
        } else {
            suite.valid_results as f64 / suite.required_tools as f64 * 100.0
        };
        let status = if defects.is_empty() && score >= 99.95 {
            IntegrityStatus::Healthy
        } else if score >= 80.0 {
            IntegrityStatus::Degraded
        } else {
            IntegrityStatus::Failed
        };
        AnalysisIntegrity {
            status,
            score,
            grade: letter_for(score),
            defects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_boundaries() {
        assert_eq!(letter_for(100.0), "A+");
        assert_eq!(letter_for(97.0), "A+");
        assert_eq!(letter_for(96.9), "A");
        assert_eq!(letter_for(90.0), "A-");
        assert_eq!(letter_for(89.9), "B+");
        assert_eq!(letter_for(60.0), "D-");
        assert_eq!(letter_for(59.9), "F");
        assert_eq!(letter_for(0.0), "F");
    }

    fn tool_report(tool: &'static str, score: Option<f64>) -> ToolReport {
        ToolReport {
            tool,
            purpose: "test",
            status: Status::Ok,
            availability: Availability::Installed,
            execution: Execution::Succeeded,
            evidence: Evidence {
                coverage: Some(1.0),
                confidence: Some(1.0),
                observations: Some(1),
            },
            binary: None,
            score,
            grade: score.map(letter_for),
            exit_code: Some(0),
            duration_ms: Some(1),
            summary: String::new(),
            findings: Vec::new(),
            note: None,
            raw: None,
        }
    }

    #[test]
    fn overall_ignores_ungraded_tools() {
        let tools = vec![
            tool_report("amber", Some(80.0)),
            tool_report("bart", None),
            tool_report("traci", Some(100.0)),
        ];
        let overall = Report::compute_overall(&tools);
        assert_eq!(overall.graded_tools, 2);
        assert_eq!(overall.total_tools, 3);
        assert!((overall.score.unwrap() - 90.0).abs() < 1e-9);
        assert_eq!(overall.grade, Some("A-"));
    }

    #[test]
    fn a_hard_fail_caps_overall_despite_a_healthy_average() {
        let mut failing = tool_report("lwoodz", Some(40.0));
        failing.status = Status::Fail;
        let tools = vec![
            failing,
            tool_report("fract", Some(100.0)),
            tool_report("chakra", Some(100.0)),
        ];
        let overall = Report::compute_overall(&tools);
        // Unweighted average would be (40+100+100)/3 = 80.0 (a B-); the hard
        // fail must cap it at 73.0 instead.
        assert!((overall.score.unwrap() - 73.0).abs() < 1e-9);
        assert_eq!(overall.grade, Some("C"));
    }

    #[test]
    fn hard_fail_cap_does_not_raise_an_already_lower_score() {
        let mut failing = tool_report("lwoodz", Some(40.0));
        failing.status = Status::Fail;
        let tools = vec![failing, tool_report("fract", Some(50.0))];
        let overall = Report::compute_overall(&tools);
        // Average is 45.0, already below the cap — the cap must not raise it.
        assert!((overall.score.unwrap() - 45.0).abs() < 1e-9);
    }

    #[test]
    fn overall_is_none_when_nothing_graded() {
        let tools = vec![tool_report("bart", None)];
        let overall = Report::compute_overall(&tools);
        assert_eq!(overall.score, None);
        assert_eq!(overall.grade, None);
    }

    #[test]
    fn low_evidence_coverage_marks_project_health_provisional() {
        let mut report = tool_report("isopod", None);
        report.evidence.coverage = Some(0.35);
        assert!(Report::compute_overall(&[report]).provisional);
    }

    #[test]
    fn integrity_distinguishes_findings_from_execution_defects() {
        let mut finding = tool_report("ferret", Some(73.0));
        finding.status = Status::Fail;
        let tools = vec![finding];
        let suite = Report::compute_suite(&tools);
        let integrity = Report::compute_integrity(&tools, &suite);
        assert_eq!(integrity.status, IntegrityStatus::Healthy);
        assert!(integrity.defects.is_empty());
    }

    #[test]
    fn incompatible_tool_degrades_integrity_without_scoring_the_project() {
        let mut tool = tool_report("ami", None);
        tool.status = Status::Unavailable;
        tool.availability = Availability::Incompatible;
        tool.execution = Execution::NotRun;
        let tools = vec![tool];
        let suite = Report::compute_suite(&tools);
        let integrity = Report::compute_integrity(&tools, &suite);
        assert_eq!(integrity.status, IntegrityStatus::Failed);
        assert_eq!(integrity.score, 0.0);
        assert_eq!(integrity.defects.len(), 1);
    }

    fn cohort_repo(
        repo: &str,
        score: Option<f64>,
        integrity: Option<IntegrityStatus>,
    ) -> CohortRepoEntry {
        CohortRepoEntry {
            repo: repo.to_string(),
            path: Some(format!("/home/sal/{repo}")),
            report_file: score.map(|_| format!("{repo}.json")),
            status: if score.is_some() {
                CohortRepoStatus::Graded
            } else {
                CohortRepoStatus::NotLocallyAvailable
            },
            overall_score: score,
            overall_grade: score.map(letter_for),
            integrity_status: integrity,
        }
    }

    #[test]
    fn rollup_ranks_worst_scores_and_lists_integrity_failures() {
        let repos = vec![
            cohort_repo("healthy", Some(95.0), Some(IntegrityStatus::Healthy)),
            cohort_repo("degraded", Some(60.0), Some(IntegrityStatus::Degraded)),
            cohort_repo("not-checked-out", None, None),
        ];
        let rollup = CohortRollup::compute(&repos);
        assert_eq!(rollup.graded_count, 2);
        assert!((rollup.mean_score.unwrap() - 77.5).abs() < 1e-9);
        assert_eq!(rollup.worst[0], "degraded: 60.0");
        assert_eq!(rollup.integrity_failures, vec!["degraded".to_string()]);
    }

    #[test]
    fn rollup_with_no_graded_repos_has_no_mean() {
        let repos = vec![cohort_repo("not-checked-out", None, None)];
        let rollup = CohortRollup::compute(&repos);
        assert_eq!(rollup.graded_count, 0);
        assert_eq!(rollup.mean_score, None);
        assert!(rollup.worst.is_empty());
    }

    // -- ELCI-DSEQ-EITR-001 §3: canonical evidence-state mapping ------------

    fn tool_with_status(status: Status, coverage: Option<f64>) -> ToolReport {
        let mut t = tool_report("x", Some(90.0));
        t.status = status;
        t.evidence.coverage = coverage;
        t
    }

    #[test]
    fn evidence_state_maps_every_status_to_its_canonical_analogue() {
        assert_eq!(
            tool_with_status(Status::Ok, Some(1.0)).evidence_state(),
            EvidenceState::Healthy
        );
        assert_eq!(
            tool_with_status(Status::Warn, Some(1.0)).evidence_state(),
            EvidenceState::Warning
        );
        assert_eq!(
            tool_with_status(Status::Fail, Some(1.0)).evidence_state(),
            EvidenceState::Finding
        );
        assert_eq!(
            tool_with_status(Status::NoData, Some(1.0)).evidence_state(),
            EvidenceState::Unknown
        );
        assert_eq!(
            tool_with_status(Status::NotApplicable, Some(1.0)).evidence_state(),
            EvidenceState::Inapplicable
        );
        assert_eq!(
            tool_with_status(Status::Skipped, Some(1.0)).evidence_state(),
            EvidenceState::Skipped
        );
        assert_eq!(
            tool_with_status(Status::Unavailable, Some(1.0)).evidence_state(),
            EvidenceState::Blocked
        );
        assert_eq!(
            tool_with_status(Status::Error, Some(1.0)).evidence_state(),
            EvidenceState::Error
        );
    }

    #[test]
    fn low_coverage_overrides_a_healthy_or_warning_status_to_insufficient_coverage() {
        assert_eq!(
            tool_with_status(Status::Ok, Some(0.35)).evidence_state(),
            EvidenceState::InsufficientCoverage
        );
        assert_eq!(
            tool_with_status(Status::Warn, Some(0.79)).evidence_state(),
            EvidenceState::InsufficientCoverage
        );
    }

    #[test]
    fn a_skipped_or_blocked_tool_ignores_its_coverage_number() {
        // A tool that never ran doesn't get reclassified by whatever stale
        // or zeroed coverage value happens to be sitting in `evidence`.
        assert_eq!(
            tool_with_status(Status::Skipped, Some(0.0)).evidence_state(),
            EvidenceState::Skipped
        );
        assert_eq!(
            tool_with_status(Status::Unavailable, Some(0.0)).evidence_state(),
            EvidenceState::Blocked
        );
    }

    #[test]
    fn missing_coverage_never_manufactures_insufficient_coverage() {
        // No coverage figure reported at all is not the same claim as "low
        // coverage" — it must not downgrade an otherwise-healthy verdict.
        assert_eq!(
            tool_with_status(Status::Ok, None).evidence_state(),
            EvidenceState::Healthy
        );
    }

    // -- §6: coverage-gated confidence ceiling on Overall --------------------

    #[test]
    fn overall_exposes_mean_coverage_and_confidence_across_graded_tools() {
        let mut a = tool_report("fract", Some(90.0));
        a.evidence.coverage = Some(0.6);
        a.evidence.confidence = Some(0.9);
        let mut b = tool_report("chakra", Some(80.0));
        b.evidence.coverage = Some(1.0);
        b.evidence.confidence = Some(0.7);
        let overall = Report::compute_overall(&[a, b]);
        assert!((overall.coverage.unwrap() - 0.8).abs() < 1e-9);
        assert!((overall.confidence.unwrap() - 0.8).abs() < 1e-9);
        // effective = 0.8 * 0.8 = 0.64 -> "moderate"
        assert_eq!(overall.confidence_label, "moderate");
    }

    #[test]
    fn overall_confidence_label_is_unknown_with_no_evidence_at_all() {
        let mut t = tool_report("bart", Some(100.0));
        t.evidence.coverage = None;
        t.evidence.confidence = None;
        let overall = Report::compute_overall(&[t]);
        assert_eq!(overall.confidence_label, "unknown");
    }

    // -- §10: three-dimension split -------------------------------------

    #[test]
    fn dimensions_average_engineering_and_governance_independently() {
        let tools = vec![
            tool_report("fract", Some(100.0)),   // engineering
            tool_report("chakra", Some(80.0)),   // engineering
            tool_report("lwoodz", Some(40.0)),   // governance
            tool_report("ami", Some(100.0)),     // neither
        ];
        let dims = Report::compute_dimensions(&tools);
        assert!((dims.engineering.score.unwrap() - 90.0).abs() < 1e-9);
        assert_eq!(dims.engineering.graded_tools, 2);
        assert!((dims.governance.score.unwrap() - 40.0).abs() < 1e-9);
        assert_eq!(dims.governance.graded_tools, 1);
    }

    #[test]
    fn dimensions_unknown_surface_collects_non_healthy_evidence_states() {
        let mut blocked = tool_report("ami", None);
        blocked.status = Status::Unavailable;
        let mut insufficient = tool_report("fract", Some(90.0));
        insufficient.evidence.coverage = Some(0.2);
        let healthy = tool_report("chakra", Some(90.0));
        let dims = Report::compute_dimensions(&[blocked, insufficient, healthy]);
        assert_eq!(dims.evidence.unknown_surface.len(), 2);
        assert!(dims.evidence.unknown_surface.contains(&"ami".to_string()));
        assert!(dims.evidence.unknown_surface.contains(&"fract".to_string()));
        assert!(!dims.evidence.unknown_surface.contains(&"chakra".to_string()));
    }
}

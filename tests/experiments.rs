// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Integration tests for `uni experiments`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni::experiments::compare::compare;
use uni::experiments::git::{
    classify_source, discover_candidates, resolve_default_branch, resolve_repo_root,
    resolve_revision,
};
use uni::experiments::report::{
    CandidateSource, CorrectnessValidation, DependabotMetadata, Dimension, ValidationCheck, Verdict,
};
use uni::report::{
    AnalysisIntegrity, Availability, Evidence, Execution, IntegrityStatus, Overall, Report, Status,
    SuiteHealth, ToolReport,
};

fn tmp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("git should be available");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(path: &std::path::Path) {
    std::fs::create_dir_all(path).unwrap();
    run_git(path, &["init", "--quiet", "--initial-branch=main"]);
    run_git(path, &["config", "user.email", "test@example.com"]);
    run_git(path, &["config", "user.name", "Test User"]);
}

fn commit_file(repo: &std::path::Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).unwrap();
    run_git(repo, &["add", name]);
    run_git(repo, &["commit", "--quiet", "-m", &format!("add {name}")]);
}

fn branch_from(repo: &std::path::Path, new_branch: &str, start_point: &str) {
    run_git(repo, &["branch", new_branch, start_point]);
}

#[tokio::test]
async fn discovers_dependabot_and_agent_branches() {
    let root = tmp_dir("uni-exp-discover");
    let _ = std::fs::remove_dir_all(&root);
    init_repo(&root);
    commit_file(&root, "README.md", "# baseline");

    branch_from(&root, "dependabot/cargo/ureq-3.3.0", "main");
    branch_from(&root, "agent/refactor-parser", "main");
    branch_from(&root, "feature/oauth", "main");

    let candidates = discover_candidates(&root).await.unwrap();
    let names: Vec<_> = candidates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"dependabot/cargo/ureq-3.3.0"));
    assert!(names.contains(&"agent/refactor-parser"));
    assert!(names.contains(&"feature/oauth"));

    let dependabot = candidates
        .iter()
        .find(|c| c.name == "dependabot/cargo/ureq-3.3.0")
        .unwrap();
    assert!(
        matches!(
            &dependabot.source,
            CandidateSource::Dependabot(DependabotMetadata {
                ecosystem,
                package: Some(pkg),
                target_version: Some(ver),
                ..
            }) if ecosystem == "cargo" && pkg == "ureq" && ver == "3.3.0"
        ),
        "unexpected source: {:?}",
        dependabot.source
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn resolves_default_branch_to_main() {
    let root = tmp_dir("uni-exp-baseline");
    let _ = std::fs::remove_dir_all(&root);
    init_repo(&root);
    commit_file(&root, "file.txt", "hello");

    let default = resolve_default_branch(&root).await.unwrap();
    assert_eq!(default, "main");

    let revision = resolve_revision(&root, "main").await.unwrap();
    assert_eq!(revision.branch, "main");
    assert_eq!(revision.sha.len(), 40);

    std::fs::create_dir_all(root.join("subdir")).unwrap();
    let resolved_root = resolve_repo_root(&root.join("subdir")).await.unwrap();
    assert_eq!(resolved_root, root);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn classify_source_parses_dependabot_metadata() {
    let source = classify_source("dependabot/cargo/ureq-3.3.0");
    assert!(
        matches!(
            &source,
            CandidateSource::Dependabot(DependabotMetadata {
                ecosystem,
                package: Some(pkg),
                target_version: Some(ver),
                ..
            }) if ecosystem == "cargo" && pkg == "ureq" && ver == "3.3.0"
        ),
        "unexpected: {:?}",
        source
    );
}

fn dummy_report(score: f64) -> Report {
    let tools = ["amber", "lwoodz", "fract"]
        .iter()
        .map(|tool| ToolReport {
            tool,
            purpose: "test",
            status: Status::Ok,
            availability: Availability::Installed,
            execution: Execution::Succeeded,
            evidence: Evidence {
                coverage: None,
                confidence: None,
                observations: None,
            },
            binary: None,
            score: Some(score),
            grade: Some(uni::report::letter_for(score)),
            exit_code: Some(0),
            duration_ms: Some(1),
            summary: String::new(),
            findings: Vec::new(),
            note: None,
            raw: None,
        })
        .collect();
    Report {
        schema: "uni.report/v3",
        target: ".".into(),
        generated_at: String::new(),
        tools_dir: ".".into(),
        tools,
        overall: Overall {
            score: Some(score),
            grade: Some(uni::report::letter_for(score)),
            graded_tools: 1,
            total_tools: 1,
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
        integrity: AnalysisIntegrity {
            status: IntegrityStatus::Healthy,
            score: 100.0,
            grade: "A+",
            defects: Vec::new(),
        },
    }
}

fn correctness(success: bool) -> CorrectnessValidation {
    CorrectnessValidation {
        cargo_check: ValidationCheck {
            command: "cargo check".into(),
            success,
            duration_ms: None,
            exit_code: Some(if success { 0 } else { 101 }),
            summary: String::new(),
            detail: None,
        },
        cargo_test: ValidationCheck {
            command: "cargo test".into(),
            success,
            duration_ms: None,
            exit_code: Some(if success { 0 } else { 101 }),
            summary: String::new(),
            detail: None,
        },
    }
}

#[test]
fn comparison_detects_superior_candidate() {
    let baseline = dummy_report(70.0);
    let candidate = dummy_report(85.0);
    let result = compare(
        &baseline,
        &candidate,
        &correctness(true),
        &correctness(true),
    );
    assert!(
        matches!(result.verdict, Verdict::Superior | Verdict::LikelySuperior),
        "got {:?}",
        result.verdict
    );
    assert!(result.confidence > 0.0);
    let overall = result
        .dimensions
        .iter()
        .find(|d| d.dimension == Dimension::Overall)
        .unwrap();
    assert_eq!(overall.delta, Some(15.0));
}

#[test]
fn comparison_blocks_on_correctness_failure() {
    let baseline = dummy_report(70.0);
    let candidate = dummy_report(85.0);
    let result = compare(
        &baseline,
        &candidate,
        &correctness(true),
        &correctness(false),
    );
    assert_eq!(result.verdict, Verdict::Blocked);
}

#[test]
fn comparison_marks_equivalent_when_unchanged() {
    let baseline = dummy_report(80.0);
    let candidate = dummy_report(80.0);
    let result = compare(
        &baseline,
        &candidate,
        &correctness(true),
        &correctness(true),
    );
    assert_eq!(result.verdict, Verdict::Equivalent);
}

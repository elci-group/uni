// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Parse Vamos's stable workflow-suite summary into Uni's normalized report.

use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VamosReport {
    passed: bool,
    tasks: Vec<VamosTask>,
}

#[derive(Debug, Deserialize)]
struct VamosTask {
    task_name: String,
    evaluations: Vec<VamosEvaluation>,
}

#[derive(Debug, Deserialize)]
struct VamosEvaluation {
    evaluator: String,
    passed: bool,
    message: String,
}

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let raw: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(value) => value,
        Err(error) => return ParseOutcome::json_error(error, stdout),
    };
    let report: VamosReport = match serde_json::from_value(raw.clone()) {
        Ok(report) => report,
        Err(error) => return ParseOutcome::json_error(error, stdout),
    };

    let evaluation_count = report
        .tasks
        .iter()
        .map(|task| task.evaluations.len())
        .sum::<usize>();
    if evaluation_count == 0 {
        return ParseOutcome {
            status: Status::Warn,
            score: None,
            summary: format!(
                "{} workflow(s) ran but produced no evaluations",
                report.tasks.len()
            ),
            findings: Vec::new(),
            note: Some("Vamos cannot grade a suite without evaluator results".to_string()),
            raw: Some(raw),
        };
    }

    let passed_count = report
        .tasks
        .iter()
        .flat_map(|task| &task.evaluations)
        .filter(|evaluation| evaluation.passed)
        .count();
    let failed_count = evaluation_count - passed_count;
    let score = clamp_score(100.0 * passed_count as f64 / evaluation_count as f64);
    let status = if report.passed && failed_count == 0 {
        Status::Ok
    } else if score >= 70.0 {
        Status::Warn
    } else {
        Status::Fail
    };

    let mut findings = report
        .tasks
        .iter()
        .flat_map(|task| {
            task.evaluations
                .iter()
                .filter(|evaluation| !evaluation.passed)
                .map(|evaluation| {
                    format!(
                        "{} / {}: {}",
                        task.task_name, evaluation.evaluator, evaluation.message
                    )
                })
        })
        .collect::<Vec<_>>();
    findings.truncate(10);

    ParseOutcome {
        status,
        score: Some(score),
        summary: format!(
            "{} workflow(s), {passed_count}/{evaluation_count} evaluations passed",
            report.tasks.len()
        ),
        findings,
        note: None,
        raw: Some(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_workflows_score_one_hundred() {
        let out = parse(
            r#"{
                "passed": true,
                "tasks": [{
                    "task_name": "opens-menu",
                    "evaluations": [{
                        "evaluator": "deterministic",
                        "passed": true,
                        "message": "all assertions passed"
                    }]
                }]
            }"#,
            Some(0),
        );

        assert_eq!(out.status, Status::Ok);
        assert_eq!(out.score, Some(100.0));
        assert!(out.findings.is_empty());
        assert!(out.raw.is_some());
    }

    #[test]
    fn failed_evaluations_become_findings() {
        let out = parse(
            r#"{
                "passed": false,
                "tasks": [{
                    "task_name": "navigate-menu",
                    "evaluations": [
                        {"evaluator": "exit", "passed": true, "message": "exit code matched"},
                        {"evaluator": "screen", "passed": false, "message": "missing Goodbye"}
                    ]
                }]
            }"#,
            Some(1),
        );

        assert_eq!(out.status, Status::Fail);
        assert_eq!(out.score, Some(50.0));
        assert_eq!(
            out.findings,
            vec!["navigate-menu / screen: missing Goodbye"]
        );
    }

    #[test]
    fn empty_evaluations_are_ungraded() {
        let out = parse(
            r#"{"passed":true,"tasks":[{"task_name":"empty","evaluations":[]}]}"#,
            Some(0),
        );
        assert_eq!(out.status, Status::Warn);
        assert_eq!(out.score, None);
    }

    #[test]
    fn malformed_json_is_an_error() {
        let out = parse("not json", Some(1));
        assert_eq!(out.status, Status::Error);
        assert_eq!(out.score, None);
    }
}

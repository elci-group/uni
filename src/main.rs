// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
mod curly_expand;

use std::path::PathBuf;

use clap::Parser;

use uni::cli::{self, AnalyzeOptions, Command};
use uni::experiments;
use uni::{animation, cohort, render, revise, run};

/// Wires up the `tracing` subscriber that the whole crate already emits
/// spans and events into but that, until this call exists, has no
/// subscriber at all — every `tracing::error!`/`instrument` in the tree is
/// silently discarded. `RUST_LOG` (standard `EnvFilter` syntax, e.g.
/// `uni=debug`) takes precedence; otherwise `-v`/`-vv`/`-vvv` steps the
/// default level for uni's own spans from warn up to trace. Dependency
/// crates stay at `warn` by default so `-v` surfaces uni's own diagnostics
/// without flooding stderr with unrelated crate chatter.
fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;

    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("warn,uni={default_level}")));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

#[tokio::main]
async fn __curly_original_main() {
    let cli = cli::Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Some(Command::Analyze(args)) => {
            if args.cohort {
                run_cohort(args.cohort_options(), args.analyze_options(), args.json).await
            } else {
                run_analyze(
                    args.analyze_options(),
                    args.json,
                    args.technical,
                    args.out,
                    args.fail_under,
                )
                .await
            }
        }
        Some(Command::Experiments(args)) => run_experiments(args).await,
        Some(Command::Revise(args)) => run_revise(args).await,
        None => {
            if cli.cohort {
                run_cohort(cli.cohort_options(), cli.analyze_options(), cli.json).await
            } else {
                let json = cli.json;
                let technical = cli.technical;
                let out = cli.out.clone();
                let fail_under = cli.fail_under;
                run_analyze(cli.analyze_options(), json, technical, out, fail_under).await
            }
        }
    }
}

async fn run_cohort(cohort_opts: cli::CohortOptions, base: AnalyzeOptions, json: bool) {
    let report = match cohort::run(&cohort_opts, &base).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("uni: {e}");
            std::process::exit(2);
        }
    };

    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("uni: internal error: cohort report failed to serialize: {e}");
                std::process::exit(2);
            }
        }
    } else {
        print!("{}", render::human_cohort_report(&report));
    }
}

async fn run_analyze(
    opts: AnalyzeOptions,
    json: bool,
    technical: bool,
    out: Option<PathBuf>,
    fail_under: Option<f64>,
) {
    let report = match run::execute(&opts).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("uni: {e}");
            std::process::exit(2);
        }
    };

    let json_text = match serde_json::to_string_pretty(&report) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("uni: internal error: report failed to serialize: {e}");
            std::process::exit(2);
        }
    };

    if let Some(path) = &out {
        if let Err(e) = std::fs::write(path, &json_text) {
            eprintln!("uni: failed to write {}: {e}", path.display());
            std::process::exit(2);
        }
    }

    if json {
        println!("{json_text}");
    } else {
        let rendered = if technical {
            render::technical_report(&report)
        } else {
            render::plain_report(&report)
        };
        if let Err(e) = animation::present(&rendered).await {
            eprintln!("uni: failed to write human report: {e}");
            std::process::exit(2);
        }
    }

    if let Some(threshold) = fail_under {
        match report.overall.score {
            Some(score) if score < threshold => std::process::exit(1),
            None => {
                eprintln!("uni: --fail-under given but no tool produced a numeric score");
                std::process::exit(1);
            }
            _ => {}
        }
    }
}

async fn run_revise(args: cli::ReviseArgs) {
    let json = args.json;

    let report = match revise::execute(&args).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("uni: {e}");
            std::process::exit(2);
        }
    };

    if json {
        let json_text = match serde_json::to_string_pretty(&report) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("uni: internal error: revise report failed to serialize: {e}");
                std::process::exit(2);
            }
        };
        println!("{json_text}");
    } else {
        print!("{}", revise::human(&report));
    }

    let any_failed = report
        .remediations
        .iter()
        .any(|r| r.outcome == revise::Outcome::Failed);
    let fail_on_gate = match args.fail_on {
        Some(cli::RunFailLevel::Partial) => matches!(
            report.outcome,
            revise::RunOutcome::Partial | revise::RunOutcome::Regressed
        ),
        Some(cli::RunFailLevel::Regressed) => {
            matches!(report.outcome, revise::RunOutcome::Regressed)
        }
        None => false,
    };
    if args.apply && (any_failed || fail_on_gate) {
        std::process::exit(1);
    }
}

async fn run_experiments(args: cli::ExperimentsArgs) {
    if let Err(e) = args.validate() {
        eprintln!("uni: experiments: {e}");
        std::process::exit(2);
    }

    if args.list {
        match experiments::list(&args).await {
            Ok(candidates) => {
                if candidates.is_empty() {
                    println!("No candidate branches discovered.");
                } else {
                    println!("{:<50} {:<15}", "BRANCH", "SOURCE");
                    for c in candidates {
                        println!("{:<50} {:<15}", c.name, c.source.display_label());
                    }
                }
            }
            Err(e) => {
                eprintln!("uni: experiments: {e}");
                std::process::exit(2);
            }
        }
        return;
    }

    let json = args.json;
    let out = args.out.clone();

    let report = match experiments::run(&args).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("uni: experiments: {e}");
            std::process::exit(2);
        }
    };

    let json_text = match serde_json::to_string_pretty(&report) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("uni: internal error: experiment report failed to serialize: {e}");
            std::process::exit(2);
        }
    };

    if let Some(path) = &out {
        if let Err(e) = std::fs::write(path, &json_text) {
            eprintln!("uni: failed to write {}: {e}", path.display());
            std::process::exit(2);
        }
    }

    if json {
        println!("{json_text}");
    } else {
        print!("{}", experiments::render(&report));
    }

    // Policy gates.
    if report.any_blocked() {
        std::process::exit(1);
    }

    if let Some(min_confidence) = args.minimum_confidence {
        let meets = report
            .best_eligible()
            .is_some_and(|e| e.comparison.confidence >= min_confidence);
        if !meets {
            std::process::exit(1);
        }
    }

    if let Some(min_improvement) = args.minimum_improvement {
        let meets = report.best_eligible().is_some_and(|e| {
            e.comparison
                .overall_delta
                .delta
                .is_some_and(|d| d >= min_improvement)
        });
        if !meets {
            std::process::exit(1);
        }
    }
}

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    let mut positions: Vec<usize> = Vec::new();
    let mut fields: Vec<Vec<String>> = Vec::new();
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--out" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--out=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--out={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--only" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--only=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--only={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--skip" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--skip=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--skip={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--jeenome-trace" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--jeenome-trace=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--jeenome-trace={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--tools-dir" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--tools-dir=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--tools-dir={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--cohort-org" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--cohort-org=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--cohort-org={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--cohort-out" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--cohort-out=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--cohort-out={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--out" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--out=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--out={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--only" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--only=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--only={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--skip" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--skip=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--skip={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--jeenome-trace" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--jeenome-trace=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--jeenome-trace={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--tools-dir" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--tools-dir=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--tools-dir={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--cohort-org" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--cohort-org=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--cohort-org={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--cohort-out" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--cohort-out=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--cohort-out={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--only" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--only=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--only={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--skip" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--skip=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--skip={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--tools-dir" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--tools-dir=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--tools-dir={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--baseline" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--baseline=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--baseline={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--branch" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--branch=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--branch={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--out" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--out=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--out={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--only" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--only=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--only={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--skip" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--skip=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--skip={}", v))
                    .collect(),
            );
            break;
        }
    }
    for (__i, __a) in raw_args.iter().enumerate() {
        if __a == "--tools-dir" {
            if let Some(__v) = raw_args.get(__i + 1) {
                positions.push(__i + 1);
                fields.push(curly_expand::expand_or_literal(__v));
            }
            break;
        } else if let Some(__v) = __a.strip_prefix("--tools-dir=") {
            positions.push(__i);
            fields.push(
                curly_expand::expand_or_literal(__v)
                    .into_iter()
                    .map(|v| format!("--tools-dir={}", v))
                    .collect(),
            );
            break;
        }
    }
    if let Some(__v) = raw_args.get(1) {
        if !__v.starts_with('-') {
            positions.push(1);
            fields.push(curly_expand::expand_or_literal(__v));
        }
    }
    if let Some(__v) = raw_args.get(2) {
        if !__v.starts_with('-') {
            positions.push(2);
            fields.push(curly_expand::expand_or_literal(__v));
        }
    }
    if let Some(__v) = raw_args.get(3) {
        if !__v.starts_with('-') {
            positions.push(3);
            fields.push(curly_expand::expand_or_literal(__v));
        }
    }
    if let Some(__v) = raw_args.get(4) {
        if !__v.starts_with('-') {
            positions.push(4);
            fields.push(curly_expand::expand_or_literal(__v));
        }
    }

    if fields.is_empty() || fields.iter().all(|f| f.len() <= 1) {
        __curly_original_main();
        return;
    }

    let combos = curly_expand::cartesian(&fields);
    let exe = std::env::current_exe().expect("resolve current exe");
    let mut had_failure = false;
    for combo in &combos {
        let mut new_args = raw_args.clone();
        for (slot, value) in positions.iter().zip(combo.iter()) {
            new_args[*slot] = value.clone();
        }
        let status = std::process::Command::new(&exe)
            .args(&new_args[1..])
            .status()
            .expect("failed to re-exec self");
        if !status.success() {
            had_failure = true;
        }
    }
    if had_failure {
        std::process::exit(1);
    }
}

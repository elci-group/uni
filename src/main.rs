// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
mod animation;
mod cli;
mod kaptaind;
mod parsers;
mod render;
mod report;
mod revise;
mod run;
mod tool;

use std::path::PathBuf;

use clap::Parser;

use cli::{AnalyzeOptions, Command};

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    match cli.command {
        Some(Command::Analyze(args)) => {
            run_analyze(args.analyze_options(), args.json, args.out, args.fail_under).await
        }
        Some(Command::Revise(args)) => run_revise(args).await,
        None => {
            let json = cli.json;
            let out = cli.out.clone();
            let fail_under = cli.fail_under;
            run_analyze(cli.analyze_options(), json, out, fail_under).await
        }
    }
}

async fn run_analyze(
    opts: AnalyzeOptions,
    json: bool,
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
        let rendered = render::human_report(&report);
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

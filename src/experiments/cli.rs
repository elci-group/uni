// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! CLI arguments for `uni experiments`.

use std::path::PathBuf;

use clap::Parser;

use crate::cli::AnalyzeOptions;

#[derive(Parser, Debug)]
pub struct ExperimentsArgs {
    /// Project to analyze. Must be a git repository.
    #[arg(default_value = ".")]
    pub target: PathBuf,

    /// Baseline branch or ref. Defaults to the repository's default branch
    /// (usually `main`).
    #[arg(long)]
    pub baseline: Option<String>,

    /// Analyze only this candidate branch. May be given multiple times.
    #[arg(long)]
    pub branch: Vec<String>,

    /// Discover and analyze all candidate branches instead of requiring
    /// `--branch`.
    #[arg(long)]
    pub all_candidates: bool,

    /// List discovered candidate branches and exit without analyzing.
    #[arg(long)]
    pub list: bool,

    /// Print the experiment report as JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,

    /// Also write the JSON experiment report to this file.
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// Run only these UNI tools during baseline/candidate analysis
    /// (comma-separated, e.g. amber,traci).
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Skip these UNI tools during baseline/candidate analysis
    /// (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    /// Per-tool timeout in seconds.
    #[arg(long, default_value_t = 300)]
    pub timeout: u64,

    /// Root directory containing sibling tool checkouts.
    #[arg(long)]
    pub tools_dir: Option<PathBuf>,

    /// Clone and install selected missing tools after validating Baby recipes.
    #[arg(long)]
    pub install_missing: bool,

    /// Exit with status 1 if no adoption-eligible candidate reaches at least
    /// this confidence level (0-1).
    #[arg(long)]
    pub minimum_confidence: Option<f64>,

    /// Exit with status 1 if no adoption-eligible candidate improves the
    /// overall score by at least this amount.
    #[arg(long)]
    pub minimum_improvement: Option<f64>,
}

impl ExperimentsArgs {
    /// Options shared with the ordinary `uni analyze` pass.
    pub fn analyze_options(&self) -> AnalyzeOptions {
        AnalyzeOptions {
            target: self.target.clone(),
            only: self.only.clone(),
            skip: self.skip.clone(),
            jeenome: false,
            jeenome_trace: None,
            timeout: self.timeout,
            tools_dir: self.tools_dir.clone(),
            install_missing: self.install_missing,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(c) = self.minimum_confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(format!(
                    "--minimum-confidence must be between 0.0 and 1.0, got {c}"
                ));
            }
        }
        Ok(())
    }
}

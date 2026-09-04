// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

pub use crate::experiments::cli::ExperimentsArgs;

#[derive(Parser, Debug)]
#[command(
    name = "uni",
    version,
    about = "Unified analysis snapshot with separate project-health and analysis-integrity verdicts.",
    long_about = "uni runs amber, bart, chakra, ferret hunt (`ferret hunt`, with `ferret track` compatibility), fract, isopod, lwoodz, scrawny, tempcheq, traci, vamos, viva-palestina, and wilder concurrently, then reports project findings separately from analyzer defects. AMI is opt-in via --only ami because profile completeness is not code health and requires a JSON-capable build. Jeenome is opt-in (--jeenome) because it audits an strace trace. Missing public applications are classified without mutation by default; pass --install-missing to opt into validated, serialized installation through Baby."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Project to analyze. Ignored when a subcommand is given.
    #[arg(default_value = ".")]
    pub target: PathBuf,

    /// Print the report as JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,

    /// Show the detailed technical report (tool names, raw findings,
    /// severity tables) instead of the concise plain-language summary that
    /// non-technical readers get by default. Pairs well with --json for
    /// programmatic/model consumption; ignored when --json is also given.
    #[arg(long)]
    pub technical: bool,

    /// Increase diagnostic logging (-v for info, -vv for debug, -vvv for
    /// trace). Logs go to stderr and never mix into --json/--out output.
    /// RUST_LOG, if set, overrides this entirely.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Also write the JSON report to this file (independent of --json).
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// Run only these tools (comma-separated, e.g. amber,traci).
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Skip these tools (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    /// Include jeenome. Requires GROQ_API_KEY and either --jeenome-trace or
    /// a Cargo project + `strace` on PATH so uni can generate one.
    #[arg(long)]
    pub jeenome: bool,

    /// Use this existing strace log for jeenome instead of generating one.
    #[arg(long)]
    pub jeenome_trace: Option<PathBuf>,

    /// Per-tool timeout in seconds.
    #[arg(long, default_value_t = 300)]
    pub timeout: u64,

    /// Root directory containing sibling tool checkouts (default: the
    /// parent directory of uni's own source tree).
    #[arg(long)]
    pub tools_dir: Option<PathBuf>,

    /// Clone and install selected missing tools after validating Baby recipes.
    #[arg(long)]
    pub install_missing: bool,

    /// Exit with status 1 if the overall score falls below this threshold
    /// (0-100). Omit to always exit 0 for a successful run (uni is a
    /// snapshot tool by default, not a gate).
    #[arg(long)]
    pub fail_under: Option<f64>,

    /// Assess every first-party elci-group repository instead of one
    /// project. `target` becomes the discovery root override (rarely
    /// needed) rather than a single project path.
    #[arg(long)]
    pub cohort: bool,

    /// GitHub account to discover first-party repos from.
    #[arg(long, default_value = "elci-group")]
    pub cohort_org: String,

    /// How many repos to analyze concurrently per cycle.
    #[arg(long, default_value_t = 4)]
    pub cohort_batch_size: usize,

    /// Seconds to pause between batches.
    #[arg(long, default_value_t = 30)]
    pub cohort_cycle_seconds: u64,

    /// Directory to write per-repo reports and the rollup summary into
    /// (default: `./uni-cohort-<timestamp>/`).
    #[arg(long)]
    pub cohort_out: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the concurrent analysis snapshot (same as passing no subcommand).
    Analyze(AnalyzeArgs),

    /// Discover, evaluate, and compare candidate branches against a baseline.
    /// Read-only: no branches are merged or modified.
    Experiments(ExperimentsArgs),

    /// Diagnose issues, then run each flagged tool's own fix/remediation
    /// command (amber --propose, isopod harden, lwoodz remedy,
    /// tempcheq --fix). Dry-run by default; pass --apply to actually
    /// mutate the project.
    Revise(ReviseArgs),
}

#[derive(Parser, Debug)]
pub struct AnalyzeArgs {
    #[arg(default_value = ".")]
    pub target: PathBuf,

    #[arg(long)]
    pub json: bool,

    /// Show the detailed technical report instead of the concise
    /// plain-language summary that's the default.
    #[arg(long)]
    pub technical: bool,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    #[arg(long)]
    pub jeenome: bool,

    #[arg(long)]
    pub jeenome_trace: Option<PathBuf>,

    #[arg(long, default_value_t = 300)]
    pub timeout: u64,

    #[arg(long)]
    pub tools_dir: Option<PathBuf>,

    /// Clone and install selected missing tools after validating Baby recipes.
    #[arg(long)]
    pub install_missing: bool,

    #[arg(long)]
    pub fail_under: Option<f64>,

    #[arg(long)]
    pub cohort: bool,

    #[arg(long, default_value = "elci-group")]
    pub cohort_org: String,

    #[arg(long, default_value_t = 4)]
    pub cohort_batch_size: usize,

    #[arg(long, default_value_t = 30)]
    pub cohort_cycle_seconds: u64,

    #[arg(long)]
    pub cohort_out: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct ReviseArgs {
    /// Project to revise.
    #[arg(default_value = ".")]
    pub target: PathBuf,

    /// Actually run each tool's fix command. Without this, uni only shows
    /// what it would run (every underlying fix command is itself dry-run
    /// or plan-only by default: amber --propose never touches your source,
    /// isopod harden needs its own --apply, and tempcheq --fix needs --yes —
    /// uni passes those through only here).
    #[arg(long)]
    pub apply: bool,

    /// Only consider these tools for remediation (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Skip these tools during remediation (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    /// Per-tool timeout in seconds, for both the diagnostic pass and each
    /// remediation command.
    #[arg(long, default_value_t = 300)]
    pub timeout: u64,

    /// Root directory containing sibling tool checkouts.
    #[arg(long)]
    pub tools_dir: Option<PathBuf>,

    /// Print the revise plan/result as JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,

    /// Required in addition to --apply before uni will run a remediation
    /// that rewrites the contents of files already tracked in the target
    /// (currently: tempcheq --fix, traci trace --apply). Remediations that
    /// only ever create new files (amber --propose, isopod harden, lwoodz
    /// remedy) don't need this — --apply alone is enough for those.
    #[arg(long)]
    pub confirm_source_rewrite: bool,

    /// Required in addition to --apply and --confirm-source-rewrite before
    /// uni will run a remediation that delegates to a model-generated
    /// patch (currently: traci trace --apply). Delegates like this verify
    /// their own patch against a benchmark and a complexity/diagnostic
    /// regression budget before merging it — but it's still code a model
    /// wrote, a separate trust concern from a deterministic rewrite like
    /// tempcheq's, so it needs its own explicit opt-in.
    #[arg(long)]
    pub confirm_ai_patch: bool,

    /// Exit with status 1 if the run's outcome reaches this severity or
    /// worse (partial < regressed). `uni analyze --fail-under`'s
    /// counterpart for `uni revise`: omit to always exit 0 for a
    /// successful *run* of revise itself, regardless of how the
    /// remediations it ran turned out (revise is a report by default,
    /// same as analyze) — the run's classification is always in the
    /// report's `outcome` field either way, for a CI script to read
    /// itself even without this flag.
    #[arg(long, value_enum)]
    pub fail_on: Option<RunFailLevel>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum RunFailLevel {
    /// Fail on `partial` or `regressed`.
    Partial,
    /// Fail only on `regressed`.
    Regressed,
}

/// The subset of options `run::execute` needs, shared by the bare
/// `uni <target>` invocation and `uni analyze`. Clone so cohort mode can
/// reuse one base template per repo, overriding just `target`.
#[derive(Clone)]
pub struct AnalyzeOptions {
    pub target: PathBuf,
    pub only: Vec<String>,
    pub skip: Vec<String>,
    pub jeenome: bool,
    pub jeenome_trace: Option<PathBuf>,
    pub timeout: u64,
    pub tools_dir: Option<PathBuf>,
    pub install_missing: bool,
}

/// Cohort-mode discovery/pacing options, shared by the bare `uni --cohort`
/// invocation and `uni analyze --cohort`.
pub struct CohortOptions {
    pub root: Option<PathBuf>,
    pub org: String,
    pub batch_size: usize,
    pub cycle_seconds: u64,
    pub out: Option<PathBuf>,
}

impl Cli {
    pub fn analyze_options(&self) -> AnalyzeOptions {
        AnalyzeOptions {
            target: self.target.clone(),
            only: self.only.clone(),
            skip: self.skip.clone(),
            jeenome: self.jeenome,
            jeenome_trace: self.jeenome_trace.clone(),
            timeout: self.timeout,
            tools_dir: self.tools_dir.clone(),
            install_missing: self.install_missing,
        }
    }

    pub fn cohort_options(&self) -> CohortOptions {
        CohortOptions {
            root: (self.target.as_path() != Path::new(".")).then(|| self.target.clone()),
            org: self.cohort_org.clone(),
            batch_size: self.cohort_batch_size,
            cycle_seconds: self.cohort_cycle_seconds,
            out: self.cohort_out.clone(),
        }
    }
}

impl AnalyzeArgs {
    pub fn analyze_options(&self) -> AnalyzeOptions {
        AnalyzeOptions {
            target: self.target.clone(),
            only: self.only.clone(),
            skip: self.skip.clone(),
            jeenome: self.jeenome,
            jeenome_trace: self.jeenome_trace.clone(),
            timeout: self.timeout,
            tools_dir: self.tools_dir.clone(),
            install_missing: self.install_missing,
        }
    }

    pub fn cohort_options(&self) -> CohortOptions {
        CohortOptions {
            root: (self.target.as_path() != Path::new(".")).then(|| self.target.clone()),
            org: self.cohort_org.clone(),
            batch_size: self.cohort_batch_size,
            cycle_seconds: self.cohort_cycle_seconds,
            out: self.cohort_out.clone(),
        }
    }
}

impl ReviseArgs {
    /// The diagnostic pass revise runs first, to see which tools are
    /// currently flagged. jeenome is never included: it has no
    /// remediation action, and forcing it on would mean generating an
    /// strace trace just to throw the result away.
    pub fn analyze_options(&self) -> AnalyzeOptions {
        AnalyzeOptions {
            target: self.target.clone(),
            only: self.only.clone(),
            skip: self.skip.clone(),
            jeenome: false,
            jeenome_trace: None,
            timeout: self.timeout,
            tools_dir: self.tools_dir.clone(),
            install_missing: false,
        }
    }
}

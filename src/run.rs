// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ingauge_gate::Admitter;
use tracing::Instrument;

use crate::cli::AnalyzeOptions;
use crate::parsers;
use crate::report::{
    letter_for, Availability, Evidence, Execution, Overall, Report, Status, ToolReport,
};
use crate::tool::{self, ToolId};

/// Pairs a spawned tool run's own future with the [`ToolId`] it belongs to,
/// so the `JoinHandle` result can be routed back to the right report slot.
async fn tag<F: std::future::Future<Output = ToolReport>>(
    tool: ToolId,
    fut: F,
) -> (ToolId, ToolReport) {
    (tool, fut.await)
}

pub async fn execute(opts: &AnalyzeOptions) -> Result<Report, String> {
    let target = std::fs::canonicalize(&opts.target)
        .map_err(|e| format!("target path {:?} is not accessible: {e}", opts.target))?;

    let tools_dir = opts
        .tools_dir
        .clone()
        .unwrap_or_else(tool::default_tools_dir);

    let only: Vec<String> = opts.only.iter().map(|s| s.trim().to_lowercase()).collect();
    let skip: Vec<String> = opts.skip.iter().map(|s| s.trim().to_lowercase()).collect();

    for k in only.iter().chain(skip.iter()) {
        if ToolId::from_key(k).is_none() {
            eprintln!("uni: warning: unknown tool name {k:?} in --only/--skip, ignoring");
        }
    }

    let selected = |t: ToolId| -> bool {
        if !only.is_empty() {
            return only.iter().any(|k| k == t.key());
        }
        if skip.iter().any(|k| k == t.key()) {
            return false;
        }
        t.enabled_by_default() || (t == ToolId::Jeenome && opts.jeenome)
    };

    let timeout = Duration::from_secs(opts.timeout);
    let admitter = Arc::new(Admitter::from_env());

    let mut handles: Vec<tokio::task::JoinHandle<(ToolId, ToolReport)>> = Vec::new();
    let mut immediate: BTreeMap<&'static str, ToolReport> = BTreeMap::new();

    for tool in ToolId::ALL {
        if !selected(tool) {
            let note = if tool == ToolId::Jeenome && !opts.jeenome {
                "jeenome is opt-in (needs an strace trace, not just a project path); pass --jeenome to include it".to_string()
            } else {
                "excluded via --only/--skip".to_string()
            };
            immediate.insert(tool.key(), skipped_report(tool, note));
            continue;
        }

        if tool == ToolId::Jeenome {
            let target = target.clone();
            let trace_override = opts.jeenome_trace.clone();
            let admitter = Arc::clone(&admitter);
            let tools_dir = tools_dir.clone();
            #[rustfmt::skip]
            handles.push(tokio::spawn(
                tag(tool, run_jeenome(target, trace_override, timeout, admitter, tools_dir)).instrument(tracing::info_span!("tool_run", tool = tool.key())),
            ));
            continue;
        }

        if tool == ToolId::Vamos {
            let target = target.clone();
            let tools_dir = tools_dir.clone();
            #[rustfmt::skip]
            handles.push(tokio::spawn(
                tag(tool, run_vamos(target, timeout, tools_dir)).instrument(tracing::info_span!("tool_run", tool = tool.key())),
            ));
            continue;
        }

        let bin = if let Some(bin) = tool::resolve_binary(tool, &tools_dir) {
            bin
        } else if !opts.install_missing {
            immediate.insert(
                tool.key(),
                installable_report(
                    tool,
                    format!(
                        "known repository {}; installation was not attempted (pass --install-missing)",
                        tool.repo_url()
                    ),
                ),
            );
            continue;
        } else {
            match ensure_binary(tool, &tools_dir).await {
                Ok(b) => b,
                Err(reason) => {
                    tracing::error!(tool = tool.key(), stage = "install", error = %reason, "tool unavailable after installation attempt");
                    immediate.insert(tool.key(), unavailable_report(tool, reason));
                    continue;
                }
            }
        };

        let target = target.clone();
        let admitter = Arc::clone(&admitter);
        #[rustfmt::skip]
        handles.push(tokio::spawn(
            tag(tool, run_one(tool, bin, target, timeout, admitter)).instrument(tracing::info_span!("tool_run", tool = tool.key())),
        ));
    }

    for h in handles {
        match h.await {
            Ok((tool, report)) => {
                immediate.insert(tool.key(), report);
            }
            Err(e) => {
                eprintln!("uni: internal error: a tool task panicked: {e}");
                // The task panicked; we don't know which tool without the
                // join set telling us, but panics here would be a uni bug,
                // not a tool result, so surface it loudly.
            }
        }
    }

    let tools: Vec<ToolReport> = ToolId::ALL
        .into_iter()
        .map(|t| {
            immediate
                .remove(t.key())
                .unwrap_or_else(|| error_report(t, "no result recorded".to_string(), None, None))
        })
        .collect();

    let overall: Overall = Report::compute_overall(&tools);
    let suite = Report::compute_suite(&tools);

    Ok(Report {
        schema: "uni.report/v2",
        target: target.display().to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        tools_dir: tools_dir.display().to_string(),
        tools,
        overall,
        suite,
    })
}

/// Resolve an installed tool, or bootstrap its associated elci-group checkout
/// and install it through Baby. Bootstrap is deliberately attempted only for
/// selected tools: skipped/opt-in tools never cause network or filesystem work.
async fn ensure_binary(tool: ToolId, tools_dir: &Path) -> Result<PathBuf, String> {
    if let Some(bin) = tool::resolve_binary(tool, tools_dir) {
        return Ok(bin);
    }

    // One installation at a time across concurrent Uni processes sharing a
    // tools directory. This lock is held through clone, recipe validation,
    // build, install, and executable verification.
    let _install_lock = InstallLock::acquire(tools_dir, Duration::from_secs(300)).await?;
    if let Some(bin) = tool::resolve_binary(tool, tools_dir) {
        return Ok(bin);
    }

    let repo = tools_dir.join(tool.repo_dir());
    if !repo.is_dir() {
        if let Err(e) = std::fs::create_dir_all(tools_dir) {
            eprintln!(
                "uni: could not create tools directory {} for {}: {e}",
                tools_dir.display(),
                tool.key()
            );
            return Err(format!(
                "could not create tools directory {}: {e}",
                tools_dir.display()
            ));
        }

        let git = match tool::resolve_binary_by_name("git") {
            Some(bin) => bin,
            None => {
                eprintln!("uni: cannot bootstrap {}: git is not installed", tool.key());
                return Err("git is not installed".to_string());
            }
        };
        let url = tool.repo_url();
        let mut cmd = tokio::process::Command::new(git);
        cmd.args(["clone", "--depth", "1", url])
            .arg(&repo)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let output = run_bootstrap_stage(
            cmd,
            tool,
            "clone",
            format!("cloning {} from {url}", tool.key()),
        )
        .await;
        match output {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                eprintln!(
                    "uni: clone failed for {}: {}",
                    tool.key(),
                    String::from_utf8_lossy(&out.stderr)
                        .chars()
                        .take(400)
                        .collect::<String>()
                );
                tracing::error!(tool = tool.key(), stage = "clone", exit_code = ?out.status.code(), "tool repository clone failed");
                return Err(format!(
                    "clone failed for {}: {}",
                    tool.key(),
                    diagnostic(&out)
                ));
            }
            Err(e) => {
                eprintln!("uni: failed to start git for {}: {e}", tool.key());
                return Err(format!("failed to start git for {}: {e}", tool.key()));
            }
        }
    }

    let baby = match tool::resolve_binary_by_name("baby") {
        Some(bin) => bin,
        None => {
            eprintln!(
                "uni: cannot install {} from {}: baby is not installed",
                tool.key(),
                repo.display()
            );
            return Err(
                "Baby is not installed; cannot validate an installation recipe".to_string(),
            );
        }
    };

    let mut validate = tokio::process::Command::new(&baby);
    validate
        .arg("--check-recipe")
        .current_dir(&repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let validation = run_bootstrap_stage(
        validate,
        tool,
        "validate_recipe",
        format!("validating {} installation recipe", tool.key()),
    )
    .await
    .map_err(|e| {
        tracing::error!(tool = tool.key(), stage = "validate_recipe", error = %e, "failed to start Baby recipe validation");
        format!(
            "failed to start Baby recipe validation for {}: {e}",
            tool.key()
        )
    })?;
    persist_install_log(tools_dir, tool, "validate", &validation);
    if !validation.status.success() {
        tracing::error!(tool = tool.key(), stage = "validate_recipe", exit_code = ?validation.status.code(), "installation recipe validation failed");
        return Err(format!(
            "installation recipe validation failed for {}: {}",
            tool.key(),
            diagnostic(&validation)
        ));
    }

    let mut cmd = tokio::process::Command::new(baby);
    cmd.arg("--user")
        .current_dir(&repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let output = run_bootstrap_stage(
        cmd,
        tool,
        "install",
        format!("installing {} with Baby", tool.key()),
    )
    .await;
    match output {
        Ok(out) if out.status.success() => {
            persist_install_log(tools_dir, tool, "install", &out);
            tool::resolve_binary(tool, tools_dir).ok_or_else(|| {
                tracing::error!(
                    tool = tool.key(),
                    stage = "verify_binary",
                    "Baby succeeded but expected executable was not found"
                );
                format!(
                    "Baby reported success, but expected executable {:?} was not found; see {}",
                    tool.key(),
                    install_log_path(tools_dir, tool, "install").display()
                )
            })
        }
        Ok(out) => {
            persist_install_log(tools_dir, tool, "install", &out);
            tracing::error!(tool = tool.key(), stage = "install", exit_code = ?out.status.code(), "Baby installation failed");
            Err(format!(
                "Baby installation failed for {}: {}; complete log: {}",
                tool.key(),
                diagnostic(&out),
                install_log_path(tools_dir, tool, "install").display()
            ))
        }
        Err(e) => {
            tracing::error!(tool = tool.key(), stage = "install", error = %e, "failed to start Baby");
            Err(format!("failed to start Baby for {}: {e}", tool.key()))
        }
    }
}

struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    async fn acquire(tools_dir: &Path, timeout: Duration) -> Result<Self, String> {
        let state_dir = tools_dir.join(".uni");
        std::fs::create_dir_all(&state_dir).map_err(|e| {
            format!(
                "could not create installation scheduler directory {}: {e}",
                state_dir.display()
            )
        })?;
        let path = state_dir.join("install.lock");
        let started = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write as _;
                    if let Err(e) = writeln!(file, "pid={}", std::process::id()) {
                        eprintln!(
                            "uni: tool_install_scheduler stage=record_owner outcome=failed error={e}"
                        );
                    }
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if install_lock_owner_is_gone(&path) {
                        match std::fs::remove_file(&path) {
                            Ok(()) => {
                                eprintln!(
                                    "uni: tool_install_scheduler stage=reclaim outcome=success path={}",
                                    path.display()
                                );
                                continue;
                            }
                            Err(remove_error) if remove_error.kind() == io::ErrorKind::NotFound => {
                                continue;
                            }
                            Err(remove_error) => {
                                eprintln!(
                                    "uni: tool_install_scheduler stage=reclaim outcome=failed path={} error={remove_error}",
                                    path.display()
                                );
                            }
                        }
                    }
                    if started.elapsed() >= timeout {
                        tracing::error!(stage = "install_scheduler", timeout_s = timeout.as_secs(), path = %path.display(), "timed out waiting for installation lock");
                        return Err(format!(
                            "installation scheduler timed out after {}s waiting for {}; another Uni process may still be installing",
                            timeout.as_secs(),
                            path.display()
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(e) => {
                    tracing::error!(stage = "install_scheduler", path = %path.display(), error = %e, "could not acquire installation lock");
                    return Err(format!(
                        "could not acquire installation scheduler lock {}: {e}",
                        path.display()
                    ));
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn install_lock_owner_is_gone(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Some(value) = contents.trim().strip_prefix("pid=") else {
        return false;
    };
    let pid = match value.parse::<u32>() {
        Ok(pid) => pid,
        Err(error) => {
            tracing::warn!(stage = "install_scheduler", path = %path.display(), error = %error, "installation lock has an invalid owner PID");
            return false;
        }
    };
    !Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn install_lock_owner_is_gone(_path: &Path) -> bool {
    false
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            eprintln!(
                "uni: tool_install_scheduler stage=release outcome=failed path={} error={e}",
                self.path.display()
            );
        }
    }
}

fn install_log_path(tools_dir: &Path, tool: ToolId, stage: &str) -> PathBuf {
    tools_dir
        .join(".uni")
        .join("install-logs")
        .join(format!("{}-{stage}.log", tool.key()))
}

fn persist_install_log(tools_dir: &Path, tool: ToolId, stage: &str, output: &Output) {
    let path = install_log_path(tools_dir, tool, stage);
    let Some(parent) = path.parent() else { return };
    if let Err(e) = std::fs::create_dir_all(parent) {
        eprintln!(
            "uni: could not create install log directory {}: {e}",
            parent.display()
        );
        return;
    }
    let mut bytes = output.stdout.clone();
    bytes.extend_from_slice(b"\n--- stderr ---\n");
    bytes.extend_from_slice(&output.stderr);
    if let Err(e) = std::fs::write(&path, bytes) {
        eprintln!("uni: could not write install log {}: {e}", path.display());
    }
}

fn diagnostic(output: &Output) -> String {
    let text = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    String::from_utf8_lossy(text)
        .chars()
        .take(400)
        .collect::<String>()
}

/// Run one visible bootstrap stage. Interactive terminals get a compact
/// spinner; redirected/CI output gets one stable start line. Every path emits
/// local-only telemetry to stderr, including spawn failures.
async fn run_bootstrap_stage(
    mut cmd: tokio::process::Command,
    tool: ToolId,
    stage: &'static str,
    label: String,
) -> io::Result<Output> {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

    let animated = io::stderr().is_terminal();
    if !animated {
        eprintln!("uni: {label}...");
    }

    let started = Instant::now();
    let result = if animated {
        let future = cmd.output();
        tokio::pin!(future);
        let mut ticker = tokio::time::interval(Duration::from_millis(80));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut frame = 0usize;

        loop {
            tokio::select! {
                result = &mut future => break result,
                _ = ticker.tick() => {
                    eprint!("\r\x1b[2K{} {label}", FRAMES[frame % FRAMES.len()]);
                    if let Err(e) = io::stderr().flush() {
                        eprintln!("uni: stderr flush failed: {e}");
                    }
                    frame += 1;
                }
            }
        }
    } else {
        cmd.output().await
    };
    let duration_ms = started.elapsed().as_millis();

    if animated {
        eprint!("\r\x1b[2K");
        if let Err(e) = io::stderr().flush() {
            eprintln!("uni: stderr flush failed: {e}");
        }
    }

    let (outcome, exit_code) = match &result {
        Ok(output) if output.status.success() => ("success", output.status.code()),
        Ok(output) => ("failed", output.status.code()),
        Err(_) => ("spawn_error", None),
    };
    eprintln!(
        "{}",
        bootstrap_telemetry_line(tool, stage, outcome, exit_code, duration_ms)
    );
    result
}

fn bootstrap_telemetry_line(
    tool: ToolId,
    stage: &str,
    outcome: &str,
    exit_code: Option<i32>,
    duration_ms: u128,
) -> String {
    let exit_code = exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!(
        "uni telemetry (local only): tool={} stage={stage} outcome={outcome} exit_code={exit_code} duration_ms={duration_ms}",
        tool.key()
    )
}

fn build_command(tool: ToolId, bin: &Path, target: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(bin);
    match tool {
        ToolId::Amber => {
            cmd.arg(target).args(["--format", "json", "analyze"]);
        }
        ToolId::Ami => {
            // show-project only: fast, local, no network/Groq. The full
            // `ami analyze` discovery pipeline is out of scope for a
            // concurrent snapshot run (it makes real outbound requests).
            cmd.args(["show-project", "--path"]).arg(target);
            cmd.env("NO_COLOR", "1");
        }
        ToolId::Bart => {
            cmd.args(["--json", "-d", "3", "-n", "0"]).arg(target);
        }
        ToolId::Chakra => {
            cmd.arg(target).arg("--json");
        }
        ToolId::Ferret => unreachable!("ferret is capability-probed by run_one"),
        ToolId::Fract => {
            cmd.current_dir(target);
            cmd.args(["index", "--format", "json"]);
        }
        ToolId::Isopod => {
            cmd.arg("--base").arg(target).args(["check", "--json"]);
        }
        ToolId::Lwoodz => {
            cmd.current_dir(target);
            cmd.args(["--audit", "--json"]);
        }
        ToolId::Tempcheq => {
            cmd.arg(target).arg("--report");
        }
        ToolId::Traci => {
            cmd.arg("check").arg(target).args(["--format", "json"]);
        }
        ToolId::Jeenome => unreachable!("jeenome is built by run_jeenome"),
        ToolId::Vamos => unreachable!("vamos is built by run_vamos"),
    }
    cmd
}

async fn run_one(
    tool: ToolId,
    bin: PathBuf,
    target: PathBuf,
    timeout: Duration,
    admitter: Arc<Admitter>,
) -> ToolReport {
    if let Some(provider) = tool.gate_provider() {
        let _ = admitter
            .wait_and_admit(provider, Option::<String>::None, None)
            .await;
    }

    let mut cmd = if tool == ToolId::Ferret {
        let Some(subcommand) = ferret_hunt_subcommand(&bin).await else {
            return incompatible_report(
                tool,
                &bin,
                "installed ferret supports neither `hunt` nor the newer project-review alias `track`; update ferret and rerun uni"
                    .to_string(),
            );
        };
        let mut cmd = tokio::process::Command::new(&bin);
        // Keep Ferret's learning corpus in memory so a uni snapshot never
        // writes ferret.db into the caller or the analyzed project.
        cmd.args(["--database", ":memory:", subcommand])
            .arg(&target);
        cmd.env("NO_COLOR", "1");
        cmd
    } else {
        build_command(tool, &bin, &target)
    };
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let start = Instant::now();
    let outcome = tokio::time::timeout(timeout, cmd.output()).await;
    let duration_ms = start.elapsed().as_millis();

    if let Some(provider) = tool.gate_provider() {
        admitter.complete(provider, Option::<String>::None).await;
    }

    let output = match outcome {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            eprintln!(
                "uni: tool={} stage=spawn outcome=failed error={e}",
                tool.key()
            );
            return error_report(
                tool,
                format!("failed to spawn: {e}"),
                None,
                Some(duration_ms),
            );
        }
        Err(_) => {
            eprintln!(
                "uni: tool={} stage=run outcome=timeout timeout_s={}",
                tool.key(),
                timeout.as_secs()
            );
            return error_report(
                tool,
                format!("timed out after {}s", timeout.as_secs()),
                None,
                Some(duration_ms),
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();

    if stdout.trim().is_empty() {
        let stderr_snippet: String = stderr.chars().take(300).collect();
        return error_report(
            tool,
            format!("produced no stdout; stderr: {stderr_snippet}"),
            exit_code,
            Some(duration_ms),
        );
    }

    let parsed = parsers::parse(tool, &stdout, exit_code);
    let evidence = evidence_for(tool, parsed.raw.as_ref());
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: parsed.status,
        availability: Availability::Installed,
        execution: if parsed.status == Status::Error {
            Execution::Failed
        } else {
            Execution::Succeeded
        },
        evidence,
        binary: Some(bin.display().to_string()),
        score: parsed.score,
        grade: parsed.score.map(letter_for),
        exit_code,
        duration_ms: Some(duration_ms),
        summary: parsed.summary,
        findings: parsed.findings,
        note: parsed.note,
        raw: parsed.raw,
    }
}

/// Ferret has shipped the project-wide review under both names: prefer the
/// requested `hunt` spelling and retain compatibility with builds that expose
/// it as `track`. A help probe is side-effect free and avoids guessing from a
/// version string.
async fn ferret_hunt_subcommand(bin: &Path) -> Option<&'static str> {
    for subcommand in ["hunt", "track"] {
        let mut cmd = tokio::process::Command::new(bin);
        cmd.args([subcommand, "--help"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null());
        if matches!(
            tokio::time::timeout(Duration::from_secs(5), cmd.status()).await,
            Ok(Ok(status)) if status.success()
        ) {
            return Some(subcommand);
        }
    }
    None
}

async fn run_jeenome(
    target: PathBuf,
    trace_override: Option<PathBuf>,
    timeout: Duration,
    admitter: Arc<Admitter>,
    tools_dir: PathBuf,
) -> ToolReport {
    if std::env::var_os("GROQ_API_KEY").is_none() {
        return skipped_report(
            ToolId::Jeenome,
            "GROQ_API_KEY is not set; jeenome streams behavioural analysis to Groq and cannot run without it".to_string(),
        );
    }

    let bin = match tool::resolve_binary(ToolId::Jeenome, &tools_dir) {
        Some(b) => b,
        None => {
            return unavailable_report(
                ToolId::Jeenome,
                "binary \"jeenome\" not found on PATH or under the tools dir".to_string(),
            )
        }
    };

    let (trace_path, generated) = match trace_override {
        Some(p) => (p, false),
        None => match generate_trace(&target, timeout).await {
            Ok(p) => (p, true),
            Err(note) => {
                eprintln!("uni: tool=jeenome stage=generate_trace outcome=skipped reason={note}");
                return skipped_report(ToolId::Jeenome, note);
            }
        },
    };

    let _ = admitter
        .wait_and_admit("groq", Option::<String>::None, None)
        .await;

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("-i")
        .arg(&trace_path)
        .args(["--format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let start = Instant::now();
    let outcome = tokio::time::timeout(timeout, cmd.output()).await;
    let duration_ms = start.elapsed().as_millis();

    admitter.complete("groq", Option::<String>::None).await;

    if generated {
        if let Err(e) = std::fs::remove_file(&trace_path) {
            eprintln!(
                "uni: tool=jeenome stage=cleanup outcome=failed path={} error={e}",
                trace_path.display()
            );
        }
    }

    let output = match outcome {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            eprintln!("uni: tool=jeenome stage=spawn outcome=failed error={e}");
            return error_report(
                ToolId::Jeenome,
                format!("failed to spawn: {e}"),
                None,
                Some(duration_ms),
            );
        }
        Err(_) => {
            eprintln!(
                "uni: tool=jeenome stage=run outcome=timeout timeout_s={}",
                timeout.as_secs()
            );
            return error_report(
                ToolId::Jeenome,
                format!("timed out after {}s", timeout.as_secs()),
                None,
                Some(duration_ms),
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code();
    let parsed = parsers::parse(ToolId::Jeenome, &stdout, exit_code);
    let evidence = evidence_for(ToolId::Jeenome, parsed.raw.as_ref());

    ToolReport {
        tool: ToolId::Jeenome.key(),
        purpose: ToolId::Jeenome.purpose(),
        status: parsed.status,
        availability: Availability::Installed,
        execution: if parsed.status == Status::Error {
            Execution::Failed
        } else {
            Execution::Succeeded
        },
        evidence,
        binary: Some(bin.display().to_string()),
        score: parsed.score,
        grade: parsed.score.map(letter_for),
        exit_code,
        duration_ms: Some(duration_ms),
        summary: parsed.summary,
        findings: parsed.findings,
        note: parsed.note,
        raw: parsed.raw,
    }
}

/// vamos has a real "not applicable yet" state that's structural, not an
/// error: no `vamos.toml` means the project hasn't adopted action-lifecycle
/// tracking. Checking for it up front (instead of letting `vamos stats`
/// fail with "run vamos init first") lets that report as Skipped rather
/// than Error — same reasoning as jeenome's precondition check.
async fn run_vamos(target: PathBuf, timeout: Duration, tools_dir: PathBuf) -> ToolReport {
    let manifest_path = target.join("vamos.toml");
    if !manifest_path.is_file() {
        return skipped_report(
            ToolId::Vamos,
            "no vamos.toml in project root; vamos hasn't been adopted here yet — run `vamos init` in the target to start tracking action lifecycles".to_string(),
        );
    }

    let bin = match tool::resolve_binary(ToolId::Vamos, &tools_dir) {
        Some(b) => b,
        None => {
            return unavailable_report(
                ToolId::Vamos,
                "binary \"vamos\" not found on PATH or under the tools dir".to_string(),
            )
        }
    };

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("--manifest")
        .arg(&manifest_path)
        .arg("--session")
        .arg(target.join(".vamos/session.json"))
        .arg("stats")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let start = Instant::now();
    let outcome = tokio::time::timeout(timeout, cmd.output()).await;
    let duration_ms = start.elapsed().as_millis();

    let output = match outcome {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            eprintln!("uni: tool=vamos stage=spawn outcome=failed error={e}");
            return error_report(
                ToolId::Vamos,
                format!("failed to spawn: {e}"),
                None,
                Some(duration_ms),
            );
        }
        Err(_) => {
            eprintln!(
                "uni: tool=vamos stage=run outcome=timeout timeout_s={}",
                timeout.as_secs()
            );
            return error_report(
                ToolId::Vamos,
                format!("timed out after {}s", timeout.as_secs()),
                None,
                Some(duration_ms),
            );
        }
    };

    let exit_code = output.status.code();
    if exit_code != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet: String = stderr.chars().take(300).collect();
        return error_report(
            ToolId::Vamos,
            format!("vamos stats exited {exit_code:?}: {snippet}"),
            exit_code,
            Some(duration_ms),
        );
    }

    // Unlike the other tools, empty stdout is a legitimate result here
    // (vamos.toml exists but no instances recorded yet) — the parser
    // handles that itself rather than treating it as an error.
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let parsed = parsers::parse(ToolId::Vamos, &stdout, exit_code);
    let evidence = evidence_for(ToolId::Vamos, parsed.raw.as_ref());

    ToolReport {
        tool: ToolId::Vamos.key(),
        purpose: ToolId::Vamos.purpose(),
        status: parsed.status,
        availability: Availability::Installed,
        execution: if parsed.status == Status::Error {
            Execution::Failed
        } else {
            Execution::Succeeded
        },
        evidence,
        binary: Some(bin.display().to_string()),
        score: parsed.score,
        grade: parsed.score.map(letter_for),
        exit_code,
        duration_ms: Some(duration_ms),
        summary: parsed.summary,
        findings: parsed.findings,
        note: parsed.note,
        raw: parsed.raw,
    }
}

/// Best-effort: strace a `cargo build` in the target so jeenome has
/// something to analyze. Only attempted for Cargo projects, and only when
/// `strace` is available — jeenome has no meaning for a bare project path.
async fn generate_trace(target: &Path, timeout: Duration) -> Result<PathBuf, String> {
    if !target.join("Cargo.toml").is_file() {
        eprintln!("uni: tool=jeenome stage=generate_trace outcome=skipped reason=no_cargo_toml");
        return Err("no Cargo.toml under target; jeenome needs an strace log and uni only knows how to generate one for `cargo build` (pass --jeenome-trace to supply your own)".to_string());
    }
    let strace_bin = tool::resolve_binary_by_name("strace")
        .ok_or_else(|| "strace is not on PATH and no --jeenome-trace was given".to_string())?;

    let trace_path = std::env::temp_dir().join(format!("uni-jeenome-{}.trace", std::process::id()));

    let mut cmd = tokio::process::Command::new(&strace_bin);
    cmd.args(["-f", "-o"])
        .arg(&trace_path)
        .arg("--")
        .arg("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(target.join("Cargo.toml"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    match tokio::time::timeout(timeout, cmd.status()).await {
        Ok(Ok(_)) if trace_path.is_file() => Ok(trace_path),
        Ok(Ok(status)) => {
            eprintln!("uni: tool=jeenome stage=strace outcome=no_trace exit_status={status}");
            Err(format!(
                "strace/cargo build exited {status} and produced no trace file"
            ))
        }
        Ok(Err(e)) => {
            eprintln!("uni: tool=jeenome stage=strace outcome=spawn_failed error={e}");
            Err(format!("failed to run strace: {e}"))
        }
        Err(_) => {
            eprintln!(
                "uni: tool=jeenome stage=strace outcome=timeout timeout_s={}",
                timeout.as_secs()
            );
            Err(format!(
                "trace generation timed out after {}s",
                timeout.as_secs()
            ))
        }
    }
}

fn skipped_report(tool: ToolId, note: String) -> ToolReport {
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: Status::Skipped,
        availability: Availability::NotChecked,
        execution: Execution::Skipped,
        evidence: no_evidence(),
        binary: None,
        score: None,
        grade: None,
        exit_code: None,
        duration_ms: None,
        summary: "skipped".to_string(),
        findings: Vec::new(),
        note: Some(note),
        raw: None,
    }
}

fn installable_report(tool: ToolId, note: String) -> ToolReport {
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: Status::Unavailable,
        availability: Availability::Installable,
        execution: Execution::NotRun,
        evidence: no_evidence(),
        binary: None,
        score: None,
        grade: None,
        exit_code: None,
        duration_ms: None,
        summary: "known but not installed".to_string(),
        findings: Vec::new(),
        note: Some(note),
        raw: None,
    }
}

fn unavailable_report(tool: ToolId, note: String) -> ToolReport {
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: Status::Unavailable,
        availability: Availability::Unavailable,
        execution: Execution::NotRun,
        evidence: no_evidence(),
        binary: None,
        score: None,
        grade: None,
        exit_code: None,
        duration_ms: None,
        summary: "unavailable".to_string(),
        findings: Vec::new(),
        note: Some(note),
        raw: None,
    }
}

fn incompatible_report(tool: ToolId, bin: &Path, note: String) -> ToolReport {
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: Status::Unavailable,
        availability: Availability::Incompatible,
        execution: Execution::NotRun,
        evidence: no_evidence(),
        binary: Some(bin.display().to_string()),
        score: None,
        grade: None,
        exit_code: None,
        duration_ms: None,
        summary: "installed but incompatible".to_string(),
        findings: Vec::new(),
        note: Some(note),
        raw: None,
    }
}

fn error_report(
    tool: ToolId,
    note: String,
    exit_code: Option<i32>,
    duration_ms: Option<u128>,
) -> ToolReport {
    ToolReport {
        tool: tool.key(),
        purpose: tool.purpose(),
        status: Status::Error,
        availability: Availability::Installed,
        execution: Execution::Failed,
        evidence: no_evidence(),
        binary: None,
        score: None,
        grade: None,
        exit_code,
        duration_ms,
        summary: "error".to_string(),
        findings: Vec::new(),
        note: Some(note),
        raw: None,
    }
}

fn no_evidence() -> Evidence {
    Evidence {
        coverage: None,
        confidence: None,
        observations: None,
    }
}

fn evidence_for(tool: ToolId, raw: Option<&serde_json::Value>) -> Evidence {
    use serde_json::Value;
    let Some(root) = raw else {
        return no_evidence();
    };
    match tool {
        ToolId::Chakra => {
            let total = root.pointer("/metadata/file_count").and_then(Value::as_u64);
            let analyzed = root
                .pointer("/metadata/analyzed_file_count")
                .and_then(Value::as_u64);
            let flows = root.get("flows").and_then(Value::as_array);
            let confidence = flows.and_then(|items| {
                if items.is_empty() {
                    None
                } else {
                    Some(
                        items
                            .iter()
                            .filter_map(|item| item.get("confidence").and_then(Value::as_f64))
                            .sum::<f64>()
                            / items.len() as f64,
                    )
                }
            });
            Evidence {
                coverage: total.zip(analyzed).and_then(|(total, analyzed)| {
                    (total > 0).then_some(analyzed as f64 / total as f64)
                }),
                confidence,
                observations: flows.map(|items| items.len() as u64),
            }
        }
        ToolId::Isopod => {
            let controls = root.as_array();
            let assessed = controls.map(|items| {
                items
                    .iter()
                    .filter(|item| item.get("status").and_then(Value::as_str) != Some("UNKNOWN"))
                    .count() as u64
            });
            let total = controls.map(|items| items.len() as u64);
            let coverage = total.zip(assessed).and_then(|(total, assessed)| {
                (total > 0).then_some(assessed as f64 / total as f64)
            });
            Evidence {
                coverage,
                confidence: coverage,
                observations: assessed,
            }
        }
        ToolId::Tempcheq => Evidence {
            coverage: None,
            confidence: None,
            observations: root.get("total_actions").and_then(Value::as_u64),
        },
        ToolId::Amber => Evidence {
            coverage: None,
            confidence: None,
            observations: root.get("total_dependencies").and_then(Value::as_u64),
        },
        ToolId::Traci => Evidence {
            coverage: None,
            confidence: None,
            observations: root.pointer("/summary/diagnostics").and_then(Value::as_u64),
        },
        _ => no_evidence(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_telemetry_is_explicit_and_local_only() {
        let line = bootstrap_telemetry_line(ToolId::Ferret, "install", "success", Some(0), 42);
        assert_eq!(
            line,
            "uni telemetry (local only): tool=ferret stage=install outcome=success exit_code=0 duration_ms=42"
        );
    }

    #[test]
    fn bootstrap_telemetry_handles_spawn_errors_without_fake_exit_codes() {
        let line = bootstrap_telemetry_line(ToolId::Tempcheq, "clone", "spawn_error", None, 7);
        assert!(line.contains("outcome=spawn_error"));
        assert!(line.contains("exit_code=none"));
    }

    #[tokio::test]
    async fn installation_scheduler_reclaims_dead_owner_and_releases_on_drop() {
        let root =
            std::env::temp_dir().join(format!("uni-install-lock-test-{}", std::process::id()));
        let state = root.join(".uni");
        std::fs::create_dir_all(&state).unwrap();
        let path = state.join("install.lock");
        std::fs::write(&path, "pid=4294967295\n").unwrap();

        let lock = InstallLock::acquire(&root, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(path.is_file());
        drop(lock);
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

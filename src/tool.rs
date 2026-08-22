// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! The fixed set of tools `uni` orchestrates, in the single canonical order
//! every report uses (alphabetical). Keeping this order fixed here — rather
//! than deriving it from HashMap iteration or CLI arg order — is what makes
//! `uni`'s report structurally deterministic run to run.

use std::env;
use std::path::{Path, PathBuf};

/// Where sibling tool checkouts live, absent an explicit `--tools-dir`:
/// the parent directory of uni's own source tree (uni lives at
/// `<tools_dir>/uni`, every other tool at `<tools_dir>/<name>`).
pub fn default_tools_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/home/sal"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolId {
    Amber,
    Ami,
    Bart,
    Chakra,
    Ferret,
    Fract,
    Isopod,
    Jeenome,
    Lwoodz,
    Tempcheq,
    Traci,
    Vamos,
}

impl ToolId {
    /// Every tool, in canonical (alphabetical) order.
    pub const ALL: [ToolId; 12] = [
        ToolId::Amber,
        ToolId::Ami,
        ToolId::Bart,
        ToolId::Chakra,
        ToolId::Ferret,
        ToolId::Fract,
        ToolId::Isopod,
        ToolId::Jeenome,
        ToolId::Lwoodz,
        ToolId::Tempcheq,
        ToolId::Traci,
        ToolId::Vamos,
    ];

    /// CLI/report key, lowercase.
    pub fn key(self) -> &'static str {
        match self {
            ToolId::Amber => "amber",
            ToolId::Ami => "ami",
            ToolId::Bart => "bart",
            ToolId::Chakra => "chakra",
            ToolId::Ferret => "ferret",
            ToolId::Fract => "fract",
            ToolId::Isopod => "isopod",
            ToolId::Jeenome => "jeenome",
            ToolId::Lwoodz => "lwoodz",
            ToolId::Tempcheq => "tempcheq",
            ToolId::Traci => "traci",
            ToolId::Vamos => "vamos",
        }
    }

    pub fn from_key(key: &str) -> Option<ToolId> {
        ToolId::ALL.into_iter().find(|t| t.key() == key)
    }

    /// Sibling checkout directory name under the tools root.
    pub fn repo_dir(self) -> &'static str {
        self.key()
    }

    /// The binary this tool is invoked as (differs from the repo name nowhere
    /// here, but kept separate since e.g. lwoodz ships two binaries).
    fn bin_name(self) -> &'static str {
        self.key()
    }

    /// Canonical source repository used to bootstrap an absent application.
    pub fn repo_url(self) -> &'static str {
        match self {
            ToolId::Amber => "https://github.com/elci-group/amber.git",
            ToolId::Ami => "https://github.com/elci-group/ami.git",
            ToolId::Bart => "https://github.com/elci-group/bart.git",
            ToolId::Chakra => "https://github.com/elci-group/chakra.git",
            ToolId::Ferret => "https://github.com/elci-group/ferret.git",
            ToolId::Fract => "https://github.com/elci-group/fract.git",
            ToolId::Isopod => "https://github.com/elci-group/isopod.git",
            ToolId::Jeenome => "https://github.com/elci-group/jeenome.git",
            ToolId::Lwoodz => "https://github.com/elci-group/lwoodz.git",
            ToolId::Tempcheq => "https://github.com/elci-group/tempcheq.git",
            ToolId::Traci => "https://github.com/elci-group/traci.git",
            ToolId::Vamos => "https://github.com/elci-group/vamos.git",
        }
    }

    /// Tools not run by default (need extra input/setup beyond a project
    /// path). Only jeenome today: it audits an strace log, not a project.
    pub fn enabled_by_default(self) -> bool {
        !matches!(self, ToolId::Jeenome)
    }

    /// Tools that make a live LLM call and so should be admitted through
    /// ingauge before spawning, to avoid saturating the shared quota.
    pub fn needs_gate(self) -> bool {
        matches!(self, ToolId::Jeenome | ToolId::Lwoodz)
    }

    pub fn gate_provider(self) -> Option<&'static str> {
        if self.needs_gate() {
            Some("groq")
        } else {
            None
        }
    }

    /// One-line description of what this tool measures, for the report.
    pub fn purpose(self) -> &'static str {
        match self {
            ToolId::Amber => "dependency bloat / replaceability",
            ToolId::Ami => "project profile completeness (market-intelligence readiness)",
            ToolId::Bart => "filesystem size & hotspots (informational)",
            ToolId::Chakra => "data-flow / architecture map coverage",
            ToolId::Ferret => "repository-specific review findings (hunt)",
            ToolId::Fract => "module entropy, cohesion, duplication",
            ToolId::Isopod => "ISO27001/27002 compliance posture",
            ToolId::Jeenome => "behavioural trace analysis (opt-in)",
            ToolId::Lwoodz => "license / SPDX compliance",
            ToolId::Tempcheq => "LLM sampling-temperature correctness",
            ToolId::Traci => "observability/telemetry completeness",
            ToolId::Vamos => "nominal vs. validated action completion",
        }
    }
}

/// Where to find a tool's binary: a resolved absolute path, or nothing.
pub fn resolve_binary(tool: ToolId, tools_dir: &Path) -> Option<PathBuf> {
    let bin_name = tool.bin_name();

    // Prefer the explicitly selected tools directory over PATH. PATH often
    // contains an older system build whose behavior no longer matches the
    // checked-out ELCI tool (for example Fract's dependency exclusions).
    let repo = tools_dir.join(tool.repo_dir());
    for profile in ["release", "debug"] {
        let candidate = repo.join("target").join(profile).join(bin_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    if let Some(found) = find_on_path(bin_name) {
        return Some(found);
    }

    let local_bin = env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin").join(bin_name));
    if let Some(candidate) = local_bin {
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Resolve an arbitrary binary name on PATH (used for `strace`, which isn't
/// one of the nine orchestrated tools but is needed to feed jeenome).
pub fn resolve_binary_by_name(name: &str) -> Option<PathBuf> {
    find_on_path(name)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_its_associated_elci_group_repo() {
        for tool in ToolId::ALL {
            assert_eq!(
                tool.repo_url(),
                format!("https://github.com/elci-group/{}.git", tool.repo_dir())
            );
        }
    }
}

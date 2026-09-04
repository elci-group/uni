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
    Scrawny,
    Tempcheq,
    Traci,
    Vamos,
    VivaPalestina,
    Wilder,
}

impl ToolId {
    /// Every tool, in canonical (alphabetical) order.
    pub const ALL: [ToolId; 15] = [
        ToolId::Amber,
        ToolId::Ami,
        ToolId::Bart,
        ToolId::Chakra,
        ToolId::Ferret,
        ToolId::Fract,
        ToolId::Isopod,
        ToolId::Jeenome,
        ToolId::Lwoodz,
        ToolId::Scrawny,
        ToolId::Tempcheq,
        ToolId::Traci,
        ToolId::Vamos,
        ToolId::VivaPalestina,
        ToolId::Wilder,
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
            ToolId::Scrawny => "scrawny",
            ToolId::Tempcheq => "tempcheq",
            ToolId::Traci => "traci",
            ToolId::Vamos => "vamos",
            ToolId::VivaPalestina => "viva-palestina",
            ToolId::Wilder => "wilder",
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
    pub fn repo_url(self) -> Option<&'static str> {
        Some(match self {
            ToolId::Amber => "https://github.com/elci-group/amber.git",
            ToolId::Ami => "https://github.com/elci-group/ami.git",
            ToolId::Bart => "https://github.com/elci-group/bart.git",
            ToolId::Chakra => "https://github.com/elci-group/chakra.git",
            ToolId::Ferret => "https://github.com/elci-group/ferret.git",
            ToolId::Fract => "https://github.com/elci-group/fract.git",
            ToolId::Isopod => "https://github.com/elci-group/isopod.git",
            ToolId::Jeenome => "https://github.com/elci-group/jeenome.git",
            ToolId::Lwoodz => "https://github.com/elci-group/lwoodz.git",
            ToolId::Scrawny => "https://github.com/elci-group/scrawny.git",
            ToolId::Tempcheq => "https://github.com/elci-group/tempcheq.git",
            ToolId::Traci => "https://github.com/elci-group/traci.git",
            ToolId::Vamos => return None,
            ToolId::VivaPalestina => "https://github.com/elci-group/viva-palestina.git",
            ToolId::Wilder => "https://github.com/elci-group/wilder.git",
        })
    }

    /// Tools not run by default (need extra input/setup beyond a project
    /// path). Only jeenome today: it audits an strace log, not a project.
    pub fn enabled_by_default(self) -> bool {
        !matches!(self, ToolId::Ami | ToolId::Jeenome)
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

    /// The tool's own metaphorical identity — the device its native
    /// terminal renderer already stages through
    /// [`form3`](https://github.com/elci-group/3form), or (for tools that
    /// don't yet draw on `form3`) the closest reading of its name and
    /// purpose. `--technical` output relays this next to each tool's
    /// findings so the report speaks in each source tool's own voice
    /// instead of a flattened Uni-wide vocabulary — the same principle
    /// that keeps `tool_accent`'s native palettes in `render.rs`.
    pub fn metaphor(self) -> &'static str {
        match self {
            ToolId::Amber => "the fossil: preserves what's worth keeping in resin, flags what should be let go",
            ToolId::Ami => "the scout: reads the room before the room reads you",
            ToolId::Bart => "the surveyor: maps the terrain and its heaviest patches",
            ToolId::Chakra => "the mystic aura: traces energy flowing along the system's channels",
            ToolId::Ferret => "the hunter: digs through the burrow for what's buried",
            ToolId::Fract => "the glass: shows the cracks before they shatter",
            ToolId::Isopod => "the roly-poly: crawls every control, curling up tight where compliance fails",
            ToolId::Jeenome => "the detective: reconstructs what happened from the trace",
            ToolId::Lwoodz => "the woodsman: checks the timber's paperwork before it ships",
            ToolId::Scrawny => "the sparring partner: throws every punch a hostile reviewer would",
            ToolId::Tempcheq => "the thermostat: checks the model's sampling temperature is actually correct",
            ToolId::Traci => "the nervous system: senses whether the project can feel its own pain",
            ToolId::Vamos => "the road trip: checks you actually arrived, not just that you left",
            ToolId::VivaPalestina => "the conscience: vets who you're doing business with",
            ToolId::Wilder => "the ranger: orchestrates evidence out in the wild",
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
            ToolId::Scrawny => "review-hostility of current changes",
            ToolId::Tempcheq => "LLM sampling-temperature correctness",
            ToolId::Traci => "observability/telemetry completeness",
            ToolId::Vamos => "nominal vs. validated action completion",
            ToolId::VivaPalestina => "ethical vendor / dependency policy compliance",
            ToolId::Wilder => "repository evidence & coverage orchestration",
        }
    }
}

/// Where to find a tool's binary: a resolved absolute path, or nothing.
pub fn resolve_binary(tool: ToolId, tools_dir: &Path) -> Option<PathBuf> {
    let bin_name = tool.bin_name();

    // Prefer the explicitly selected tools directory over PATH. PATH often
    // contains an older system build whose behavior no longer matches the
    // checked-out ELCI tool (for example Fract's dependency exclusions).
    let mut repos = vec![tools_dir.join(tool.repo_dir())];
    // The validated-action-lifecycle tool historically shipped from a repo
    // named `vamos`, but the local checkout that matches `uni`'s CLI contract
    // lives at `vamos-lifecycle`. Prefer the contract-correct checkout when it
    // exists, and still fall back to the legacy directory name.
    if tool == ToolId::Vamos {
        repos.push(tools_dir.join("vamos-lifecycle"));
    }

    for repo in repos {
        for profile in ["release", "debug"] {
            let candidate = repo.join("target").join(profile).join(bin_name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
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
    fn public_tools_have_their_associated_elci_group_repo() {
        for tool in ToolId::ALL {
            let expected = format!("https://github.com/elci-group/{}.git", tool.repo_dir());
            if tool == ToolId::Vamos {
                assert_eq!(tool.repo_url(), None);
            } else {
                assert_eq!(tool.repo_url(), Some(expected.as_str()));
            }
        }
    }

    #[test]
    fn every_tool_has_a_distinct_metaphor() {
        let metaphors: Vec<&str> = ToolId::ALL.iter().map(|t| t.metaphor()).collect();
        for m in &metaphors {
            assert!(!m.is_empty());
        }
        let mut unique = metaphors.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), metaphors.len(), "metaphors must not collide");
    }
}

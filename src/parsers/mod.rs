// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! One parser per tool: turns raw stdout + exit code into a normalized
//! [`ParseOutcome`]. Every parser is best-effort — if a tool's JSON doesn't
//! contain the fields we expect, we fall back to an ungraded `score: None`
//! result with a `note` explaining why, rather than guessing.

mod amber;
mod ami;
mod bart;
mod catskin;
mod chakra;
mod ferret;
mod fract;
mod isopod;
mod jeenome;
mod lwoodz;
mod scrawny;
mod tempcheq;
mod traci;
mod vamos;
mod viva_palestina;
mod wilder;

use crate::report::Status;
use crate::tool::ToolId;

pub struct ParseOutcome {
    pub status: Status,
    pub score: Option<f64>,
    pub summary: String,
    pub findings: Vec<String>,
    pub note: Option<String>,
    pub raw: Option<serde_json::Value>,
}

impl ParseOutcome {
    fn json_error(err: impl std::fmt::Display, stdout: &str) -> Self {
        let snippet: String = stdout.chars().take(300).collect();
        ParseOutcome {
            status: Status::Error,
            score: None,
            summary: "failed to parse tool output as JSON".to_string(),
            findings: Vec::new(),
            note: Some(format!("{err}; stdout began: {snippet:?}")),
            raw: None,
        }
    }
}

pub fn parse(tool: ToolId, stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    match tool {
        ToolId::Amber => amber::parse(stdout, exit_code),
        ToolId::Ami => ami::parse(stdout, exit_code),
        ToolId::Bart => bart::parse(stdout, exit_code),
        ToolId::Catskin => catskin::parse(stdout, exit_code),
        ToolId::Chakra => chakra::parse(stdout, exit_code),
        ToolId::Ferret => ferret::parse(stdout, exit_code),
        ToolId::Fract => fract::parse(stdout, exit_code),
        ToolId::Isopod => isopod::parse(stdout, exit_code),
        ToolId::Jeenome => jeenome::parse(stdout, exit_code),
        ToolId::Lwoodz => lwoodz::parse(stdout, exit_code),
        ToolId::Scrawny => scrawny::parse(stdout, exit_code),
        ToolId::Tempcheq => tempcheq::parse(stdout, exit_code),
        ToolId::Traci => traci::parse(stdout, exit_code),
        ToolId::Vamos => vamos::parse(stdout, exit_code),
        ToolId::VivaPalestina => viva_palestina::parse(stdout, exit_code),
        ToolId::Wilder => wilder::parse(stdout, exit_code),
    }
}

pub(crate) fn clamp_score(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

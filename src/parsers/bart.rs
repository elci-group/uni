// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! bart reports filesystem size/hotspots, not code health — there is no
//! meaningful pass/fail signal here, so this is the one tool `uni` never
//! assigns a score to. It's included for the size/hotspot context only.
use super::ParseOutcome;
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, _exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=bart stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let size = root.get("size").and_then(Value::as_u64).unwrap_or(0);
    let file_count = root.get("file_count").and_then(Value::as_u64).unwrap_or(0);
    let mut children = root
        .get("children")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    children.sort_by_key(|c| std::cmp::Reverse(c.get("size").and_then(Value::as_u64).unwrap_or(0)));

    let summary = format!("{file_count} files, {} under root", human_bytes(size));

    let findings = children
        .iter()
        .take(5)
        .map(|c| {
            let path = c.get("path").and_then(Value::as_str).unwrap_or("?");
            let sz = c.get("size").and_then(Value::as_u64).unwrap_or(0);
            format!("{path}: {}", human_bytes(sz))
        })
        .collect();

    ParseOutcome {
        status: Status::Ok,
        score: None,
        summary,
        findings,
        note: Some("informational only — bart measures disk usage, not code health".to_string()),
        raw: Some(root),
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

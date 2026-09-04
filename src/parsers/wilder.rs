// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! wilder establishes repository evidence and coverage; it deliberately emits
//! no health score — its contract is that an analysis gap is not a failing
//! result — so we derive one from finding severities. Coverage gaps are
//! reported in the summary but not penalized: the other orchestrated tools
//! cover those domains. Exit code 2 means the analysis is incomplete, which
//! is expected at the 0.3 milestone, so it is surfaced as a note rather than
//! treated as an error.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;


    const FIXTURE: &str = r#"{
        "schema": "wilder.schema.v1",
        "coverage": {
            "analysis_percent": 27.3,
            "complete_domains": 6,
            "applicable_domains": 22,
            "domains": []
        },
        "evidence": [{"id": "WLD-EVID-1"}, {"id": "WLD-EVID-2"}],
        "findings": [
            {"id": "WLD-A", "title": "Change hotspot", "severity": "MEDIUM", "confidence": "HIGH"},
            {"id": "WLD-B", "title": "Unsafe surface", "severity": "HIGH", "confidence": "MEDIUM"},
            {"id": "WLD-C", "title": "Oversized file", "severity": "LOW", "confidence": "HIGH"},
            {"id": "WLD-D", "title": "Info note", "severity": "INFO", "confidence": "LOW"}
        ]
    }"#;

// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! scrawny measures how review-hostile the current working-tree change is.
//! `metrics.review_load.total` is already a normalized 0-100 index, so the
//! score is its complement. Status thresholds mirror scrawny's own default
//! policy (`scrawny check`): fail above 70 review load or below 0.55
//! cohesion, warn above 40 load or more than 4 concern types. A clean tree
//! scores 100 — it measures the diff, not the project.
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;



    fn fixture(load: f64, cohesion: f64, concerns: &str) -> String {
        format!(
            r#"{{"version": "2", "metrics": {{"review_load": {{"total": {load}, "size": 0.0, "concern_multiplicity": 0.0, "file_dispersion": 0.0, "mechanical_noise": 0.0, "behavioural_density": 0.0}}, "cohesion": {cohesion}, "lines_added": 10, "lines_removed": 5, "files_changed": 3}}, "concerns": {concerns}, "clusters": []}}"#
        )
    }

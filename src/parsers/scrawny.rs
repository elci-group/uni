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




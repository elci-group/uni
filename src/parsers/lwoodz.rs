// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
use super::{clamp_score, ParseOutcome};
use crate::report::Status;
use serde_json::Value;

pub fn parse(stdout: &str, exit_code: Option<i32>) -> ParseOutcome {
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("uni: tool=lwoodz stage=parse outcome=json_error error={e}");
            return ParseOutcome::json_error(e, stdout);
        }
    };

    let has_license = root
        .get("has_license_file")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let spdx_valid = root
        .get("spdx_valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let detected_license = root.get("detected_license").and_then(Value::as_str);

    let total_files = root
        .pointer("/header_coverage/total_files")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let with_header = root
        .pointer("/header_coverage/with_header")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let sampled_of_total = root
        .pointer("/header_coverage/sampled_of_total")
        .and_then(Value::as_u64);
    let header_ratio = if total_files > 0 {
        with_header as f64 / total_files as f64
    } else {
        1.0
    };

    let total_deps = root
        .pointer("/compatibility/total_deps")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let incompatible = root
        .pointer("/compatibility/incompatible")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let warnings = root
        .pointer("/compatibility/warnings")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let issues = root
        .pointer("/compatibility/issues")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Open-source vs open-standard differentiation and online-service/
    // inference terms-of-use: both additive to the core license audit, and
    // both absent from lwoodz releases before `lwoodz.openness-analysis/v1`
    // and `lwoodz.service-terms-analysis/v1`. Missing means "not run by this
    // lwoodz version", not "zero dependencies matched" — kept as `Option` so
    // an older lwoodz never gets penalized for a check it doesn't have.
    let rand_encumbered = root
        .pointer("/openness/rand_encumbered")
        .and_then(Value::as_u64);
    let open_standard_deps = root
        .pointer("/openness/open_standard")
        .and_then(Value::as_u64);
    let service_terms_matched = root
        .pointer("/service_terms/matched")
        .and_then(Value::as_u64);
    let service_terms_providers: Vec<&str> = root
        .pointer("/service_terms/providers")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let mut penalty = 0.0;
    if !has_license {
        penalty += 30.0;
    }
    if !spdx_valid {
        penalty += 15.0;
    }
    penalty += (1.0 - header_ratio) * 15.0;
    penalty += incompatible as f64 * 20.0;
    let warning_ratio = if total_deps > 0 {
        warnings as f64 / total_deps as f64
    } else {
        0.0
    };
    // Advisory dependency-license warnings scale with the dependency set.
    // A large tree containing many attribution notices must not look worse
    // than a missing project license or an actual incompatibility.
    penalty += warning_ratio * 20.0;
    // A RAND/FRAND-encumbered standard implementation is a review flag, not
    // a proven incompatibility (lwoodz itself reports it as a `warning`
    // finding, never an `error`) — weighted at half an incompatible
    // dependency's penalty for the same reason.
    penalty += rand_encumbered.unwrap_or(0) as f64 * 10.0;
    let score = clamp_score(100.0 - penalty);

    let status = if exit_code == Some(2) || incompatible > 0 || !has_license {
        Status::Fail
    } else if exit_code == Some(1)
        || warnings > 0
        || !spdx_valid
        || header_ratio < 0.5
        || rand_encumbered.unwrap_or(0) > 0
    {
        Status::Warn
    } else {
        Status::Ok
    };

    let sample_note = match sampled_of_total {
        Some(eligible) if eligible > total_files => {
            format!(" (sampled {total_files} of {eligible} eligible)")
        }
        _ => String::new(),
    };
    let mut summary = format!(
        "license={}, spdx_valid={spdx_valid}, header coverage {with_header}/{total_files}{sample_note}, {incompatible} incompatible / {warnings} warning issues among {total_deps} deps",
        detected_license.unwrap_or("unknown")
    );
    if let Some(open_standard_deps) = open_standard_deps {
        summary.push_str(&format!(
            ", {open_standard_deps} open-standard dep(s) ({} RAND-encumbered)",
            rand_encumbered.unwrap_or(0)
        ));
    }
    if let Some(matched) = service_terms_matched {
        summary.push_str(&format!(
            ", {matched} online-service/inference dep(s) flagged for terms-of-use review"
        ));
    }

    let mut findings: Vec<String> = issues
        .iter()
        .take(5)
        .map(|i| {
            let dep = i.get("dependency").and_then(Value::as_str).unwrap_or("?");
            let sev = i.get("severity").and_then(Value::as_str).unwrap_or("?");
            let reason = i.get("reason").and_then(Value::as_str).unwrap_or("");
            format!("{dep} [{sev}]: {reason}")
        })
        .collect();
    if rand_encumbered.unwrap_or(0) > 0 {
        findings.push(format!(
            "{} dependenc{} implement a RAND/FRAND-encumbered open standard — run `lwoodz openness` for citations",
            rand_encumbered.unwrap_or(0),
            if rand_encumbered.unwrap_or(0) == 1 { "y" } else { "ies" }
        ));
    }
    if let Some(matched) = service_terms_matched {
        if matched > 0 {
            findings.push(format!(
                "{matched} dependenc{} talk to a catalogued online service or inference provider ({}) — run `lwoodz service-terms` for terms of use",
                if matched == 1 { "y" } else { "ies" },
                service_terms_providers.join(", ")
            ));
        }
    }

    ParseOutcome {
        status,
        score: Some(score),
        summary,
        findings,
        note: Some(
            "advisory warning deductions are proportional to dependency count; missing licenses and incompatible dependencies remain hard findings"
                .to_string(),
        ),
        raw: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_json(extra: &str) -> String {
        format!(
            r#"{{"has_license_file":true,"spdx_valid":true,"detected_license":"MIT",
            "header_coverage":{{"total_files":10,"with_header":10}},
            "compatibility":{{"total_deps":5,"incompatible":0,"warnings":0,"issues":[]}}{extra}}}"#
        )
    }

    #[test]
    fn missing_openness_and_service_terms_fields_do_not_change_score_or_status() {
        let out = parse(&base_json(""), Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!((out.score.unwrap() - 100.0).abs() < 1e-9);
        assert!(!out.summary.contains("open-standard"));
        assert!(
            !out.summary.contains("service-terms") && !out.summary.contains("terms-of-use review")
        );
    }

    #[test]
    fn rand_encumbered_standard_dependency_downgrades_to_warn_with_a_finding() {
        let stdout = base_json(
            r#","openness":{"total_deps":5,"osi_approved":5,"open_standard":2,"rand_encumbered":1}"#,
        );
        let out = parse(&stdout, Some(0));
        assert_eq!(out.status, Status::Warn);
        assert!(out.score.unwrap() < 100.0);
        assert!(out
            .summary
            .contains("2 open-standard dep(s) (1 RAND-encumbered)"));
        assert!(out
            .findings
            .iter()
            .any(|f| f.contains("RAND/FRAND-encumbered")));
    }

    #[test]
    fn service_terms_matches_are_informational_and_never_change_score_or_status() {
        let stdout = base_json(r#","service_terms":{"matched":2,"providers":["Groq","Stripe"]}"#);
        let out = parse(&stdout, Some(0));
        assert_eq!(out.status, Status::Ok);
        assert!((out.score.unwrap() - 100.0).abs() < 1e-9);
        assert!(out
            .summary
            .contains("2 online-service/inference dep(s) flagged"));
        assert!(out.findings.iter().any(|f| f.contains("Groq, Stripe")));
    }
}

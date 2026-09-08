# The `uni.report/v3` result protocol

Uni separates the state of the analysis machinery from evidence about the
target project. A tool result contains these independent dimensions:

- `availability`: `installed`, `installable`, `incompatible`, `unavailable`,
  or `not_checked`.
- `execution`: `succeeded`, `failed`, `skipped`, or `not_run`.
- `status`: the analytical outcome, including `not_applicable` and `no_data`.
- `evidence.coverage`: the assessed fraction, when the tool exposes it.
- `evidence.confidence`: confidence in the produced evidence, when known.
- `evidence.observations`: the number of observations behind the result.
- `score`: optional project-health evidence. Zero observations, insufficient
  compliance coverage, execution errors, and unavailable tools are ungraded.

The top-level `suite` object reports availability, execution, valid-result,
coverage, and confidence measures independently of `overall`, which contains
only project-health scores. The `integrity` object grades the analysis pipeline
from its valid-result rate and lists compatibility, availability, and execution
defects. `overall.provisional` is true whenever integrity is not `healthy` or
when project evidence is materially incomplete.

`overall.score` is the unweighted mean of every graded tool's score, with one
exception: if any graded tool reports `status: fail`, the overall score is
capped at 73.0 (the bottom of the letter-grade "C" band) even when the
unweighted mean would be higher. This prevents a single critical-axis failure
(e.g. a missing license) from being averaged away by unrelated healthy scores
(e.g. module cohesion). The cap only lowers the score — it never raises an
average that was already below it.

Scrawny measures the current diff's review difficulty and is always ungraded
(`score: null`). Valid results use `ok` or `warn`, never `fail`; load above 40,
cohesion below 0.55, or more than four concern types warns. Warnings appear in
the human summaries' **Considerations** section, with metrics and findings.
JSON retains these in the Scrawny tool's `summary`, `findings`, `note`, and
`raw` fields. Scrawny contributes neither to the mean nor the failure cap.
Protocol and execution errors remain analysis defects.

`status: fail` means the analyzer ran successfully and found project issues;
it does not mean execution failed. Human reports render that state as
`findings`. Analyzer failures are identified by `execution: failed`,
`availability: incompatible|unavailable`, or `status: error`, and appear in a
separate **ANALYSIS DEFECTS** block.

Before invoking version-sensitive interfaces, Uni probes capabilities instead
of trusting a checkout name or version string. Lwoodz supports both its current
subcommand contract and its legacy flag contract. AMI is executed only when
`show-project --help` advertises JSON; decorated human output is never parsed.

Machine-readable integrations must emit JSON only on stdout. Human logs,
banners, progress indicators, ANSI control sequences, and diagnostics belong
on stderr. Uni treats invalid JSON as an execution/protocol error and excludes
that result from project scoring.

## Installation lifecycle

Missing tools are classified without changing the filesystem by default.
`--install-missing` opts into this lifecycle:

1. resolve the known repository;
2. clone it if necessary;
3. ask Baby to validate `.baby.toml` (`baby.install/v1`) or its Cargo metadata
   compatibility recipe;
4. acquire `<tools-dir>/.uni/install.lock` and run installations serially,
   including across concurrent Uni processes (dead Linux owners are reclaimed);
5. preserve complete validation/build logs under
   `<tools-dir>/.uni/install-logs/`;
6. verify that the expected executable can be resolved;
7. only then begin analysis with that tool.

This deliberately distinguishes repository availability from installability
and prevents a checkout directory name from being treated as a Cargo package
or executable name.

## Cohort reports (`uni.cohort/v1`)

`--cohort` produces a separate top-level shape, not a list of
`uni.report/v3` documents: `{schema, org, discovered, locally_available,
cycle: {batch_size, cycle_seconds}, repos: [...], rollup: {...}}`. Each
`repos[]` entry is a compact pointer — repo name, local path, the written
`report_file` name, overall score/grade, and integrity status — not the full
per-tool report; that full detail is what `report_file` points to on disk.
`rollup` is computed only from repos with a numeric `overall_score` (a tool
selection that never scores, e.g. `--only bart`, legitimately yields
`graded_count: 0` — that mirrors `overall.score: null` on a single-project
report when nothing graded, not a cohort-specific failure). A repo discovered
on GitHub but absent from the local `--tools-dir` convention is reported
`status: "not_locally_available"`, distinct from a repo whose analysis ran
and failed.

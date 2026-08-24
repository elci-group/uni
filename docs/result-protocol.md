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

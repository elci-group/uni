# The `uni.report/v2` result protocol

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
only project-health scores. `overall.provisional` is true when an unavailable
tool or execution/protocol error prevents a complete snapshot.

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

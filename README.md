# Uni

Uni runs a deterministic, concurrent analysis suite against one project and
normalizes each application's output into a single graded report.

The default suite is Amber, Ami, Bart, Chakra, Ferret hunt, Fract, Isopod,
Lwoodz, Tempcheq, Traci, and Vamos. Jeenome is opt-in because it requires an
`strace` trace. Ferret hunt prefers Ferret's `hunt` operation and supports
builds that call it `track`; it uses an in-memory corpus, so the analysis does
not write `ferret.db` into the target.

## Usage

```bash
cargo run -- /path/to/project
cargo run -- --json --only ferret,tempcheq /path/to/project
cargo run -- analyze --fail-under 80 /path/to/project
cargo run -- analyze --install-missing /path/to/project
cargo run -- revise /path/to/project
cargo run -- revise --apply /path/to/project
```

Use `--skip` to exclude applications and `--tools-dir` to choose where sibling
application repositories live.

`uni revise` initializes Vamos with `vamos init` when the target has no
`vamos.toml`, then includes Vamos in its diagnostic pass. Lwoodz analysis uses
`lwoodz --json audit`; a flagged missing license is previewed and repaired with
`lwoodz remedy` (`--apply` is still required for the repair itself).

## Missing applications

Missing applications are classified as known/installable without mutating the
filesystem. Pass `--install-missing` to opt into installation. Uni then clones
the known repository if necessary, requires Baby to validate a versioned
installation recipe before executing it, serializes installations, preserves
complete logs under `<tools-dir>/.uni/install-logs/`, and verifies the expected
binary. A failed recipe, build, or verification is reported as tool
availability—not as project health.

In an interactive terminal, clone and install stages show a spinner. In CI or
redirected output, Uni prints stable start lines instead. Each stage also emits
transparent telemetry to stderr containing only the application name, stage,
outcome, exit code, and elapsed milliseconds. This telemetry is local output:
Uni does not persist or transmit it, and JSON report data on stdout stays clean.

## Result semantics

The `uni.report/v2` JSON contract separates availability, execution, evidence
coverage/confidence/observation count, and analytical score. The human report
likewise presents suite execution health separately from project health.
In an interactive terminal, a successful Fract detail section uses Fract's
deterministic wave reveal. CI, redirected output, `TERM=dumb`, and JSON output
remain static, so machine-readable and captured reports are byte-stable.
Not-applicable and zero-observation results are ungraded; unknown Isopod
controls reduce coverage rather than counting as failures, and compliance is
not graded below 80% assessment coverage. See
[`docs/result-protocol.md`](docs/result-protocol.md).

## Ferret scoring

Ferret findings reduce its 100-point score by severity: 25 points for a
critical finding, 15 for major, 5 for minor, and 1 for informational. Critical
or major findings fail the Ferret check; lower-severity findings warn.

## Development

```bash
cargo fmt -- --check
cargo test
```

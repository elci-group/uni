# Uni

Uni runs a deterministic, concurrent analysis suite against one project and
normalizes each application's output into a single graded report.

The default suite is Amber, Bart, Chakra, Ferret hunt, Fract, Isopod, Lwoodz,
Scrawny, Tempcheq, Traci, Vamos, Viva Palestina, and Wilder. AMI is opt-in
via `--only ami`:
project-profile completeness is not code health, and Uni requires a JSON-capable
AMI rather than scraping its human table. Jeenome is opt-in because it requires
an `strace` trace. Ferret hunt prefers Ferret's `hunt` operation and supports
builds that call it `track`; it uses an in-memory corpus, so the analysis does
not write `ferret.db` into the target. Wilder exits 2 while its analysis
remains incomplete at the 0.3 milestone; Uni parses its JSON anyway and notes
the coverage gap rather than failing the tool. Scrawny grades the review
load of the working-tree diff, so a clean checkout scores 100 by construction.

Each tool has a custom emoji badge, generated with
[xpressive](https://github.com/elci-group/xpressive)'s `.xpr` vector format —
see [`docs/tool-badges.md`](docs/tool-badges.md). The terminal report itself
still uses plain Unicode emoji; the badges are documentation-only.

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

Pass `-v`/`-vv`/`-vvv` (or set `RUST_LOG`, e.g. `RUST_LOG=uni=debug`) to see
diagnostic logging on stderr: the exact command run per tool, full stderr on
failure, and why a tool was judged incompatible. This never mixes into
`--json`/`--out` output.

## Cohort mode

`--cohort` runs the same per-project suite across every first-party
repository instead of one target:

```bash
cargo run -- --cohort --cohort-out ./cohort-results
cargo run -- --cohort --cohort-org elci-group --cohort-batch-size 4 --cohort-cycle-seconds 30
```

Discovery calls `gh repo list <org> --json name,isFork,isArchived` (default
org `elci-group`) — the authoritative first-party list, since fork status
isn't reliably derivable from local git metadata and a filesystem walk
would also sweep in a large account's dormant/legacy repos. Each surviving
repo resolves to `<tools-dir>/<repo>` (the same sibling-checkout convention
`--tools-dir` already uses for Uni's own applications); a repo with no local
checkout there is reported `not_locally_available`, not silently skipped.

Repos run `--cohort-batch-size` at a time (default 4), pausing
`--cohort-cycle-seconds` between batches (default 30) — deliberately paced
rather than firing every repo's full tool suite at once. Every repo's full
`uni.report/v3` JSON is written to `--cohort-out` as `<repo>.json`, plus one
`summary.json` rollup; the human and `--json` output is a compact one-line-
per-repo table, not the full per-tool detail a single-project run prints.

`uni revise` initializes Vamos with `vamos init` when the target has no
`vamos.toml`, then includes Vamos in its diagnostic pass. Lwoodz analysis uses
the current `lwoodz --json audit` contract or the probed legacy equivalent; a flagged missing license is previewed and repaired with
`lwoodz remedy` (`--apply` is still required for the repair itself).

## Missing applications

Missing applications are classified as known/installable without mutating the
filesystem. Pass `--install-missing` to opt into installation. Uni then clones
the known repository if necessary, requires Baby to validate a versioned
installation recipe before executing it, serializes installations, preserves
complete logs under `<tools-dir>/.uni/install-logs/`, and verifies the expected
binary. A failed recipe, build, or verification is reported as tool
availability—not as project health.

Before running Lwoodz or Traci, Uni checks for their Poka-managed target inputs
(`lwoodz.toml` and `traci.toml`). If either is missing, Uni initializes or
extends the target's `poka.toml`, runs `poka apply`, verifies that Poka created
the file, and only then starts the assessment tools. Existing inputs are left
untouched. If Poka is unavailable or cannot materialize an input, Uni still
runs the underlying analyzer with its built-in defaults and records the Poka
failure in that tool's report note.

In an interactive terminal, clone and install stages show a
[`form3`](https://github.com/elci-group/3form) braille spinner. In CI or
redirected output, Uni prints stable start lines instead. Each stage also emits
transparent telemetry to stderr containing only the application name, stage,
outcome, exit code, and elapsed milliseconds. This telemetry is local output:
Uni does not persist or transmit it, and JSON report data on stdout stays clean.

## Result semantics

The `uni.report/v3` JSON contract separates availability, execution, evidence
coverage/confidence/observation count, and analytical score. The human report
likewise presents analysis integrity and its defects separately from project
health. A project grade is provisional whenever analysis integrity is degraded.
In an interactive terminal, a successful Fract detail section uses Fract's
deterministic wave reveal. CI, redirected output, `TERM=dumb`, and JSON output
remain static, so machine-readable and captured reports are byte-stable.
Interactive human reports also preserve each source tool's native terminal
palette instead of imposing a Uni-wide status scheme: for example Bart keeps
its blue/cyan depth colors, Chakra its mauve brand accent, Ferret its
cyan-plus-severity treatment, Fract its 256-color glass palette, and Traci its
cyan framing with native severity colors. Tools whose own human renderer is
plain, including Catskin, Lwoodz, and Vamos, remain uncolored. `NO_COLOR`
disables all ANSI styling.

All of Uni's own styling, tables, and terminal-capability detection run on
[`form3`](https://github.com/elci-group/3form) — the same dependency-free
ANSI/table/animation crate that Amber, Bart, Chakra, Fract, and Isopod's own
human renderers already use, so a native accent Uni relays (Chakra's mauve,
Fract's glass, ...) is drawn through the same primitives the source tool
draws it with, not a reimplementation. `--technical` output also names each
tool's own metaphorical analog next to its findings — Isopod's compliance
crawl, Fract's glass, Chakra's mystic aura, and so on for every tool `uni`
orchestrates — so the report speaks in each tool's own voice, not a
flattened Uni-wide vocabulary. See `ToolId::metaphor` in `src/tool.rs`.

Not-applicable and zero-observation results are ungraded; unknown Isopod
controls reduce coverage rather than counting as failures, and compliance is
not graded below 80% assessment coverage. A graded tool reporting `fail`
caps the overall score at 73 (the bottom of the "C" band) regardless of the
weighted average, so one critical-axis failure cannot be diluted away by
unrelated healthy scores. See
[`docs/result-protocol.md`](docs/result-protocol.md).

## Ferret scoring

Ferret findings reduce its 100-point score by severity: 20 points for a
critical finding, 8 for major, 1 for minor, and 0.25 for informational. Critical
or major findings fail the Ferret check; lower-severity findings warn.

Lwoodz treats missing licenses and incompatible dependencies as hard findings.
Advisory compatibility warnings are deducted proportionally to dependency
count, so a large dependency tree is not penalized once per attribution notice.

Viva Palestina flags excluded vendors as hard findings and review vendors as
warnings; the score scales with the fraction of dependencies affected, and
unknown vendors are noted but not penalized.

Wilder findings reduce its 100-point score by severity: 25 for critical, 10
for high, 4 for medium, 1 for low. Coverage gaps are reported in the summary
but not penalized — an analysis gap is not a failing result — and wilder's
exit code 2 (analysis incomplete) is noted, not treated as an error.

Scrawny's score is the complement of its normalized review-load index
(100 − review load). Status mirrors scrawny's own `check` policy: review load
above 70 or cohesion below 0.55 fails; load above 40 or more than 4 concern
types warns. A clean working tree scores 100 by construction, since scrawny
grades the diff, not the project.

Catskin is ungraded (`score: none`), like Bart: it proposes deterministic,
type-checked rewrites (loop -> iterator, filter-loop -> `filter().collect()`,
if/else chain -> match) via its `export` command and reports how many
verified equivalent rewrites it found. Having zero isn't a defect — it just
means the current rule set found nothing applicable — so this is
informational context, not a health signal. It's `NotApplicable` when no
source file could be lowered into catskin's process IR at all.

## Experiments

`uni experiments` evaluates candidate branches against an explicit baseline
(default: the repository's default branch). It isolates each revision in a git
worktree, runs the standard UNI analysis plus `cargo check`/`cargo test`, and
produces a differential verdict:

```bash
uni experiments                          # discover and analyze all candidates
uni experiments --list                   # list discovered candidates
uni experiments --branch dependabot/...  # analyze a specific branch
uni experiments --baseline main --branch feature/x
uni experiments --json                   # machine-readable report
```

The subsystem is read-only: it never merges, pushes, deletes branches, or
rewrites history. Experiment records are written to `.uni/experiments/<id>/`
for auditability. Verdicts include `SUPERIOR`, `LIKELY_SUPERIOR`, `EQUIVALENT`,
`UNCERTAIN`, `LIKELY_INFERIOR`, `INFERIOR`, and `BLOCKED`; correctness and
security failures are hard gates that block adoption regardless of aggregate
score.

Policy gates let `uni experiments` act as a CI check:

```bash
uni experiments --minimum-confidence 0.8 --minimum-improvement 2.0
```

Structured telemetry events (`experiment.discovered`, `experiment.started`,
`experiment.analysis_completed`, `experiment.verdict_produced`, etc.) are emitted
as JSON lines on stderr for observability pipelines.

When `gh` is available, `uni experiments` associates candidates with open pull
requests and includes CI status rollup in the report.

## Development

```bash
cargo fmt -- --check
cargo test
```

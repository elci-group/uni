---
name: uni
description: >-
  Use `uni` to run a deterministic, concurrent analysis snapshot of a project
  and get one graded report. It fans out to amber, ami, bart, chakra, ferret
  hunt, fract, isopod, lwoodz, tempcheq, traci, and vamos (jeenome is
  opt-in), normalizes each tool's findings, and grades the project overall.
  Reach for it whenever you need a single before/after quality snapshot, a
  CI gate on a numeric score, or a plan for fixing what a run flagged.
---

# uni

`uni` is the unified analysis snapshot tool for a project. It runs the
project's constituent analysis applications concurrently, normalizes their
output, and compiles one deterministic report with a per-tool grade and an
overall grade. It does not replace any single tool's own CLI — it is the
aggregator you reach for when you want the combined picture instead of
running each analyzer by hand.

## Quick decision flow

1. **Want a one-shot snapshot of a project?** `uni /path/to/project` (or
   `uni analyze /path/to/project` — identical).
2. **Need machine-readable output for CI or scripting?** `uni --json
   /path/to/project`, optionally with `--out report.json` to also persist it.
3. **Only care about specific tools?** `uni --only ferret,tempcheq
   /path/to/project`.
4. **Want a pass/fail gate?** `uni --fail-under 80 /path/to/project` — uni
   exits 1 if the overall score is below the threshold (uni exits 0 by
   default; it's a snapshot tool, not a gate, unless you ask it to be one).
5. **Something got flagged and you want it fixed?** `uni revise
   /path/to/project` (dry-run by default; add `--apply` to actually mutate
   the project). If `vamos.toml` is absent, revise first adopts Vamos by
   running `vamos init`; this initialization is the one mutation performed
   without `--apply`.

## When to call uni

Call uni when you need to:

- Get a single deterministic, graded snapshot of a project's health across
  the whole constituent tool suite, instead of running each tool separately.
- Gate a CI job or a pre-merge check on a combined numeric score
  (`--fail-under`).
- Compare two points in a project's history by diffing two JSON reports.
- Find out, in one pass, what a project's linked analysis tools would flag
  before considering a change complete.
- Let flagged tools attempt their own remediation (`uni revise`) rather than
  invoking each tool's fix command individually.

Do **not** call uni when:

- You already know exactly one tool's output is what you need — call that
  tool directly (e.g. `ferret hunt`, `tempcheq`) instead of paying for the
  full concurrent suite.
- You need jeenome's strace-based audit as your primary signal — pass
  `--jeenome` explicitly (it requires `GROQ_API_KEY` and either
  `--jeenome-trace` or a Cargo project with `strace` on `PATH`); it is
  opt-in and excluded by default.
- The project has no `vamos.toml` and you specifically need vamos results —
  uni will report it `Skipped`, not `Error`, which is expected, not a bug.
- You are about to run `uni revise --apply` in an unfamiliar repo without
  reviewing the dry-run plan first.

## CLI at a glance

```text
uni [OPTIONS] [TARGET]              # analyze TARGET (default: .)
uni analyze [OPTIONS] [TARGET]      # same as the bare form
uni revise [OPTIONS] [TARGET]       # diagnose, then run each tool's own fix

Common options (analyze / bare form):
  --json                 machine-readable report on stdout
  --out <FILE>           also write the JSON report to FILE
  --only <a,b,c>         run only these tools
  --skip <a,b,c>         skip these tools
  --jeenome               opt in to jeenome (needs GROQ_API_KEY + strace trace)
  --jeenome-trace <FILE>  use an existing strace log instead of generating one
  --timeout <SECS>        per-tool timeout (default: 300)
  --tools-dir <DIR>       root containing sibling tool checkouts
  --fail-under <SCORE>    exit 1 if the overall score is below this (0-100)

revise-only:
  --apply                 actually run each tool's fix command (default: dry-run)
```

## Agent workflows

### Pre-completion quality gate

Before declaring a change complete, run a snapshot and don't consider the
work done while it reports failures you haven't addressed or explained:

```bash
uni --fail-under 80 .
```

Inspect the report (add `--json` for structured output) rather than only the
exit code — a passing overall score can still hide a critical finding in one
tool.

### Targeted check after a specific kind of change

If you only touched licensing metadata or dependency headers, don't pay for
the full suite:

```bash
uni --only lwoodz,tempcheq .
```

### Fixing what a run flagged

```bash
uni revise .            # see the plan: which tools are flagged, what each would run
uni revise --apply .    # actually invoke amber --propose / isopod harden /
                         # lwoodz remedy / tempcheq --fix, etc.
```

Before diagnosis, revise runs `vamos init` when the target has no
`vamos.toml`, so the same report includes Vamos rather than skipping it.
Regular `uni` and `uni analyze` snapshots do not initialize Vamos.

Each underlying fix command keeps its own safety defaults (e.g. `amber
--propose` never touches source on its own, `isopod harden` needs its own
`--apply`, `tempcheq --fix` needs `--yes`) — `uni
revise --apply` only unlocks uni's own dry-run gate, not each tool's.

### Missing applications

If a selected tool isn't installed, uni clones
`https://github.com/elci-group/<application>.git` into the tools directory
and installs it with `baby --user`, then continues. If that fails, uni
records the tool as unavailable in the report and keeps going rather than
aborting the whole run.

## Ferret scoring

Ferret findings reduce its 100-point score by severity: 20 for critical, 8
for major, 1 for minor, 0.25 for informational. Critical or major findings fail
the ferret check; lower severities only warn.

## Anti-patterns

- **Treating a passing `--fail-under` exit code as sufficient** without
  reading which tools contributed warnings — the threshold is on the overall
  score, not a guarantee every tool passed cleanly.
- **Running `uni revise --apply` on an unfamiliar project** without first
  reading the dry-run plan (`uni revise` with no `--apply`).
- **Forcing `--jeenome` on** for a routine snapshot — it needs a trace and an
  API key, and isn't part of the default suite for a reason.
- **Re-running the full suite for a single-tool question** — use `--only`
  instead of paying for every constituent tool's runtime.

## Safety

- uni is a snapshot tool by default: it exits 0 regardless of findings
  unless you pass `--fail-under`. `uni revise` also initializes a missing
  `vamos.toml`; regular analysis remains non-mutating.
- Ferret runs against an in-memory corpus, so uni's snapshot does not write
  `ferret.db` into the target project.
- `uni revise` only plans by default; it needs `--apply` to run any
  underlying fix command, and each fix command keeps its own apply gate on
  top of that.
- Telemetry for clone/install stages goes to stderr only (tool name, stage,
  outcome, exit code, elapsed ms) — uni does not persist or transmit it, and
  stdout JSON stays clean.

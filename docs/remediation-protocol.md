# The `uni.remediate/v1` protocol

`uni revise` fixes flagged findings by handing them to the tool that raised
them. Historically that meant one hardcoded invocation per tool, wired into
`uni`'s own source and shipped on `uni`'s own release cycle — adding a fifth
remediable tool meant a new match arm in `uni`, not a change in that tool.

This document specifies an optional contract any elci-group tool can
implement instead: a `remediate` subcommand that self-describes what it can
fix. A tool that implements it needs no code change in `uni` to be
discovered — `uni revise` finds it by probing for the subcommand at
runtime, the same way it already probes for ferret's `hunt`/`track` split
and checks that a remediation flag still exists before trusting it (see
`src/revise.rs`).

Tools that don't implement this fall back to `uni`'s existing hardcoded
per-tool table (`amber --propose`, `isopod harden --apply`, `lwoodz
remedy`, `tempcheq --fix --yes`) exactly as before — this protocol is
additive, not a breaking change to any existing integration.

## Discovery

`uni` probes:

```
<tool> remediate --help
```

Exit code `0` means the tool implements this protocol. Anything else
(nonzero exit, unrecognized-subcommand error, timeout) means it doesn't,
and `uni` falls back to its hardcoded entry for that tool, if any.

## Plan

```
<tool> remediate --format plan --json --base <path>
```

Must be side-effect-free — this call is made even in `uni revise` dry-run
mode, with no `--apply` anywhere in sight. Stdout must be exactly one JSON
object:

```json
{
  "protocol": "uni.remediate/v1",
  "items": [
    {
      "id": "missing-license",
      "summary": "LICENSE file is missing",
      "risk": "new_files_only"
    },
    {
      "id": "header-coverage",
      "summary": "12/20 tracked files are missing an SPDX header",
      "risk": "rewrites_source"
    }
  ]
}
```

- `protocol` — must be exactly `"uni.remediate/v1"`. `uni` rejects (and
  falls back to its hardcoded entry, if any) any other value, including a
  future `uni.remediate/v2` it doesn't understand yet — this is how the
  protocol versions without breaking older `uni` builds against newer
  tools, or newer `uni` builds against tools that haven't caught up.
- `items` — zero or more independently addressable findings. Zero items is
  a valid, meaningful response ("nothing here is currently addressable"),
  distinct from not implementing the protocol at all. Each item is a
  **separate** remediation as far as `uni` is concerned: separately risk
  gated, separately checkpointed, separately rolled back on failure,
  separately re-verified afterward. This is the protocol's answer to a
  tool like `lwoodz` today, where "the remediation command" is really one
  command bundling several only-partially-related fixes with no way for
  `uni` (or the operator) to select or reason about them individually.
- `id` — stable within one `plan` response; passed back verbatim to
  `apply`. Not required to be stable across runs or tool versions.
- `summary` — one line, human-readable, shown to the operator as-is in
  place of a preview. Should say what's wrong and what would change, not
  just restate the id.
- `risk` — `"new_files_only"` if this item only ever creates new files (or
  writes to a tool-owned output directory) and never rewrites a file
  already tracked in the target, `"rewrites_source"` if it rewrites
  existing tracked files. `uni` requires `--confirm-source-rewrite` in
  addition to `--apply` before running a `rewrites_source` item; a
  `new_files_only` item only needs `--apply`. Get this wrong in the unsafe
  direction (claiming `new_files_only` for something that rewrites source)
  and the operator loses a safety gate `uni` would otherwise have enforced
  for them — classify conservatively.

Any other shape (missing fields, wrong types, unparseable JSON, wrong
`protocol` value) is treated identically to "doesn't implement the
protocol": `uni` logs it and falls back.

## Apply

```
<tool> remediate --format apply --item <id> --json --base <path>
```

Runs exactly the one item named by `--item`, previously returned from
`plan`. Exit `0` means success; `uni` treats stdout as a human-readable
detail message (truncated) and does not require it to be JSON. Any nonzero
exit is a failure — `uni` rolls back whatever partial mutation this made
(`git checkout -- .` + `git clean -fd`, scoped to the target directory)
before moving on to the next item.

Idempotence is the tool's responsibility: `uni` may re-run `plan` and
re-apply an item that was already applied in an earlier `uni revise`
invocation (e.g. because the operator ran it again). Applying an
already-fixed item again should be a safe no-op, not an error.

## What `uni` does around this, unconditionally

None of this is configurable per protocol implementer — it's `uni`'s own
safety envelope, identical for every remediation regardless of whether it
came from this protocol or the hardcoded fallback table:

- `--apply` requires a clean git worktree, scoped to the target directory
  even when the target is a subdirectory of a larger repository.
- Every successful apply is immediately committed as its own checkpoint.
- Every failed apply is rolled back to that checkpoint before continuing.
- Every apply is re-diagnosed afterward (`uni analyze --only <tool>`) and
  compared against its pre-apply grade, so "applied" means the tool's own
  score actually moved the way the item claimed it would.

A tool implementing this protocol gets all of that for free; it only needs
to answer "what could I fix" and "fix this one specific thing," honestly.

## Reference implementation

`uni`'s own test suite (`src/revise.rs`, the `native protocol` tests)
includes a minimal shell-script implementation of this contract used to
prove the discovery/plan/apply path end-to-end without depending on any
real tool having adopted it yet.

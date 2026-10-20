# AGENTS.md

This file is managed by poka. Edits will be overwritten on the next `poka apply`
unless the corresponding rule is disabled in poka.toml.

Project: uni

## Policies

<!-- poka:block:policy:security -->
- Never commit secrets, API keys, or credentials. Use environment variables or a secret manager.
<!-- /poka:block:policy:security -->
<!-- poka:block:policy:testing -->
- Run the test suite before considering a change complete.
<!-- /poka:block:policy:testing -->
<!-- poka:block:policy:documentation -->
- Keep documentation in sync with behavior changes; do not let it drift.
<!-- /poka:block:policy:documentation -->
- Run `uni` for a graded analysis snapshot before considering a change complete.

## Analysis

Use `uni`, this project's own deterministic multi-tool analysis snapshot, to
check a project (including this one) before calling a change complete:

```
uni --fail-under 80 .
```

See `.skillastic/skills/uni.md` (`skillastic show uni`) for the full skill:
when to reach for `uni` vs. a single constituent tool, `--only`/`--skip`,
`--json`/`--out`, and `uni revise` for remediation.

## TempCheq

- Submit every artifact containing LLM inference calls to tempcheq before delivery: run `tempcheq <path>` on the workspace that produced the artifact, review the temperature audit, and include the findings with the artifact (`tempcheq --report` for JSON output, `tempcheq report <path>` for Markdown/HTML report files).

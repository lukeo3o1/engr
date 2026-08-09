# Engr behavioral evaluations

`evals.json` is the frozen S01–S16 behavioral contract for an agent using
`skill/SKILL.md` with the native `engr` CLI. `trigger-evals.json` records the
positive and negative skill-trigger corpus. The v3 log is the evidence
attachment used by S07.

The Rust `behavior_contract` integration test proves that these inputs are
valid JSON, complete, and free from legacy project-directory and interpreter
fallback wording. A real agent evaluation must additionally save a
commit-bound result that identifies the agent runtime and records one outcome
per S01–S16; the static test is not a substitute for that runtime evidence.

## Captured runtime evidence

Commit-bound runtime reports live under `results/<evaluated-commit>/`. The
[S01–S16 report for `1e0f351`](results/1e0f351e38731d40f008bc3116b62fa391041f85/codex-runtime-s01-s16.md)
records real native-CLI execution by Claude Code (`claude-opus-5`) on Linux
x86_64 and a native Windows x64 released binary, with all 64 case/check
outcomes passing. Its historical filename is not the runtime identity; the
report header is authoritative.

A report proves only the exact commit, runtime, platforms, and commands it
names. Any future semantic-runtime change requires fresh evaluation evidence;
a static corpus check or a report for a different commit is not a substitute.

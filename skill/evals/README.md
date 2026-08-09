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

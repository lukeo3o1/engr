# Engr Agent Constitution

## Purpose

Engr is designed to mitigate engineering-context drift across long-running
human-AI engineering work, including repeatedly revised ADRs, specifications,
design documents, summaries, handoff material, and reports. Keep the
implementation subordinate to that purpose. Do not turn Engr into a
mutable-document editor, a free-form summary system, or an ad-hoc
event-sourcing example.

## Non-Negotiable Model

EventStore is the authoritative semantic history. State is deterministic,
derived State. A Snapshot is a replay checkpoint, not authority. Artifacts are
evidence. Outputs are derived communication surfaces. State must never be
semantically patched, and accepted EventStore history must never be rewritten
to make present understanding look cleaner.

Corrections use explicit semantic transitions. Preserve these distinctions:

```text
inference != fact
proposal != accepted decision
implementation completed != verification passed
verification passed != automatic resolution
```

Retained history is part of correctness. Inactive, invalidated, rejected,
superseded, and resolved entities remain available with explicit status. A
derived view may filter them; it must not make them disappear from State.
Semantic forks fail closed and require explicit reconciliation. Never sort or
choose a branch by time, revision, filesystem order, Git order, or identifier.

## Human Authority

Human-authoritative semantic change follows this invariant:

```text
exact candidate -> fresh challenge -> exact CONFIRM <code> -> exact confirmed wording appended
```

The gate protects requirements, constraints, selected direction, accepted risk,
priority, and durable decision acceptance, supersession, or revocation. It does
not apply to every event. Agent-originated observations, hypotheses,
implementation progress, and verification results may be recorded when their
certainty and provenance are accurate. Do not mistake ordinary clarification
for confirmation, and never hide human meaning in structured fields.

## Source Authority

When sources disagree, use this order: explicit human-settled direction;
protocol and reducer semantics; schemas and conformance oracles; current
replayed State in an adopted project; then repository guidance and generated
views. Tests and examples are evidence, not permission to change a higher
authority. A genesis design may clarify the original problem and enduring
semantic objectives when it does not conflict with a higher authority; it must
not restore an implementation choice that later human-settled direction has
superseded. Do not infer a semantic rule from implementation convenience or a
document that is merely a derived output.

## Change Discipline

Keep one canonical repository, one canonical Skill, and one Rust production
implementation with many verified platform distributions. Do not introduce a
second editable Skill, another production writer, a compatibility alias, or an
ad-hoc replacement when tooling is unavailable.

Protocol-sensitive work requires matching evidence in the appropriate owner:
protocol or event types for normative rules, schemas for machine contracts,
conformance for fixed expectations, and tests for executable invariants. Keep
documentation responsibilities separate. Do not copy detailed algorithms, field
tables, exit codes, or command syntax into a general guide when their canonical
owner already exists.

## Git Workflow

Use short branches named `<type>/<short-kebab-description>` and Conventional
Commits. Common types are `feat`, `fix`, `docs`, `refactor`, `test`,
`chore`, and `release`. A semantic or protocol breaking change must say so
explicitly; never disguise it as a refactor, cleanup, chore, or routine
documentation edit. Detailed branch, review, CI, and release procedures belong
in `CONTRIBUTING.md`.

## Claims

Use evidence-bounded language. It is valid to say that Engr is designed to
mitigate engineering-context drift. Do not claim that it prevents drift,
eliminates hallucinations, guarantees correct decisions, has a published
release, or supports a platform without the corresponding direct evidence.
Configuration, a green local build, or a manifest template is not proof of a
published artifact or platform runtime support.

## Stop Conditions

Stop and request explicit human direction when canonical sources conflict; an
invariant would need to change; a protocol or architecture decision is needed;
or implementation convenience would weaken a semantic guarantee. Stop normal
record work on an unresolved fork, incompatible runtime, or replay/verification
failure. Do not work around those conditions by editing derived files or
writing raw EventStore data.

## Definition of Done

Before claiming a change is complete, confirm the implementation, protocol,
schemas, documentation, and relevant tests agree. Run the focused checks that
cover the changed invariant, and run the full required suite when scope or
project rules require it. Ensure generated or packaged outputs are not claimed
without direct artifact evidence. Review the diff for accidental authority,
compatibility, or wording changes. A passing narrow test cannot prove a broader
semantic claim.

## Where Details Belong

`README.md` owns purpose, motivation, architecture overview, and evidence
status. `skill/SKILL.md` owns agent behavior inside an adopted project.
`CONTRIBUTING.md` owns repository workflow. `protocol/` owns normative
semantics and the CLI contract; `schemas/` owns machine-readable contracts;
`conformance/` owns fixed oracles. Put extensive rationale, research, or
experiments in `docs/` when they are needed. This constitution remains short,
durable, and subordinate to the protocol.

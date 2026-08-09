---
name: engr
description: >-
  Use Engr in a repository that has adopted `.engr/` to keep long-running
  engineering context coherent. Read deterministic Engineering Record State,
  record semantic changes as append-only events with honest certainty and
  provenance, use exact human confirmation for human-authoritative changes,
  and treat ADRs, specifications, handoff material, reports, and other
  generated documents as derived views. Not for application event-sourcing
  architecture, EventStoreDB, Kafka or Elasticsearch work, ordinary logs,
  personal journals, private session checkpoints, or standalone ADR writing
  outside an adopted project.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# Engr

Use this Skill inside a project that has adopted `.engr/`, or when the user
explicitly asks to adopt an Engineering Record. It is the runtime guide for
working with a project's record, not a guide for changing the Engr repository.

## Working model

| Layer | Role |
| --- | --- |
| EventStore | append-only authoritative semantic history |
| State | deterministic derived registry of entities and statuses |
| Snapshot | replay checkpoint, never authority |
| Artifact | supporting evidence |
| Output | disposable generated communication surface |

State is a retained registry, not an active-only list. An invalidated fact, a
rejected solution, and a resolved unknown remain history with explicit status.
ADRs, specifications, handoff material, reports, and generated views never
override replayed State.

## Start safely

1. Find the nearest `.engr/` directory and read its `FORMAT.md`.
2. Use the project-local runtime described by `TOOLING.md`; run `engr doctor`
   before protocol work.
3. Read the active slice with `engr show <id> --brief`, or `engr backlog` when
   the relevant stream is not known.
4. Ask targeted provenance questions with `engr why` or `show --provenance`.
   Do not load all EventStore history, artifacts, snapshots, or rejected
   alternatives merely to begin work.

If no `.engr/` exists, do not adopt Engr implicitly. Initialize only when the
user explicitly asks for this durable record.

## Record semantic change

After doing engineering work, distinguish what changed in meaning from a
transient conversation or a document edit. Use the weakest accurate certainty:

- direct evidence may support a fact or verification result;
- reasoning without proof is a hypothesis;
- an open question is an unknown;
- a suggested direction is a proposal;
- implementation completion is not verification or resolution.

Append agent or system observations through Engr with explicit provenance and
the current parent. Inspect the affected stream after a successful write, replay
derived State, and verify before claiming the record is healthy.

## Human Alignment Gate

Use the gate when the human supplies durable meaning: requirements, constraints,
selected direction, priority, accepted risk, or acceptance, supersession, or
revocation of a Decision Record.

```text
exact candidate -> fresh challenge -> exact CONFIRM <code> -> exact wording appended
```

An acknowledgement, an incomplete response, or a response that qualifies the
candidate does not authorize the event. Keep the candidate pending when nothing
changed; retire it and prepare a new candidate when the human corrects it.
Never add hidden human meaning in structured fields or polish confirmed wording.

## Derived State, forks, and outputs

Never edit EventStore lines, State, snapshots, manifests, or outputs to make
meaning persist. Correct understanding through a new semantic event, then
replay and regenerate views.

If replay stops at a fork, stop normal work. Do not choose by timestamp,
revision, file order, Git order, or identifier. Reconciliation is explicit and
retains rejected history as non-canonical.

Feedback on a generated document that changes engineering meaning follows the
same path: classify it, use the gate when human authority is involved, record
the event, then regenerate the document from State.

## Tooling failures and references

Do not replace a failing Engr runtime, hand-assemble an EventStore append, or
fall back to another writer. Run `engr doctor`, `engr verify`, and `engr
conformance` as appropriate, then report the tooling defect. If replay or
verification fails, do not present persisted State as current truth.

Read these project-local documents for details rather than reconstructing them:

- `.engr/PROTOCOL.md` for the CLI contract, replay, forks, confirmations, and
  exit behavior;
- `.engr/event-types.md` before selecting an event or structured fields;
- `.engr/TOOLING.md` for the selected runtime and compatibility handshake;
- `.engr/conformance.md` before changing protocol, reducer, schema, lifecycle,
  snapshot, or verification behavior.

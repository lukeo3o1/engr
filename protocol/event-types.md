# Event Types And Reducer Effects

Use this reference to choose a semantic event and construct its `data` object. Protocol v1 rejects unknown event types and unknown data fields; that keeps replay deterministic and prevents hidden human meaning from being smuggled into an open-ended object.

## Common Rules

Every event has the envelope defined in the normative protocol document. `record.text` is a non-empty string and is copied byte-for-byte at the Unicode text level into derived State. Structured identifiers use non-empty, case-sensitive strings up to 64 characters unless a stricter format is named below.

Fields listed as optional may be omitted. Do not send them as `null` unless the field explicitly allows it. Artifact references are repository-relative paths or stable external identifiers; they are references, not proof by themselves.

State is a registry of every entity introduced in the stream together with its current status. No effect below ever deletes an entry: words like *remove*, *clear*, and *retire* mean a status transition on a retained entry. That is what lets a later event reference the entity by ID, lets the reducer separate an unknown ID from a legitimately inactive one, and lets an incremental replay base stay equivalent to a full replay. Agent Views filter by status; State does not.

The first event in a stream is exactly one of:

- `work_item.created` for `WI-YYYYMMDD-NN`
- `decision.created` for `DR-YYYYMMDD-NN-BADGE`

No other event may precede it. IDs introduced within a stream are never reused, including after invalidation or supersession.

## Work Item Events

### `work_item.created`

Data: `{}`

Effect: create Work Item State, copy `record.text` to `title`, and set status to `discovered`.

### `problem.revised`

Data:

```json
{"problem_id":"P1","supersedes":"P0"}
```

`problem_id` is required. `supersedes` is required when a current problem exists and must name it; it is omitted for the first problem.

Effect: set the current problem to the exact new record. Retire the superseded problem without deleting its history: the retired entry keeps its own `id`, `text`, and `introduced_by`, and takes `status`, `last_event_id`, `last_text`, and `provenance` from this revision — the event that retired it, exactly like any other status change.

### `impact.revised`

Data:

```json
{"impact_id":"IMPACT-2","supersedes":"IMPACT-1"}
```

Rules and effect match `problem.revised` for the current impact.

### `fact.added`

Data: `{"fact_id":"F1"}`

Effect: add an active fact.

### `fact.invalidated`

Data: `{"fact_id":"F1"}`

Effect: mark the referenced active fact invalidated. The invalidation record explains why; it does not rewrite the original text.

### `constraint.added`

Data: `{"constraint_id":"C1"}`

Effect: add an active constraint.

### `constraint.removed`

Data: `{"constraint_id":"C1"}`

Effect: retire the referenced active constraint explicitly.

### `unknown.added`

Data: `{"unknown_id":"U1"}`

Effect: add an unresolved unknown.

### `unknown.resolved`

Data: `{"unknown_id":"U1"}`

Effect: mark the referenced unknown resolved and preserve the resolution record.

### `hypothesis.added`

Data: `{"hypothesis_id":"H1"}`

Effect: add an active hypothesis. An inference remains a hypothesis until a separate evidence-backed event changes the engineering record.

### `hypothesis.invalidated`

Data: `{"hypothesis_id":"H1"}`

Effect: mark the referenced active hypothesis invalidated.

### `solution.proposed`

Data: `{"solution_id":"S1"}`

Effect: add a candidate solution with status `proposed`.

### `solution.selected`

Data: `{"solution_id":"S1"}`

Effect: select an existing proposed solution and set Work Item status to `solution_ready` unless the item is already implementing or verifying. Selecting another solution requires an explicit `solution.superseded` or `solution.rejected` transition for the current selection first.

### `solution.rejected`

Data: `{"solution_id":"S1"}`

Effect: mark an existing proposed solution rejected. A selected solution cannot be rejected until it has been superseded.

### `solution.superseded`

Data:

```json
{"solution_id":"S1","by_solution_id":"S2"}
```

`by_solution_id` is optional only when no replacement is known yet. When present, it must name an existing proposed solution.

Effect: mark the referenced solution superseded. If it was selected, clear the selection or select `by_solution_id` when provided. Every in-progress or completed implementation tied to the old selection becomes `superseded`, with its history and artifacts retained, and every current verification result returns to `pending` because it has not established the replacement. The Work Item phase is then derived from remaining non-superseded implementations: `implementing` when one is in progress, `verifying` when one is completed, otherwise `solution_ready` with a replacement or `investigating` without one. Superseding a merely proposed, non-selected solution does not alter implementations, verification, or phase.

### `implementation.started`

Data:

```json
{"implementation_id":"IM1","solution_id":"S1"}
```

`solution_id` is optional for work that does not implement a selected solution. When present, it must match the current selected solution.

Effect: add an in-progress implementation and set status to `implementing`.

### `implementation.completed`

Data:

```json
{"implementation_id":"IM1","solution_id":"S1","artifacts":["src/ha.go","PR #42"]}
```

`solution_id` and `artifacts` are optional. The implementation must already be in progress; a supplied solution must match both its start event and the current selection.

Effect: mark the implementation completed and set status to `verifying`. It does not create a verification result and cannot resolve the Work Item.

### `verification.criterion_added`

Data:

```json
{"verification_id":"V1","required":true}
```

`required` is required and Boolean.

Effect: add a verification criterion with status `pending`.

### `verification.result`

Data:

```json
{"verification_id":"V1","result":"passed","artifacts":[".engr/artifacts/test-42.txt"]}
```

`result` is one of `passed`, `failed`, or `inconclusive`. `artifacts` is optional.

Effect: replace the current result for the referenced criterion with this result and set status to `verifying`. A later result is a new event, never an edit to the old result.

### `verification.invalidated`

Data: `{"verification_id":"V1"}`

Effect: mark the current result invalidated and return the criterion to `pending`. A resolved Work Item must first receive `work_item.reopened`.

### `finding.raised`

Data: `{"finding_id":"FIND-1"}`

Effect: add an active finding.

### `finding.promoted`

Data: `{"finding_id":"FIND-1","work_item_id":"WI-20260808-02"}`

Effect: mark the finding promoted and link the new Work Item. The target must be a different Work Item and must exist by project verification time.

### `decision.linked`

Data: `{"decision_id":"DR-20260808-01-KEY-DIST"}`

Effect: add an active Decision Record link. Project verification requires the target stream to exist and be currently `accepted`. If that Decision Record is later superseded or revoked, explicitly unlink it and link the new accepted Decision Record when one exists; an old linked ID is never rendered as current by implication.

### `decision.unlinked`

Data: `{"decision_id":"DR-20260808-01-KEY-DIST"}`

Effect: set the link status to `unlinked`; the entry and the historical relationship both remain retrievable.

Links and relations are retained registry entries like any other: each carries its current status, the event that last moved it, that event's record text, and its provenance, and `show --provenance` renders them annotated in the same form as every other entry.

### `work_item.related`

Data:

```json
{"work_item_id":"WI-20260808-02","relation":"depends_on"}
```

`relation` is one of `relates_to`, `depends_on`, `blocks`, `duplicates`, or `parent_of`.

Effect: add the current relationship. The target cannot be the same Work Item and must exist by project verification time.

### `work_item.unrelated`

Data matches `work_item.related` and removes that exact active relationship. The entry stays in State as `removed`, with the provenance of whoever removed it; re-asserting the same target under a different relation creates a separate entry.

### `risk.added`

Data: `{"risk_id":"R1"}`

Effect: add an open residual risk.

### `risk.accepted`

Data: `{"risk_id":"R1"}`

Effect: mark the open risk accepted. Human risk acceptance normally requires the Human Alignment Gate.

### `risk.mitigated`

Data: `{"risk_id":"R1"}`

Effect: mark the open risk mitigated.

### `work_item.blocked`

Data: `{"blocker_id":"B1"}`

Effect: add an active blocker and set status to `blocked`.

### `work_item.unblocked`

Data: `{"blocker_id":"B1"}`

Effect: set the referenced blocker to `cleared`. When no blocker is still active, set status to `reopened`; a later lifecycle event establishes a more specific active phase.

### `work_item.deferred`

Data: `{}`

Effect: set status to `deferred`. This does not clear unknowns, blockers, risks, or verification.

### `work_item.resumed`

Data: `{}`

Effect: move a deferred Work Item to `reopened` without changing its semantic dimensions.

### `work_item.resolved`

Data: `{}`

Effect: set status to `resolved` only after the resolution gate passes: at least one required criterion exists and every required criterion currently passes, no blocker is active, every risk is accepted or mitigated, and every non-superseded implementation is completed and does not contradict the selected solution. Historical implementations retired by a solution pivot do not gate the current solution.

### `work_item.reopened`

Data: `{}`

Effect: move a resolved or cancelled Work Item to `reopened` while preserving all prior history and current dimensions. Use explicit invalidation events for facts, verification, or decisions that no longer hold.

### `work_item.cancelled`

Data: `{}`

Effect: set status to `cancelled`. Cancellation is explicit historical truth, not deletion.

## Stream Control Event

### `stream.fork_reconciled`

Allowed in either stream kind. Data:

```json
{
  "fork_parent":"<common parent event id>",
  "accepted_root":"<accepted direct child event id>",
  "rejected_roots":["<every other direct child event id>"]
}
```

Effect: no domain State field changes. Graph validation keeps the accepted linear branch, retains every rejected branch as non-canonical history, then advances State head through this marker. The record explains why that disposition is correct. See the normative protocol document for the graph rules and special preparation path required while normal replay is stopped.

## Decision Record Events

### `decision.created`

Data: `{}`

Effect: create Decision Record State, copy `record.text` to `topic`, and set status to `proposed`.

### `decision.revised`

Data: `{}`

Effect: replace the current proposed wording with the exact record. Accepted, superseded, or revoked decisions cannot be revised; create a new Decision Record instead.

### `decision.accepted`

Data: `{}`

Effect: set status to `accepted` and copy the exact record to `decision`. Durable acceptance normally requires the Human Alignment Gate.

### `decision.superseded`

Data: `{"by_decision_id":"DR-20261021-02-KEY-DIST"}`

Effect: move an accepted Decision Record to `superseded` and retain the replacement link. Before the event is admitted, the replacement must be a different, already-existing Decision Record whose current replayed status is `accepted`. This preflight makes self-links and supersession cycles unwriteable. A longer supersession chain follows each immutable direct link; its current terminal may later be explicitly revoked without resurrecting an earlier decision.

### `decision.revoked`

Data: `{}`

Effect: move an accepted Decision Record to `revoked`. Revocation never rewrites the accepted historical wording.

## Provenance Compatibility

Recommended initiator and basis combinations are:

| Initiator | Allowed basis |
| --- | --- |
| `human` | `human_confirmation` only |
| `agent` | `observation`, `inference`, `agent_proposal`, `implementation`, `verification`, `dependency_change` |
| `system` | `observation`, `verification`, `dependency_change` |

The event type, not provenance alone, determines certainty. For example, `hypothesis.added` with basis `inference` remains a hypothesis; `fact.added` needs direct evidence or confirmed human authority.

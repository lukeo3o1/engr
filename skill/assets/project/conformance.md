# Protocol v1 Conformance

Run this suite whenever the implementation, schema, reducer, lifecycle rule, confirmation flow, snapshot format, or renderer changes. Engr runs the immutable JSON fixtures under `.engr/conformance/` and returns semantically identical parsed JSON, even when insignificant JSON formatting differs. Every successful fixture carries a fixed full-State integrity oracle and a fixed brief-view oracle in addition to its expected exit, head, and status.

`engr conformance` is the sole production conformance runner. It runs every bundled protocol-v1 fixture through the native implementation selected by `TOOLING.md` and `engr doctor`; a release must also pass the Rust test suite, which covers confirmation, snapshots, locking, and mutation rejection.

Fixtures cover replay, State, and the rendered view. Flows a static event fixture cannot express (confirmation, discard, snapshots, derived-file tampering, and invalid replay bases) are covered by Rust integration tests.

## Incremental Replay Equivalence

Every fixture that replays successfully is also run a second time to prove that resuming from a base lands on the same immutable oracle. The runner writes the command stream in two halves so the resumed fold has a real tail:

```text
write events[0..n/2)  ->  replay  ->  snapshot   (base at the halfway head)
append events[n/2..n)
show --format json                              (State base + tail)
delete the State file
show --format json                              (snapshot base + tail)
```

Both reads must reproduce the fixture's `head` and `status`, and their State is judged exactly the way the full path's is: the runner recomputes the digest over the State it actually received and compares the whole parsed object to the State full replay produced. Reading back the integrity value the implementation just wrote would only prove it can repeat itself — a resumed fold that dropped a tail event and then hashed its own wrong result would report a matching-looking digest and pass. This is the machine-checkable form of `full == State + tail == snapshot + tail`. A half that is not independently replayable — an unreconciled fork falls inside it — is skipped rather than reported, because it says nothing about incremental replay.

The complementary negative cases, a non-ancestral State losing to an older valid snapshot and a base that is ahead by revision but off this chain, live in the per-implementation suites because they need forged derived files rather than EventStore input.

## Comparison Rules

Compare:

- command exit category;
- canonical chain head;
- complete parsed State after removing `integrity.value` only when independently recomputing that value;
- State integrity recomputed by the runner over the State it actually received, not the digest the implementation reported — comparing an implementation's own field to the fixture only proves it can copy a constant;
- manifest head, `state_path`, and `state_integrity`, checked against both the State they point at and the immutable oracle;
- `rejected_event_ids`, read back through `why --format json` so a fixture's branch-disposition oracle is asserted rather than stored and ignored;
- emitted event semantics and exact `record.text`;
- default brief-view active content;
- any `expected.provenance_view` oracle, the exact `show --provenance` text, so the inspection surface for retained entries is pinned as a contract rather than left to the digest — a State digest cannot see an implementation that renders a superseded entry differently or omits a whole entry class from the view;
- any `expected.state_subset` oracle, a partial State object matched key-wise with arrays aligned exactly, used when a fixture needs to name the State property it pins instead of hiding it inside the integrity hash;
- confirmation and snapshot artifacts named by the protocol.

Do not compare filesystem modification times, temporary filenames, property order, implementation-specific diagnostics, or randomly allocated IDs across runs. Fixtures that need exact event IDs and times provide them as immutable input; normal writers still allocate their own.

## Shipped Fixtures

```text
01-linear-resolution          C1   full lifecycle through resolution
02-long-preservation          T1   collection growth and deterministic replay at scale
03-unresolved-fork            C9   fork stops replay with exit 6
04-explicit-reconciliation    C10  reconciliation marker selects one branch
05-solution-pivot             C15  selected-solution pivot retires old evidence
06-problem-refinement         C2   P1 superseded by P2 with unrelated state intact
07-fact-invalidation          C3   invalidated fact leaves the active view
08-solution-selection         C4   rejected solution stays rejected when mentioned later
09-verification-gate          C5   resolution over a failed criterion exits 5
10-verification-invalidation  C6   reopen then invalidate returns the criterion to pending
11-decision-supersession      C7   old Decision Record becomes superseded, both streams intact
12-work-item-reopen           C8   reopen preserves the resolved history
13-unknown-resolution         C16  resolved unknown stays in State and leaves the view
14-long-horizon-drift         T7   61 events across every dimension: pivot, expiry, reopen, poisoning
15-hashless-human-confirmation C17 a v1 human event that omits the optional candidate hash
16-relation-provenance         C18  a relation asserted, removed, and re-asserted, with provenance
```

C11 through C14 are behavioral flows rather than replay inputs and live in the per-implementation test suites.

## Core Fixtures

### C1 — Basic Work Item

Create a Work Item, problem, fact, unknown, proposed and selected solution, implementation, verification criterion/result, and resolution.

Expected: exact head, all active dimensions preserved, required verification passed, and status `resolved`.

### C2 — Problem Refinement

Revise `P1` to `P2` with `supersedes: P1` while facts and constraints remain active. The revision is human-confirmed and the introduction was an agent observation, so the two provenances differ.

Expected: current problem is exact `P2` text; unrelated dimensions are byte-for-byte semantically unchanged; missing or wrong `supersedes` exits `5`. `P1` remains in the registry under `retired.problems` as a full record with status `superseded`, absent from the default view and readable through `show --provenance` — the singleton problem and impact are not an exception to the retained-registry rule.

The fixture also pins *which event* retired it. `P1` keeps its own `id`, `text`, and `introduced_by`, and its `status`, `last_event_id`, `last_text`, and `provenance` come from the `P2` revision. Pinning only status and text would leave an entry that says `superseded` while reporting the event that created it as the latest transition — a retired-by-a-human problem claiming an agent observation retired it. That is the shape of defect a fixture generated from an implementation's own output cannot catch, so this oracle was derived from the protocol rule first and the digest taken afterwards.

### C3 — Fact Invalidation

Invalidate an active fact.

Expected: it is explicitly `invalidated`, absent from the brief active-facts view, present in targeted provenance, and cannot be invalidated or reintroduced with the same ID again. The fixture states each half of that as a reviewable oracle rather than leaving it to the full-State digest: a `state_subset` pinning the retained entry — original text and `introduced_by` unmoved, `status`, `last_event_id`, `last_text`, and provenance taken from the invalidating event — and a `provenance_view` proving the entry is still readable after it leaves the default view.

### C4 — Solution Selection

Propose S1/S2, select S2, reject S1, then mention S1 again in an unrelated finding.

Expected: selected solution is S2. Mentioning S1 in a later unrelated record does not change its rejected status. The `state_subset` and `provenance_view` pin both entries whole, so the fixture also proves the mention did not advance S1's transition fields past the rejection that set them.

### C5 — Verification Gate

Complete implementation with one required verification failed.

Expected: implementation is completed, verification is failed, and `work_item.resolved` exits `5` without replacing valid prior State.

### C6 — Verification Invalidation And Reopen

Resolve after passing verification, reopen, invalidate the result, and rerun it.

Expected: history and old result remain traceable; current criterion returns to pending after invalidation; resolution is illegal until a new passing result exists. Invalidating before reopen exits `5`.

### C7 — Decision Supersession

Accept an old Decision Record, create/accept a new one, and supersede the old with `by_decision_id`.

Expected: both streams remain immutable, old status is `superseded`, new status is `accepted`, and current views never present the old decision as active. A self-target, missing/non-accepted target, or attempt to close a supersession cycle is rejected before append.

### C8 — Work Item Reopen

Resolve, then append a production counterexample through `work_item.reopened` and a finding or fact event.

Expected: status `reopened`; old resolution, implementation, and verification remain history; no identity changes.

### C9 — Stream Fork

Place two different children under the same parent.

Expected: replay/verify exit `6`, timestamps do not choose a winner, and persisted State is not overwritten.

### C10 — Explicit Fork Reconciliation

Add a valid `stream.fork_reconciled` marker after one accepted branch and list every rejected root.

Expected: accepted branch replays, rejected branch remains retrievable as non-canonical history, and the marker mutates no domain dimension. Missing children, ambiguous accepted paths, or a second reconciliation exit `6`.

### C11 — Snapshot Replay

Create a snapshot, append later events, and replay from the snapshot.

Expected: result equals full replay. A changed nested State, incompatible version, non-canonical head, or wrong integrity makes the snapshot unusable; full replay remains available. Each implementation additionally proves the fallback end to end — corrupt the only snapshot, remove persisted State so nothing else can serve as a base, and require the read to succeed with the State full replay produces. Unusable is not fatal: `verify` reports the corruption, `show` routes around it, and the snapshot is left as it is rather than repaired.

### C12 — Human-Confirmed Record

Prepare a candidate containing non-ASCII text and structured data, confirm with the exact response, and replay.

Expected: event and State preserve exact Unicode text, confirmation provenance contains the candidate hash/challenge, and the accepted receipt matches the event.

### C13 — Confirmation Ambiguity

Try `OK`, `yes`, `對`, an emoji, `CONFIRM` alone, the code alone, a lower-case code, a wrong code, and surrounding whitespace.

Expected: exit `2` for every one of them, no EventStore bytes change, and the pending receipt remains pending. Both suites assert the exact code rather than merely a non-zero exit; a regression that started answering `3` or `5` would still look like a rejection while breaking the stable CLI contract.

Separately: a candidate whose stream head moved between `prepare` and `confirm` exits `5` in every implementation, and leaves the receipt pending. That is an admission precondition failing, not a fork, so exit `6` would be the wrong category.

### C14 — Confirmation Correction

Try `CONFIRM <code>, but change X`, discard the candidate, and prepare a revision.

Expected: no event for the old candidate, old receipt archived rejected, and a new random challenge bound to the revised payload.

### C15 — Selected Solution Pivot

Complete and verify S1, then supersede selected S1 with proposed S2 before implementing and verifying S2.

Expected: the S1 implementation remains in State as `superseded`, the old verification result returns to `pending`, phase becomes `solution_ready`, and a later completed/passed S2 can resolve normally. Superseding the selected solution without a replacement returns an otherwise idle Work Item to `investigating`.

### C16 — Unknown Resolution

Add two unknowns and resolve one.

Expected: the resolved unknown remains in State with status `resolved` and its resolution record; the other stays `unresolved`; only the unresolved one appears in the brief view; resolving it twice exits `5`.

This case exists to pin the registry model — State keeps every entity and its status, and only Agent Views filter — so a fixture asserts the State entry directly rather than inferring it from the view.

### C17 — Human Confirmation Without A Candidate Hash

Replay a stream carrying two human-confirmed events: one whose `provenance.confirmation` contains only `challenge`, and one that also carries a well-formed `candidate_sha256`.

Expected: both replay, and the hashless event's confirmed wording reaches State and the brief view unchanged. `candidate_sha256` is RECOMMENDED in v1, so an implementation that demanded it would reject conforming history written by another v1 writer.

The hash that is present must be the hash of that event's own sealed candidate. Validating this field means deriving it from the event's `stream`, `event`, `record`, `data`, and `parent`; accepting any well-formed 64-hex string would let an event claim provenance for a candidate nobody was ever shown, which is the one thing the field exists to make impossible. Every fixture hash is therefore a real digest, and both fixture 14 and fixture 15 would fail if it were not. The reject side — a well-formed hash of a different candidate, a malformed hash, and a missing `challenge` — is per-implementation because it needs deliberately broken input rather than a shared oracle.

### C18 — Relation Provenance

Replay a Work Item that asserts a `depends_on` relation on an agent inference, removes it on a human confirmation, then asserts a `relates_to` relation on the same target.

Expected: both relations are retained with their current status, deciding event, and provenance; the default view shows neither, because a Work Item's active slice is its own work; `show --provenance` renders both in the same annotated form as every other retained entry. The fixture pins that text through the `provenance_view` oracle, so an implementation that dropped provenance from links — or rendered them as a list of bare IDs — fails the native conformance gate rather than silently offering a narrower inspection surface.

## Runner Sensitivity

The Rust suite mutates fixture heads, State digests, rejected-event dispositions, provenance output, and retained-entry status. Each mutation must fail. It also rejects duplicate JSON keys and exercises the exact native handshake used by `engr doctor` and project-local launchers.

## Design Stress Tests

### T1 — Unrelated Update Preservation

Append an unrelated finding after a fully populated Work Item.

Assert all active facts, constraints, unknowns, selected solution, linked decisions, risks, and verification entries are unchanged.

### T2 — Problem Refinement

Covered by C2. Also assert Work Item and related entity IDs remain stable.

### T3 — Rejected Solution Resurrection

Covered by C4. Add obsolete S1 text to an artifact and unrelated record; no event semantics select S1.

### T4 — Verification Failure

Covered by C5.

### T5 — Reopen

Covered by C8.

### T6 — History Poisoning

Supply obsolete problem text, invalidated facts, rejected solutions, and failed old verification through targeted history input.

Assert State and brief view remain derived only from canonical reducer transitions; prose occurrence never changes current fields.

### T7 — Long-Turn Drift

`14-long-horizon-drift` replays 61 events on one Work Item, plus two Decision Records, and
exercises every major dimension rather than repeating one event type: problem and impact,
facts that stay active and a fact that is invalidated, a constraint, an unknown that is
resolved, a hypothesis that is invalidated, two solutions where the selected one is superseded
by the other, two implementations where the first is retired by that pivot, verification that
passes, is reset by the pivot, fails once, passes again, is invalidated after a reopen and
passes a third time, a blocker raised and cleared, a human-confirmed accepted risk, twenty
unrelated findings as drift pressure, a resolution followed by a reopen, and a Decision Record
that is unlinked and replaced after the original is superseded.

Its `state_subset` pins the inactive statuses directly — `F2` invalidated, `U1` resolved, `H1`
invalidated, `S1` superseded, `IM1` superseded, `B1` cleared, the old Decision Record unlinked —
so history poisoning fails the fixture rather than merely failing to appear in a view. `02-long-preservation`
remains as the collection-growth and deterministic-replay scale test it always was.

Assert preservation is 100%, invalid-state count is 0, and full replay equals cached replay.

### T8 — Concurrent Selection

Covered by C9/C10 with S2 and S3 selected from the same parent. No last-write-wins.

### T9 — DR Supersession

Covered by C7.

### T10 — Verification Expiry

Covered by C6; record the dependency/topology change that invalidated the result.

### T11 — Confirmation Ambiguity

Covered by C13.

### T12 — Confirmation Correction

Covered by C14.

### T13 — Exact Wording Preservation

Covered by C12. Compare the candidate receipt, event `record.text`, State entry text, and rendered view after JSON decode; all must match.

### T14 Native Conformance Replay

Run every immutable fixture through `engr conformance` and the named S01-S16 Rust tests.

Expected: the parsed semantic State and exit category match the immutable oracle. A release cannot claim protocol conformance unless this native gate passes.
### T15 — Snapshot Head Identity

Create snapshots with the same readable slug at two distinct valid heads whose UUIDv7 values share the same timestamp prefix.

Expected: both immutable files coexist because the filename contains the full de-hyphenated head ID; neither is mistaken for an overwrite of the other.

## Additional Adversarial Cases

- duplicate global event ID;
- malformed or partial JSONL line;
- filename/stream/date mismatch;
- wrong revision or missing parent;
- disconnected root, cycle, or unknown event type;
- unknown structured data field, including hidden claims such as `production_ready`;
- unsupported protocol/event/state schema version;
- modified State with stale integrity;
- manually merged State disagreeing with full replay;
- stale confirmation after head changes;
- duplicate or reused entity ID;
- missing linked Work Item or Decision Record;
- accepted confirmation receipt whose hash or event reference differs;
- old snapshot renamed or overwritten;
- concurrent snapshot writers targeting the same head and slug;
- concurrent or crash-interrupted bootstrap with a stale advisory lock file;
- secret-looking content warning without silent event mutation or deletion.

## Release Evidence

A protocol/tool release is ready only with:

```text
PASS protocol-v1
native conformance: <all immutable fixtures passed>
stress: preservation=100%, invalid_state_rate=0%
```


### Running the whole gate

Run the gate against the commit under test and drive the generated project-local binary:

```text
cargo test --workspace --all-targets
engr --root <work> init
<work>/.engr/tools/engr doctor
<work>/.engr/tools/engr conformance
<work>/.engr/tools/engr verify
```

The Rust suite also proves that `init` creates the audited project layout, that the generated binary answers the exact compatibility handshake, and that confirmation, mutation, snapshot, and derived-State integrity checks reject invalid data.

# Engineering Record Protocol v1

This is the normative protocol for bundled implementations. Keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** carry their usual standards meaning.

## Versions

Protocol v1 separates four versions:

```json
{
  "implementation":"rust",
  "implementation_version":"0.1.0",
  "protocol_version":1,
  "event_schema_version":1,
  "state_schema_version":1
}
```

An implementation MAY have its own release version. It is compatible only when it supports all three declared protocol/schema versions without changing command semantics or reducer results.

`FORMAT.md` declares exactly one `Protocol: N`, `Event schema: N`, and `State schema: N` line. Every project command except implementation-only `version` validates those declarations and exits `7` before reading or mutating project records when any declared version is unsupported.

## Repository Layout

The project surface is:

```text
.engr/
  FORMAT.md
  PROTOCOL.md
  TOOLING.md
  eventstore/YYYY/MM/DD/<stream>.jsonl
  snapshots/work-items/<WI-id>/*.json
  snapshots/decisions/<DR-id>/*.json
  artifacts/confirmations/{pending,accepted,rejected,transactions}/
  outputs/
  state/manifest.json
  state/work-items/<WI-id>.json
  state/decisions/<DR-id>.json
  tools/engr[.exe]
  conformance/
  schemas/
```

`.engr/eventstore/` is the only authoritative engineering history. Event files and snapshots are append-only/immutable. State, the manifest, and outputs are derived and atomically replaceable. Confirmation receipts are audit artifacts, not semantic truth.

## Identifiers

Stream IDs match exactly:

```text
Work Item:       ^WI-[0-9]{8}-[0-9]{2}$
Decision Record:^DR-[0-9]{8}-[0-9]{2}-[A-Z0-9]+(?:-[A-Z0-9]+)*$
```

The date is the stream creation date. `NN` is a two-digit human display sequence, not an ordering primitive. A Decision Record badge names the topic, never the chosen outcome.

Protocol v1 does not provide a distributed allocator for `NN`. Do not create new streams concurrently in independent worktrees on the same date. A colliding ID produces multiple genesis events for one stream and is a fatal fork that must be reconciled or re-recorded explicitly; it is never auto-renumbered after the fact.

Event IDs are UUIDv7 strings in lowercase canonical UUID form. Implementations MAY read legacy ULIDs, but bundled v1 writers emit UUIDv7. Event numbers are optional display identifiers of the form `E-YYYYMMDD-NNNN`; they never determine order or uniqueness.

Entity IDs inside a stream are case-sensitive strings matching `^[A-Z][A-Z0-9-]{0,63}$`. They are immutable and never reused.

## Event Envelope

Each physical JSONL line is one UTF-8-without-BOM JSON object followed by `\n`. Blank lines, duplicate object keys, floating-point numbers, `NaN`/infinities, a partial final line, and Unicode normalization are rejected:

```json
{
  "format":"engineering-event",
  "protocol_version":1,
  "event_schema_version":1,
  "event_id":"019c33fd-bd20-7a45-9d2f-c182e8602a95",
  "event_no":"E-20260808-0001",
  "time":"2026-08-08T02:30:00+08:00",
  "stream":"WI-20260808-01",
  "rev":1,
  "parent":null,
  "event":"work_item.created",
  "provenance":{
    "initiator":"agent",
    "basis":"observation"
  },
  "record":{
    "text":"HA failover can invalidate tokens issued before takeover."
  },
  "data":{}
}
```

Required fields are `format`, all three identity/version fields, `time`, `stream`, `rev`, `parent`, `event`, `provenance`, `record`, and `data`. `event_no` is optional. Unknown envelope, provenance, record, or event-data fields are rejected.

`protocol_version` and `event_schema_version` are the JSON integer `1`, and readers check the wire type before the value: `true`, `"1"`, `1.0`, and `1e0` are not this version, whatever a host language's `==` would say about them. The same rule governs every versioned document this protocol reads — State, Snapshot, State manifest, and confirmation receipt — so that the set of histories the schemas allow and the set each implementation accepts are the same set. A wrong-but-readable version is exit 7; a value the strict reader will not parse at all, such as a float, is exit 4.

`time` is RFC 3339 with an explicit numeric offset or `Z`. It records creation time but never orders a stream. `rev` is a positive integer convenience value. `parent` is `null` only for the first event; otherwise it is the expected previous canonical event ID.

`record` contains exactly `text`. JSON escaping is transport only: after parsing, the Unicode string MUST equal the candidate that was admitted. Implementations serialize JSON without ASCII-escaping Unicode unless the platform cannot do so losslessly.

## Provenance

`initiator` is `human`, `agent`, or `system`. `basis` compatibility is defined in `event-types.md`.

Human events add:

```json
{
  "initiator":"human",
  "basis":"human_confirmation",
  "confirmation":{
    "challenge":"7K4M9Q",
    "candidate_sha256":"<64 lowercase hex characters>"
  }
}
```

`challenge` is REQUIRED. `candidate_sha256` is RECOMMENDED: a writer SHOULD emit it, and every reader MUST validate it when it is present. An implementation that omits it still conforms to v1, so the hash never blocks a v1 conformance claim; all bundled implementations do emit it, and a future protocol version may promote it to REQUIRED.

The hash covers canonical UTF-8 JSON containing exactly `stream`, `event`, `record`, `data`, and `expected_parent`. It proves which sealed candidate and stream context were used; it is not a credential or proof that the statement is factually correct.

## Physical Partitioning

Writers place an event under the local calendar date encoded in its `time`:

```text
.engr/eventstore/<YYYY>/<MM>/<DD>/<stream>.jsonl
```

A stream may span many daily files. Readers recursively discover every file whose basename is the exact stream ID plus `.jsonl`; they do not assume one file or sort by path/time to derive semantic order. Full verification rejects every file below `eventstore/` that is not at the exact `YYYY/MM/DD/<valid-stream>.jsonl` depth and form instead of silently ignoring it.

Every event's `stream` MUST match the filename. Its calendar date MUST match the containing date path. Duplicate event IDs anywhere in the EventStore are invalid.

Writers validate the expected canonical head, build one complete JSON line, and append it with one operating-system append operation. All bundled implementations first acquire the shared project lock at `.engr/state/.write.lock`; narrower internal locks never replace that cross-implementation boundary. A partial trailing line is corruption, not an event to ignore.

## Canonical Chain And Forks

For a new stream:

- exactly one root exists;
- root `rev` is `1` and `parent` is `null`;
- its type matches the stream kind.

For each canonical non-root event:

- `parent` names an event in the same stream;
- `rev = parent.rev + 1`;
- the chain is acyclic;
- a normal parent has at most one canonical child.

Fork detection MUST consider every event in the stream: a competing child is a fork wherever its file sits. Bundled v1 implementations meet that by building the parent graph from a full scan of all discovered events. The full scan is a characteristic of these implementations, not a permanent protocol constraint — a later implementation MAY use an index or an incremental structure provided it detects exactly the same forks. File order, timestamp, revision alone, Git order, and lexical event-ID order never linearize semantic history.

Two or more children of the same parent form a fork. Replay returns exit code `6` until an explicit `stream.fork_reconciled` event exists.

### Explicit reconciliation

The reconciliation event has:

```json
{
  "event":"stream.fork_reconciled",
  "parent":"<accepted branch terminal head>",
  "data":{
    "fork_parent":"<common parent>",
    "accepted_root":"<one direct child of fork_parent>",
    "rejected_roots":["<every other direct child>"]
  }
}
```

Its record explains the semantic disposition. `accepted_root` must lead through one linear branch to the reconciliation event's `parent`. `rejected_roots` must list every other direct child exactly once. The marker itself must be the next child of the accepted terminal head. Only one valid reconciliation may resolve a given fork.

Graph validation then treats rejected roots and all their descendants as retained non-canonical history, replays the accepted branch, applies no domain-state mutation for the marker, and continues after it. Any missing branch, ambiguous accepted path, competing reconciliation, nested fork on the accepted path, or unlisted child remains exit code `6`.

Normal `append` and `confirm` refuse unresolved forks. A dedicated reconciliation preparation path may inspect the graph and create the marker. A human-selected disposition crosses the Human Alignment Gate; a mechanically provable Agent reconciliation uses accurate Agent provenance and cites its evidence.

## Human Alignment Receipts

`engr prepare` validates a proposed human event before it becomes truth. It captures the current canonical head, generates at least 24 bits of cryptographically secure random challenge entropy rendered as 6 uppercase alphanumeric characters, and writes:

```json
{
  "format":"engineering-confirmation-candidate",
  "protocol_version":1,
  "challenge":"7K4M9Q",
  "created_at":"2026-08-08T02:31:00+08:00",
  "stream":"WI-20260808-01",
  "expected_parent":"019c33fd-bd20-7a45-9d2f-c182e8602a95",
  "event":"problem.revised",
  "record":{"text":"..."},
  "data":{"problem_id":"P2","supersedes":"P1"},
  "candidate_sha256":"<hash>",
  "status":"pending"
}
```

Pending receipts live at `.engr/artifacts/confirmations/pending/<challenge>.json`. Preparing a new candidate for the same stream first archives every older pending receipt for that stream as rejected with reason `superseded_candidate`; this makes old codes unusable. A challenge is never reused after it appears in pending, accepted, rejected, or an in-flight archive transaction. "Reused" means allocated again: a challenge is minted for one candidate and belongs to it. Showing a still-pending challenge again for the candidate it was minted for is not reuse, and is what a generic non-confirmation calls for, since that leaves the receipt untouched.

The tool renders `record.text`, canonical `data`, the expected parent, and the exact response `CONFIRM <challenge>`. The Agent shows this output unchanged.

`engr confirm --response <text>` accepts only a string that exactly equals `CONFIRM <challenge>` with no leading/trailing whitespace or commentary. It verifies the candidate hash and unchanged stream head, allocates machine envelope fields, appends the event, and archives the receipt under `accepted/<event_id>.json`. If a process stops after the authoritative append but before receipt archival, repeating the same exact response detects that one matching event, archives the pending receipt, rebuilds current State, and succeeds without a duplicate append. A generic or wrong response writes nothing and leaves an identifiable pending receipt untouched. A response that identifies the correct code but adds a correction, qualification, or other text invalidates and archives that receipt as rejected.

A sealed candidate authorizes exactly one continuation: the head it was prepared against. If the stream head has moved since, `confirm` MUST refuse with exit `5` and leave the receipt pending — the human authorized wording against a stream state that no longer holds, which is an admission precondition failing, not a fork. Exit `6` stays for an EventStore that actually has two heads. Every implementation MUST report the same category here; a stable CLI contract means the same input produces the same semantic failure everywhere.

`engr discard <challenge>` archives the receipt under `rejected/<challenge>.json`. Corrections always discard the old receipt before a new candidate is shown. Receipt closure first atomically claims the pending file into `transactions/`, then publishes the prepared accepted/rejected receipt. Interrupted transactions are finished under the project confirmation lock before another confirmation operation; once claimed, a candidate can never become pending again.

## Deterministic Reducer

Event types, their allowed data, preconditions, and effects are normative in `event-types.md`. Reducers MUST NOT call an LLM, interpret free prose to choose structure, silently ignore an unknown event, or infer a transition from timestamps.

Reducer application is transactional for one stream: if any event fails schema, chain, transition, reference, or resolution-gate validation, no new State replaces the prior valid State.

The complete canonical chain can always rebuild State from zero.

### Replay base selection

Read commands (`show`, `backlog`) SHOULD resume from a base instead of folding the whole chain. Write commands (`append`, `confirm`) and `replay` MUST fold from the root, and `verify` MUST fold from the root as its oracle.

A base is chosen in this order, and only from candidates that pass every check below:

1. persisted State for the stream;
2. otherwise the usable snapshot closest to the current head;
3. otherwise no base — full replay.

Persisted State is usable only when its protocol, event-schema, and State-schema versions are compatible, its `format` and `stream` match, and its head event is **present on the current canonical chain**. A snapshot is usable only when it additionally passes snapshot and nested-State integrity and its `through` head matches its nested State head.

Chain membership is the ancestry test, and it is the whole point: the canonical chain is the single accepted path from root to head, so a base head found in it is provably an ancestor of the current head and provably not on a branch a reconciliation rejected. An implementation MUST NOT select or rank a base by revision number, timestamp, or file order. A State at rev 80 on an abandoned branch is unusable while a snapshot at rev 72 on this history is correct; "closest" means nearest along the chain, not highest `rev`.

Because `canonical_chain` refuses to return a chain while an unresolved fork exists, no base can be carried across one.

Incremental replay is an optimization and never an authority. For every stream:

```text
full(EventStore) == State + tail == snapshot + tail
```

MUST hold, and MUST be machine-checked rather than argued. `verify` enforces it per stream, and the conformance runner re-runs every successful fixture in two halves to exercise a real tail from both base kinds.

Failing closed still applies to corruption of persisted State: unreadable bytes or a broken integrity hash are an error, not a reason to silently fall back. An incompatible or non-ancestral State is discarded quietly instead, because falling back is then the correct answer.

A snapshot is different in kind and MUST NOT be held to that rule. State is the one document a stream keeps, so a reader that quietly ignored its broken bytes would hide a real fault; a snapshot is one optional checkpoint among however many exist, so failing any usability check — integrity included — only disqualifies that candidate. The reader moves on to the next usable snapshot, or to full replay, and reports nothing. A snapshot is never repaired to make a replay possible, because the history it was derived from is still authoritative and still complete.

When both could apply, compatibility is decided first. A reader MUST check `format`, `stream`, and the three declared versions before it validates integrity, and MUST discard an incompatible base quietly without judging its bytes. The declared versions are what say whether the document is this reader's to interpret at all, integrity representation included; holding a foreign schema to this schema's hash rule would turn another version's State into a hard error and break a project two tool versions share. Once a State claims this schema, its bytes MUST hold up: a compatible State with a broken hash fails closed.

## Materialized State

Every State document contains:

```json
{
  "format":"engineering-eventstore-state",
  "protocol_version":1,
  "event_schema_version":1,
  "state_schema_version":1,
  "stream":"WI-20260808-01",
  "kind":"work_item",
  "status":"verifying",
  "head":{"event_id":"...","rev":12},
  "integrity":{"algorithm":"sha256","value":"..."}
}
```

The integrity hash covers canonical UTF-8 JSON with the entire `integrity` member omitted. Canonical JSON sorts object keys, uses compact separators, preserves array order, and emits Unicode directly.

Record-bearing State entries retain:

```json
{
  "id":"F1",
  "status":"active",
  "text":"exact admitted record text",
  "introduced_by":"<event-id>",
  "last_event_id":"<event-id>",
  "last_text":"exact latest transition text",
  "provenance":{"initiator":"agent","basis":"observation"}
}
```

State is a **registry of every semantic entity in the stream with its current status**, not a set of active objects. Facts, constraints, unknowns, hypotheses, solutions, implementations, verification criteria, findings, risks, blockers, and decision links all stay in State once introduced; an entity leaves `active` only by moving to an explicit status such as `resolved`, `invalidated`, `removed`, `rejected`, or `superseded`. Agent Views, not State, do the filtering.

This is a correctness requirement, not a display preference. Later events reference entities by ID, so a reducer must be able to distinguish "never existed" (a reference error) from "exists and is resolved" (a legal precondition), and an incremental replay base must carry that distinction forward. The rule admits no exception for the singleton problem and impact: revising either moves the prior record, whole, into `retired.problems` or `retired.impacts` with status `superseded`, exactly as any other entity changes status. Retaining only the prior ID would make this the one collection where "retained" meant "we remember the name". Array order is introduction revision, then ID as a deterministic tie-breaker.

"Exactly as any other entity changes status" governs the whole entry, not only the status field. `id`, `text`, and `introduced_by` are identity and history and never move; `status`, `last_event_id`, `last_text`, and `provenance` describe the current transition and MUST come from the event that caused it — for a retired singleton, the revision that superseded it. Carrying the introduction's transition forward would make the entry say `superseded` while attributing that decision to the event that created it, so a problem retired by a human confirmation would report an agent observation as its current provenance.

### Work Item State

Work Item State additionally contains:

```text
title, problem, impact,
retired,
facts, constraints, unknowns, hypotheses,
solutions, selected_solution,
implementations, verification,
findings, risks, blockers,
decisions, related_work_items
```

`problem` and `impact` are either `null` or the current record entry. `retired` holds the prior singleton records as `problems` and `impacts`, each a full entry with status `superseded`, which `show --provenance` renders like any other retained entry. Collection entries use explicit status values. Verification entries also contain `required`, current `result`, and artifact references. Solution selection is an ID or `null`.

`decisions` and `related_work_items` are keyed by the linked ID — plus the relation, for a Work Item relation — rather than by an entity ID of their own, and they carry no `text` of their own. Otherwise they are ordinary retained entries: `status`, `introduced_by`, `last_event_id`, `last_text`, and `provenance`. Provenance is not optional here. `show --provenance` is the inspection surface for the whole registry, and an entry class that quietly lacked provenance would make that a rule with an exception a reader could only discover by being wrong — and whether a Work Item depends on another is a semantic assertion, so it matters whether a human confirmed it or an agent inferred it.

### Decision Record State

Decision State additionally contains:

```text
topic, decision, superseded_by
```

Status is `proposed`, `accepted`, `superseded`, or `revoked`.

### State manifest

`.engr/state/manifest.json` contains compatible versions plus a `stream_heads` object keyed by stream. Each entry records `event_id`, `rev`, `kind`, relative State path, and State integrity. Implementations rewrite it atomically after State files.

State files are:

```text
.engr/state/work-items/<WI-id>.json
.engr/state/decisions/<DR-id>.json
```

Manual State merge or semantic editing is invalid. Recompute it from EventStore.

State integrity detects accidental byte changes but is not a trust signature. It is checked before State is used as a replay base, so ordinary corruption and hand edits fail closed on the read path. Detecting a forgery whose integrity field was recomputed is `verify`'s job, not `show`'s: `verify` compares the entire State object to a fresh full replay, and recomputing the hash on forged State does not survive that. Run it before any claim that the engineering record is healthy.

## Resolution Gate

Before applying `work_item.resolved`, the reducer verifies:

1. at least one active criterion is marked `required: true`, and every such criterion has a current non-invalidated `passed` result;
2. no blocker has status `active`;
3. every risk has status `accepted` or `mitigated`;
4. every non-superseded implementation is completed;
5. each non-superseded implementation that names a solution matches `selected_solution`;
6. a non-superseded implementation that names a solution cannot exist without a current selection.

When the selected solution is superseded, its implementations deterministically become historical `superseded` entries and existing verification results return to `pending`; the same event supplies the explicit reason without deleting their history. Failure is an invariant violation (exit `5`). Implementation completion never synthesizes verification or resolution.

## Snapshots

A snapshot is immutable derived data:

```json
{
  "format":"engineering-eventstore-snapshot",
  "protocol_version":1,
  "event_schema_version":1,
  "state_schema_version":1,
  "filename":"ha-token-validation.WI-20260808-01.snap.019c33fdbd207a459d2fc182e8602a95.json",
  "stream":"WI-20260808-01",
  "through":{"event_id":"...","rev":19},
  "created_at":"2026-08-08T03:00:00+08:00",
  "state":{},
  "integrity":{"algorithm":"sha256","value":"..."}
}
```

The snapshot integrity hash omits its own `integrity` member. The nested State must have the same stream/head and a valid State integrity hash. `filename` is covered by snapshot integrity and MUST exactly equal the containing file's basename, so renaming is detectable.

Filenames are:

```text
<readable-slug>.<canonical-stream-id>.snap.<full-event-id-without-hyphens>.json
```

The head ID is embedded in full, not truncated. UUIDv7 values minted close together share a leading timestamp prefix, so a short prefix would let two snapshots at genuinely different heads collide on one filename and appear to overwrite each other, which existing snapshots must never do.

The readable slug absorbs the length instead, and the tooling derives it — never the caller. From any label, every implementation MUST produce the same slug by applying exactly these steps in order:

1. **lowercase** the label;
2. **map** every character outside `[a-z0-9]` to `-`;
3. **collapse** each run of consecutive `-` into a single `-`;
4. **trim** leading and trailing `-`;
5. **truncate** to the slug budget;
6. **trim** leading and trailing `-` again, because truncation can expose one.

If the result is empty after step 4 or step 6 — a label that is entirely non-ASCII, punctuation, or whitespace — the slug is the literal `snapshot`. The slug budget is never below 8, so that fallback always fits.

The alphabet is ASCII because a slug becomes a filename on every supported platform; non-ASCII labels normalize away rather than reaching the filesystem. The budget is whatever keeps the repository-relative snapshot path within **150 characters**, clamped to at most 40 and at least 8:

```text
budget = clamp(150 - len(".engr/snapshots/<kind>/<stream>/.<stream>.snap.<head token>.json"), 8, 40)
```

`<head token>` is the de-hyphenated head event ID **as actually serialized**, measured rather than assumed: v1 writers emit UUIDv7, a legacy ULID head is shorter, and a future encoding could be longer. Implementations MUST NOT hard-code a token width.

Windows still enforces a 260-character path limit across supported Engr hosts, so this bound is part of the protocol rather than advice to keep repositories shallow. A longer stream ID buys a shorter slug automatically. An Agent never truncates a name by hand; it passes the label it wants and the tool normalizes it.

Directories use the immutable stream ID. Existing snapshots are never renamed or overwritten. Among usable snapshots a reader chooses the one whose `through` head sits furthest along the canonical chain — never file modification time, and never the greatest `through.rev`, which is the ranking authority the base-selection rule above forbids. On a validated chain the two agree, because `rev` advances by one; they stop agreeing the moment a candidate comes from a branch this history did not accept, which is exactly when the answer matters. Full verification replays from the root and compares the snapshot State at its head.

## Stable CLI Contract

Project-local Engr binaries expose one command surface:

```text
engr [--root PATH] init
engr doctor [--json]
engr version [--json]
engr prepare --stream ID --event TYPE --record-file FILE [--data-file FILE] [--json]
engr confirm --response TEXT [--json]
engr discard CODE [--reason TEXT] [--json]
engr append --stream ID --event TYPE --record-file FILE [--data-file FILE]
                   --initiator agent|system --basis BASIS
                   --expected-parent EVENT-ID|none [--json]
engr replay [ID] [--full] [--json]
engr show ID [--brief|--provenance] [--format text|markdown|json]
engr backlog [--format text|markdown|json]
engr why ID [subject] [--format text|json]
engr snapshot ID [--name SLUG] [--json]
engr verify [ID] [--json]
engr conformance [--json]
```

Direct append always requires the head the Agent actually reasoned from. Under the shared project writer lock, the tool reconstructs the canonical chain, rejects a mismatched `--expected-parent`, preflights the reducer transition, preserves every existing byte, and performs one operating-system append operation. A `decision.superseded` admission also requires its different target stream to exist and currently replay to `accepted`; this check occurs before the immutable append and prevents supersession cycles.

`prepare`, `confirm`, and `append` print no success before the event is durably appended and the affected stream replays successfully. On post-append replay failure, the immutable event remains and the command reports the failure; it never deletes or edits the event.

Commands write their requested result to stdout and diagnostics to stderr. `--json` emits exactly one JSON object on stdout. Text output is UTF-8. Paths in persisted JSON are repository-relative with `/` separators. A failing command writes `ERROR[<exit-code>] <message>` to stderr and prints nothing to stdout.

## Stable Output Contract

Every implementation emits the same stdout for the same project state. JSON object key order is insignificant; parsed content is not. Implementation identity is the only field allowed to differ between implementations: `version` reports its own `implementation`, and `doctor` reports the `selected_implementation` available on that host.

```text
version   text  engr <ver> (protocol=1, event-schema=1, state-schema=1, implementation=<name>)
          json  {implementation, implementation_version, protocol_version, event_schema_version, state_schema_version}
doctor    text  PASS protocol-v1 / Project: <root> / Selected implementation: <name> / Streams: <n>
          json  {ok, project_root, versions, selected_implementation, implementations[], streams}
replay    text  REPLAYED <stream> rev <n> — <status>            (one line per replayed stream)
          json  {ok, streams:[{stream, head, status, rejected_events, reconciliations}]}
verify    text  PASS protocol-v1 / <stream>: rev <n>, events=<e>, rejected=<r> / WARNING <text>
          json  {ok, protocol_version, verified_streams:[{stream, head, events, canonical_events,
                 rejected_events, reconciliations}], warnings[]}
show      text  <stream> — <status> / State through: rev <n> (<event-id>) then the active sections
          json  the complete State document
backlog   text  <stream> — <status> — <title>                   (markdown prefixes `# Engineering Backlog`)
          json  {work_items:[<State document>]}
why       text  rev <n> <event-id> <event> [canonical|rejected-history] then the indented record text
          json  {stream, subject, events:[{event_id, rev, event, text, data, provenance, canonical,
                 rejected_by_reconciliation}]}
snapshot  text  SNAPSHOT <path> through rev <n>
          json  {ok, path, through}
prepare   text  the exact candidate block ending with `CONFIRM <code>`
          json  the pending confirmation receipt
confirm   text  APPENDED <event-id> <stream> rev <n> / State: <status> through rev <n>
append          same as confirm
          json  {ok, event_id, stream, head, event_path}
discard   text  DISCARDED <code>
          json  the archived receipt
```

`show` and `backlog` are the Agent Views that filter the State registry down to currently active content: active facts, constraints, and hypotheses; unresolved unknowns; proposed or selected solutions; in-progress or completed implementations; current verification criteria; active findings and blockers; and every risk. Invalidated, removed, rejected, superseded, and resolved entries stay in State and are omitted from the view. `--brief` is accepted and equals the default view; `--provenance` annotates each entry with its status, last event, and provenance, and is the view for inspecting non-active entries. It covers the whole registry with no exceptions: the current problem and impact, retired problems and impacts, decision links, and Work Item relations are rendered there in the same annotated form as every other retained entry, including after they leave the active view. The default view prints the problem and impact as plain statements, which is what an Agent works from; `--provenance` annotates them, because an inspection surface that skips the two entries a Work Item is about is not one. Relations never appear in the default view — a Work Item's active slice is its own work — so `--provenance` is where they are read.

## Exit Codes

| Code | Meaning |
| ---: | --- |
| 0 | success |
| 2 | invalid CLI usage or exact confirmation response mismatch |
| 3 | project, stream, entity, receipt, artifact, or implementation not found |
| 4 | malformed JSON, schema violation, unsupported event data, or corrupt derived file |
| 5 | reducer transition, reference, lifecycle, or resolution invariant violation |
| 6 | unresolved or invalidly reconciled stream fork |
| 7 | incompatible protocol/schema version |
| 8 | filesystem, locking, atomic-write, or unexpected tooling failure |

Conformance compares exit category as well as semantic JSON. Implementations must not collapse a fork or invariant violation into generic success/failure.

`confirm` and `discard` sit on opposite sides of the 2/3 boundary, so the split is stated once here. Every rejected `confirm` response is exit `2` — a generic acknowledgement, a code carrying commentary, surrounding whitespace, and a well-formed code that is not the current pending one are all the same thing: the response did not match. `discard CODE` names a receipt it expects to act on, so a code with no pending receipt is exit `3`.

## Verification

`engr verify [ID]` uses EventStore as the authority and checks:

- JSONL completeness, UTF-8, schemas, path/date/stream agreement, and global event-ID uniqueness;
- canonical chain, revision/parent rules, explicit fork reconciliation, and transition legality;
- full root replay against persisted State and manifest heads/integrities;
- snapshot compatibility, integrity, canonical head, and full-replay equivalence;
- Work Item resolution gates;
- cross-stream Work Item and Decision Record references;
- accepted confirmation receipts and event provenance/hash agreement;
- absence of pending receipts whose expected parent has already changed (reported as stale, not semantic corruption).

Project-wide verification reports all independent failures when safe; it never repairs history automatically.

## Corruption And Secret Recovery

Malformed JSONL, a partial line, a missing parent, or a duplicate key/ID in authoritative history fails closed, and so does a State that declares this schema and then fails its own integrity check. The tool reports a path and line or event locator and writes no new State. Recovery restores exact trusted bytes from version control or backup under explicit operator authorization; it never skips the bad record or invents a semantic replacement automatically.

A base the read path merely cannot use is not corruption in this sense and MUST NOT be reported as such: State from another schema or off the current chain, and any snapshot that fails any usability check, are discarded silently and replay continues from the next candidate. `verify` still reports them, because `verify` audits the whole project instead of choosing a base.

Append-only history is a correctness rule, not permission to retain an exposed credential. If a secret is persisted, stop normal operations, avoid reproducing it in diagnostics/views, rotate or revoke it, and use the repository's authorized secret-removal and history-rewrite incident procedure. Record the exceptional recovery and verify the rebuilt canonical store afterward. This operational exception must not be disguised as an ordinary semantic supersession.

## Conformance

Every implementation runs the same immutable fixtures and produces semantically identical JSON State and exit categories. The normative suite is described in `conformance.md` and includes the design stress tests, long-history preservation, human confirmation, snapshot replay, and fork handling.

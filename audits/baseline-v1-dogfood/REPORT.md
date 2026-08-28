# Clean-room historical migration and baseline-v1 dogfood report

**Run date:** 2026-08-28 UTC  
**Repository:** `lukeo3o1/engr`  
**Outcome:** **NO** — the pinned current baseline cannot migrate the earliest usable Object/Section Human-gated workspace. Independent current-generation workflows work, but they do not establish historical continuity.

## Scope, isolation, and method

The run fetched all remote heads, tags, and pull-request heads, then pinned every binary by commit. Experiments lived under `/tmp/engr-dogfood` in separate `historical-engr`, `baseline-engr`, `dogfood-project`, `current-continuation`, `test-artifacts`, and `checkpoints` trees. No generated `.engr` was placed in the engr source checkout. No production code was changed and no fix was attempted.

Checkpoints retained during the run were: before historical initialization; after historical record creation; immediately before migration; a failed bridge state; a fresh-v3 continuation used only as a documented workaround; after current-design ingestion; and after stress scenarios. A Git bundle of the valid historical fixture was also retained in the temporary artifact tree.

This was a CLI-driven dogfood run, not a substitute for the repository test suite. Commands, stdout/stderr, exit codes, and confirmation transcripts are preserved in `evidence/`.

## A. Version identification

### Version A: earliest usable Object/Section Human-gate revision

* **Commit:** `e8459580e1e606c68df0deb54218c519f2784d91`
* **Author date:** `2026-08-13T03:11:50+08:00`
* **Commit subject:** `feat!: rewrite as v0 on an object/section model`
* **Branch/tag:** no surviving dedicated remote branch or tag was required; the commit is in the fetched ancestry of `feature/baseline-v1` and `main`.

This is the earliest complete revision whose own README, protocol, CLI, and tests agree on the requested design: Objects aggregate Sections; all seven semantic actions pass through `prepare` → exact Human confirmation; Sections are current authority; events are a purgeable projection buffer; and Git provides durable history/look-back. The earlier `1e0f351` is the superseded 0.1 event-sourced system, not the requested Object-only design. Later commits harden or extend the same model, so choosing them would not be the earliest usable starting point.

Version A writes `.engr/format.json` as `{"format":"engr-workspace","version":1}`. Object envelopes carry `format/version`, UUIDv7 id, title, `open|closed` status, revision and monotonic next Section id; Sections carry current text, committed `based_on`, whole-Section refs, SHA-256, and `confirmed_at`. Available operations were init, create, add, revise, merge, delete, close, reopen, candidate read/list, exact confirm, list/show, purge, and verify.

The gate minted a six-character challenge, rendered the exact proposal, bound it to `expected_rev`, allowed one live candidate per Object, rejected stale candidates, discarded qualified assent, and admitted only exact `CONFIRM <code>`. The transcript demonstrates the actual boundary rather than hand-authored JSON. [Historical Human Gate transcript](evidence/historical-human-gate.log).

### Version B: pinned current baseline

* **Remote:** `origin/feature/baseline-v1`
* **Pinned commit:** `6631326aefe688f9f06764cfd514c75c59783471`
* **Author date:** `2026-08-29T02:34:20+08:00`
* **Commit subject:** `Merge pull request #55 from lukeo3o1/codex/fix-protocol.md-for-alignment`

The remote did not move during the run. `main` was never substituted.

## B. Historical engineering record

Version A created **5 Objects and 10 Sections**, entirely through the Human Gate:

| Object | Sections | Durable knowledge retained |
|---|---:|---|
| Purpose and durable engineering memory | 2 | Preserve settled decisions/rationale/invariants; exclude project-management noise; evidence-driven growth rule. |
| Human authority and admission boundary | 2 | Every semantic write is reviewed and exactly confirmed; candidates freeze and revision-bind mutations. |
| Object and Section record model | 2 | Sections contain the sole current wording; UUIDv7 Object identity; monotonic non-reused Section ids. |
| Git basis, provenance, and trust-sensitive reads | 2 | Committed basis and read-time drift; dependency hash plus historical commit; one actual cross-Object Ref. |
| Projection, events, and architectural constraints | 2 | Sections are current authority; events are the historical generation's buffer; deterministic reducer and atomic writer boundary. |

The historical inventory and ordinary navigation are preserved in [historical list evidence](evidence/historical-ls.txt), and historical `verify` passed for all five Objects in [historical verification evidence](evidence/historical-verify.txt). The fixture was clean and committed before migration. No candidate remained live.

## C. Migration result

### Natural encounter

Against the untouched version-1 fixture, all three normal current reads failed closed with exit 4:

```text
engr ls --all
engr show <historical-id>
engr verify
error: workspace version 1 is not supported by engr latest (6631326a)
```

This correctly avoids silently reading v1 under v3 semantics, but the diagnostic omits a next action and differs from the usual recognized-predecessor message that directs the caller to `engr migrate`. Full output is in [pre-migration modern read evidence](evidence/pre-migration-modern-read.log).

### Supported migration attempt

`engr migrate` at the pinned baseline produced the same unsupported-version error. Source inspection confirmed this is deliberate: the current migrator accepts only workspace v2 (plus format-less legacy), while v1 is merely historically recognizable. Thus the requested direct/cumulative v1→v3 contract does not exist.

A minimal exploratory workaround was attempted and is **not counted as success**:

1. A pinned pre-Phase-3 baseline (`a0aa463dc99837d988384317786699b97fa297c1`, the integration baseline named by issue #32) migrated v1 to v2.
2. The exact pinned Version B then attempted v2 to v3 without hand editing.
3. Version B rejected the bridge output: `workspace-v2 Object does not have its generation's canonical envelope`.

The v1→v2 bridge output is in [bridge evidence](evidence/workaround-a0aa-v1-v2.log), and the v2→v3 rejection is in [current migration rejection evidence](evidence/workaround-v2-v3.log). An earlier bridge at `ce9820b` failed identically. No manual transformation or resealing was used.

### Preservation, integrity, and crash/resume

Because no supported migration completed, there is no post-migration resource state and semantic preservation, stable identity, timestamp/admission preservation, Event treatment, seal generation, deterministic publication, or historical Ref preservation are **unproven**. The valid v1 fixture remains the authority for its ten principles.

A synthetic incomplete `.engr/migration-v3/` marker on a disposable v3 copy made Object and Backlog reads fail closed with a precise resume instruction. `engr migrate` then failed on the absent manifest rather than guessing. This partially validates maintenance-window fail-closed behavior, but does not validate interruption after a genuine staged manifest, publication atomicity, or deterministic resume.

## D. Independent current-design record

To continue independent feature exploration after the migration blocker, a **fresh v3 workspace** was initialized. This is a clearly separated workaround, not a migrated record and not evidence of continuity. Six Objects were recorded incrementally through the real Human Gate, followed by one Agent-admitted Section after a passing Rule Review. Final inventory: **6 Objects, 13 Sections**.

| Current Object | Accepted design distilled | Owning sources reviewed |
|---|---|---|
| Authority and mixed-admission Sections | Per-Section Human/Agent authority, governed semantic writes, fail-closed no-Rule behavior, serialized authoritative writes | #9, #25, #32 |
| Current representation and integrity | Exact JCS v3 resources, Object/Section seals, selective semantic Ref digest, no silent reseal, exact Human repair | #13, #31, #32, #35 |
| Backlog boundary | Explicitly unresolved staging, subjects/produced, exact predecessor and destructive atomicity | #8, #25, #32 |
| Execution and planning are non-authoritative | Work handoff owns no semantics; Collection grouping/order/priority is navigation, not authority | #10, #12, #32 |
| Reference and provenance model | Canonical Ref, selective fields, distinguishable drift/trust states, Alias deferred | #7, #11, #28, #32, #35 |
| Migration and storage boundaries | Coordinated maintenance window and atomic publication; current paths preserved; month layout/merge reduction deferred | #15, #32, #33, #35 |

The audit read issue bodies **and all fetched comments** for #7, #8, #9, #10, #11, #12, #13, #14, #15, #16, #20, #25, #28, #31, #32, #33, and #35. #32's consolidation/precedence was applied; historical bodies were not treated as overriding later rulings. Issue numbers appear here only as provenance mapping, not as substitutes for meaning in the record.

The fresh v3 Human transcript is in [current Human Gate evidence](evidence/current-human-gate.log). Final verification passes, including the Agent-admitted Section, in [current verification evidence](evidence/current-verify.txt).

## E. Feature and scenario matrix

`PASS` means the observed CLI behavior matched the accepted contract. `PARTIAL` means only a meaningful subset was reproducible. `NOT RUN` means the migration blocker or finite audit window prevented defensible coverage; it is never treated as a pass.

| Area / scenario | Expected | Actual | Result / evidence |
|---|---|---|---|
| Historical create/read/Human admission | Exact reviewed proposal is admitted and readable | 5 Objects/10 Sections created; list/show/verify succeeded | **PASS** — Human transcript and historical verify |
| Current Object create/add/read/roles/content model | Supported CLI writes only through admission | Six Objects, decision roles, null basis, Human admissions; final verify passes | **PASS** |
| Human qualified assent | Discard Candidate; do not admit | Commentary after exact phrase discarded it; later lookup exit 3 | **PASS** — [scenario log](evidence/scenarios.log) |
| Replaced/stale Candidate | Older code cannot admit | Second prepare superseded first; old confirm exit 3 | **PASS** |
| Agent Rule Review pass | Surface applicable Rule/digest, then admit exact passed mutation | First call surfaced Rule and v1 digest; repeated exact mutation admitted as Agent, rev 4 | **PASS** |
| Rule artifact byte drift | Earlier review identity no longer matches | Same attestation rejected exit 5 and supplied current digest | **PASS** |
| Multiple applicable Rules | Complete set required | Not exercised | **NOT RUN** |
| Review attempts/default/explicit max, reject/human-confirm exhaustion | Follow each Rule policy; no invented fallback | Rule declared explicit 2/human confirmation, but exhaustion sequence not completed | **PARTIAL** |
| Effective semantic equivalence vs artifact-exact identity | Follow #25/#32 identity split | Artifact drift covered; semantic-equivalence variant not completed | **PARTIAL** |
| Backlog create + subject | Clearly unconfirmed staging | Created genuine cumulative-migration question, subject to migration Object | **PASS** |
| Backlog add/revise/topic/produced/consume/merge | Exact predecessor; destructive operations atomic | Not completed | **NOT RUN** |
| Two Backlog actors / stale predecessor | Stale exact token fails, newer state remains | Not completed | **NOT RUN** |
| Backlog Rule Review/exhaustion | Non-destructive vs destructive policy differs | Not completed | **NOT RUN** |
| Work start/item/block | Explicitly non-authoritative execution state | Started, added item, blocked; screens prominently label execution memory | **PASS** |
| Work pause/resume/dependency/session concurrency | Explicit human pause/resume; unrelated sessions avoid serialization | Not completed | **NOT RUN** |
| Collection create/membership/order/priority | Planning/navigation only | Object and Backlog members ordered/prioritized; UI labels planning and semantic non-authority | **PASS** |
| Collection state/missing or consumed target/concurrency | Preserve navigation semantics and stale safety | Not completed | **NOT RUN** |
| Current selective Ref states | Distinguish unchanged, drift, integrity, unavailable provenance, schema mismatch | Design recorded; end-to-end matrix not completed | **NOT RUN** |
| Aggregate tamper read | Detect; do not accept because JSON parses | `show` displayed content but marked Object integrity failed and exited 5 | **PASS** |
| Ordinary mutation after tamper | Must not silently reseal | Rename refused with stored/current seal values, exit 5 | **PASS** |
| Human repair exact replay | Gate repair; no new semantics | Not completed | **NOT RUN** |
| Truncated JSON | Schema failure, no partial read | `verify` reported path and JSON EOF, exit 4 | **PASS** |
| Migration marker reads | Maintenance window fails closed | Object and Backlog reads refused with resume instruction | **PASS** |
| Marker resume without valid manifest | Do not guess | Missing manifest reported, exit 3 | **PASS** |
| Genuine migration crash/resume/publication | No mixed generation; deterministic resume | Blocked before staging by unsupported v1 | **NOT RUN** |
| Two Object/Collection/Work writers and read/write TOCTOU | Serialize authority; session-local state scoped appropriately | Not completed as process races | **NOT RUN** |
| Dirty/detached/missing commit/read-only/partial event/canonical hostility | Owning contract-specific failures | Only truncated JSON and invalid aggregate seal exercised | **PARTIAL** |

## F. Findings

### BLOCKER — no historical v1 → current v3 migration path

**Impact:** A real user of the earliest usable Human/Object engr cannot migrate that durable record with the pinned current baseline. Even a tool-mediated sequential bridge through the consolidation baseline yields v2 bytes that the current v2→v3 migrator rejects. The remainder of the requested historical-continuity test is therefore impossible without manually understanding generations or writing a new converter.

**Minimal reproduction:**

```bash
git checkout e8459580e1e606c68df0deb54218c519f2784d91
cargo build --release
# In a Git repository: engr init; create and confirm one Object/Section.

git checkout 6631326aefe688f9f06764cfd514c75c59783471
cargo build --release
./target/release/engr --root <fixture> migrate
# error: workspace version 1 is not supported by engr latest (6631326a)
```

**Expected:** Because the product retains and recognizes historical v1 and this exercise starts at the first supported Object-only release, current tooling should provide a documented cumulative migration or a documented exact chain whose outputs satisfy each successor's predecessor contract.  
**Actual:** Direct migration is intentionally limited to v2. The attempted documented-history bridge does not produce the exact v2 envelope required by the v3 converter.

Suggested owning issue: **#32** for cross-generation coordination, with **#35** for the migration/integrity contract. Suggested GitHub text is provided below; it was not posted.

### MEDIUM — unsupported v1 diagnostics omit recovery guidance

Normal reads and `migrate` say only that v1 is unsupported. An Agent cannot discover from the CLI whether the workspace is corrupt, too old, historically readable only, or requires a particular intermediate tool. The accepted fail-closed behavior is correct, but the next action is missing.

### UX / DOCUMENTATION — compact Ref identity is not discoverable from mutation help

Object mutation accepts a UUID/prefix, while Backlog/Collection `--subject/--target` requires the 26-character compact Ref id. Passing the persisted UUID produces `compact UUID must be exactly 26 characters`. `show --format json` exposes the canonical Ref, but help does not point the caller there. Correct, but awkward for an Agent.

### UX / DOCUMENTATION — tampered `show` renders invalid content before failing

`show` clearly marks aggregate integrity failure, provides a `git show` recovery command, and exits 5. This is trust-visible and correct, but a streaming consumer can see untrusted text before the terminal error. Human output is safe if the banner is heeded; machine consumers should rely on exit status and structured integrity fields.

### TEST-GAP — public end-to-end cumulative migration fixture

The automated tree has extensive v2→v3 contract tests, but this real v1 fixture exposed the absence of a public, release-to-release migration route. A checked-in fixture generated by `e845958` through the actual Human Gate would pin the supported historical promise (or explicitly document its non-support).

## G. Design-alignment findings

1. **Accepted current baseline vs user-visible historical continuity:** #32 explicitly freezes Phase-3 migration as v2→v3 and the implementation matches it. The earliest Object-only format is v1, so the implementation is aligned with the narrow accepted text but the product cannot satisfy end-to-end historical migration. This is a **design coverage gap**, not a claim that the current v2→v3 implementation violates #32.
2. **Protocol/implementation:** Current `PROTOCOL.md` and implementation agree that v3 is exact JCS, Sections carry mixed authority, reads verify seals, migration is a maintenance window, and ordinary mutation cannot reseal damage. Observed CLI behavior aligned in the exercised cases.
3. **Skill/CLI:** The repository Skill's Human/Agent boundary aligned with observed Candidate and Rule Review behavior. Rule artifact drift invalidated review identity as required.
4. **Persisted representation/CLI:** The fresh v3 workspace used canonical format bytes and verified. The bridge v2 representation disagreement is precisely diagnosed, but no supported tool explains how a v1 owner can reach the expected canonical v2 envelope.
5. **Deferred designs:** Alias (#11) and month-based storage (#15/#33) were not incorrectly recorded as current implementation. They remain deferred boundaries.

## Suggested GitHub text (not posted)

> **Historical v1 Object workspace has no usable route to current v3**
>
> Reproduced with `e845958` creating a format-v1 workspace solely through prepare/exact Human confirm, and current `feature/baseline-v1` at `6631326`. Normal reads and `engr migrate` fail with `workspace version 1 is not supported`. A sequential attempt using #32's pre-P3 integration baseline `a0aa463` to migrate v1→v2 succeeds there, but `6631326 engr migrate` rejects the result: `workspace-v2 Object does not have its generation's canonical envelope`.
>
> Please either (a) define a cumulative v1→v3 converter, or (b) publish and test an exact supported tool chain whose v2 output meets the frozen v2→v3 predecessor contract. Also make the v1 diagnostic name that recovery path. Preserve a fixture generated by the historical binary rather than hand-authored JSON.

## H. Overall assessment

# NO

A real Agent **can** use the earliest revision to build a coherent Human-confirmed engineering record. A real Agent **can** use a fresh current workspace for Human admission, Rule-reviewed Agent admission, Backlog, Work, Collections, integrity-visible reads, and fail-closed malformed data. However, it **cannot migrate the historical record to the pinned current baseline through supported tooling**, and therefore cannot continue the same engineering memory without manual generation knowledge or re-entry. That failure breaks the central continuity claim of this exercise; independent fresh-v3 success cannot compensate for it.

# Audit: how the agent kept working state in engr (run N)

Sources: S1/S2/S3 logs (ground truth), state-S*.txt, probe-*.md, metrics-S*.json. "S2 T59" means session 2, turn 59 of the main agent. Where a log line was truncated I say so.

**Summary.** Resume worked: every probe reconstructed position and next step, and both successors repaired the previous gap within seven turns. The costs sat elsewhere. About 85% of main-agent turns went to getting wording past `record-mss`. Decisions the Rule rejected were often dropped rather than kept somewhere. The one Human handoff was quietly voided and still read "pending" when the run ended.

---

## A. At each cut-off

### S1 (cut at T74, 74 turns)
**Doing and intending at the cut:** `git add` of all `.engr` state was done, the commit was not. T74 says: "Now let's commit `.engr` and start item 2 (extending `store.rs`), marking it active."

**Done in the session:**
- The agent drafted 15 design decisions as ChangeSet steps (T16–T18). After a cold read it revised them into 16 (T39–T43).
- Three review attempts admitted 16 Sections: 10 at T47, 6 at T55.
- The View-document shape/validity Section was exhausted (T59). The agent staged backlog 01a0f2a6 (T68) and prepared Human candidate CS7JDN (T72).
- It added json-patch =4.2.0 (T33, commit 928dd5f at T42).
- It staged four open questions in the backlog (T23) and created the sidecar (7 items) and the collection.

| Item | Status at the S1 cut |
|---|---|
| 16 Sections, 5 backlog points, candidate, collection | In engr |
| Sidecar summary "Item 2 next: json-patch dep (done, commit 928dd5f), now extend store.rs" | In engr, accurate |
| "ui.update validates before it writes" (patch a private copy, re-validate, persist only if valid; T17) | **Missing.** It failed criterion 2 at T47, was removed at T49 and was never re-added. It is not in any Section, backlog point or sidecar. |
| "An element unreached from root, and a cycle among children, are still valid" (T43) | **Missing.** Removed at T57 to get the shape past review. It survives only as history inside the backlog point ("attempt 2 ... bundled ... a permissiveness claim about unreached elements and cycles"). |
| "A stale interaction produces no event, no sequence number and no domain effect" (T18/T40); "a refused interaction leaves no gap" (a separate step, dropped T51) | Dropped. Only partly implied by §16 ("for each accepted interaction"). |
| Owner attachment is advisory with no credential | Only as framing inside backlog 01a0f295-e63d ("the current v1 shape which is advisory only"). Never a Section. |
| Item 2 state | `pending`, although half of it (json-patch) was done. Only the summary says so. |
| All engr state | **Uncommitted.** Staged `A` in git status (metrics `dirty_inside_engr`: 9 files). |

### S2 (cut at T83, 84 turns)
**Doing and intending at the cut:** it had just committed §18's admission (e0ea5a9). T83: "Let me commit and close out item 2 fully, then move to item 3."

**Done in the session:**
- It committed S1's staged state (T8) and discarded the empty S1 ChangeSet (T26).
- It judged the "basis moved" notices on §1–§5 harmless (T6: "nothing in Cargo.toml/Cargo.lock touches storage layout...").
- It delegated item 2. The worker committed 3989cc7 and drafted 3 decisions.
- The agent rewrote the 3 decisions after a cold read and admitted §17 (existence is decided by the record file) at T59 and §18 (id validation, which references §11) at T82.
- It ruled out lock order as a Section (criterion 4, narration) and moved it to the backlog (T65–T68).

| Item | Status at the S2 cut |
|---|---|
| §17, §18, item 2 done with commit 3989cc7 | In engr |
| Backlog 01a0f2b2-c191 | In engr but **never committed** (`?? .engr/backlog/01a0f2b2...`). The agent committed only the work file at T73. |
| The §1–§5 basis check | Missing. The conclusion was never recorded. |
| Summary "Id-validation passed 2nd cold read (now refs SS11); Rule Review attempt 2 running." | **Stale.** It passed and was applied at T82. |
| Item 2 result "3 decisions in changeset 01a0f2a7 (§17-19, not yet applied)" | **Stale.** Two were admitted and one went to the backlog, and the numbers moved. |
| Item 1 result "Human candidate CS7JDN pending" | **Wrong from T59 on.** Admitting §17 moved the Object to rev 18 and killed the candidate. state-S2 shows `CS7JDN ... stale`. The agent never noticed. |

### S3 (ended at T82, 82 turns, `end: success`)
**Doing at the end:** the main agent was waiting on the item-4 worker (T81–T82: "Waiting for the worker to finish"). The worker had already committed uiview.rs (4a0a89b, 14:59) and added ChangeSet steps §24–§26. Its report never reached the main agent. The next steps would have been to verify, close item 4, stage the worker's OPEN lines, and review and apply 01a0f2ca.

**Done in the session:**
- It committed S2's orphaned backlog file (T7) and fixed the stale summary (T14) and item 2's result (T67).
- Item 3 (session.rs, 6b9f8ce) was done. The worker drafted 8 decisions and the agent cut them to 5: §19–§23 admitted at T59 and T71. Three were dropped as "pure implementation detail ... or a restatement of already-admitted SS18", with the reason only in commit message 920cc2e.
- It staged ownerless-open/detach (T38) and the View format/version question (T76) in the backlog.
- It told the item-4 worker to implement the unadmitted View shape from backlog prose (T80).

| Item | Status at the S3 end |
|---|---|
| §19–§23; items 2 and 3 done with commits | In engr |
| Item 4 | `active`, although the code is committed. Metrics `commits_after_sidecar: 4a0a89b`. |
| §24–§26 | Only in ChangeSet 01a0f2ca, which lives under `.engr/local` (not git-tracked, this machine only) |
| The worker's OPEN questions (reuse EXIT_STALE for §15; the inference that §4 does not gate create/close) | Only in the 4a0a89b commit body (per probe-3). Not in the backlog. |
| Choice to build uiview.rs on the exhausted, unadmitted shape wording | Only in the T80 packet and the commit body |
| Item 1 "Human candidate CS7JDN pending" | **Still stale.** S3 knew it was dead. T80 packet: "a Human candidate was prepared and then went dead when the Object moved on". The agent neither re-prepared it nor corrected the sidecar. |
| Backlog 01a0f2c9 (View version field) | Unresolved, although the worker's §24 draft answers it |
| 23/23 Sections "basis moved" | No re-check in S3 (T4 saw "2 ok 16 stale" and moved on) |

---

## B. The probes

**Probe 1**

| Question | Grade | Why |
|---|---|---|
| Q1 settled | Correct | The 16 Sections and the json-patch commit |
| Q2 open | Correct | |
| Q3 in progress | Partly correct | Correctly found json-patch done and store.rs untouched. Called item 2 "in progress" when it was `pending`. Missed that the literal in-flight act was the `.engr` commit (it did see the staged files). |
| Q4 next | Correct | "commit the currently-staged `.engr/*` files", then store.rs. That is what S1 T74 intended and what S2 did (T8, T17). |
| Q5 ask | Partly correct | The CS7JDN question is legitimate. "Were the staged files left staged deliberately" was answerable only from the log, because engr said nothing about it. |
| Q6 stale | Correct | The §1–§5 basis move, judged "dependency-only" (the same conclusion S2 reached at T6), and the uncommitted state |

It could not recover the dropped atomic-update rule, the cycle permissiveness, or the advisory-owner decision, because none of them is in engr.

**Probe 2**

| Question | Grade | Why |
|---|---|---|
| Q1 settled | Correct | One small slip: "§10–§16 admitted 14:06:28Z". §10 was admitted at 14:02:53. |
| Q2 open | Correct | Calls CS7JDN "pending" here but DEAD in Q5 |
| Q3 in progress | Correct | Found that §18 was finished and that the sidecar lags e0ea5a9 |
| Q4 next | Correct | Fix the sidecar, commit the backlog point, then item 3. S3 did exactly this (T7, T14, T20). |
| Q5 ask | Correct | **Noticed CS7JDN is DEAD, which the agent itself never noticed** |
| Q6 stale | Correct | The genuine basis drift from store.rs, the stale summary, the untracked backlog file |

It did not flag item 2's stale "not yet applied" result.

**Probe 3**

| Question | Grade | Why |
|---|---|---|
| Q1–Q4 | Correct | Found item 4 finished in code (4a0a89b, 18/18 tests), the handoff unfinished, and the next step: review and apply 01a0f2ca, then close item 4 |
| Q5 ask | Correct | Surfaced the worker's EXIT_STALE and §4 inferences from the commit body |
| Q6 stale | Correct | The dead CS7JDN, item 4 still active, 23/23 basis moved |

It did not note that 01a0f2ca exists only on this machine, or that the View shape the code implements is not in the record.

No probe hallucinated anything material. Everything the probes missed was either never written anywhere (the atomic update rule, the cycle rule) or written only in git commit bodies.

---

## C. The successor sessions

**S2**
- Seven orientation turns (T1–T7). T8 was the first productive act (committing S1's state). T17 delegated item 2, the first engineering work.
- No work redone, nothing re-decided or contradicted. T6 re-verified the §1–§5 basis instead of re-deciding it.
- Repairs: committed S1's staged state, discarded the dead empty ChangeSet.
- Caused new damage: admitting §17 at T59 voided the Human candidate the previous session had left for a human. This was not noticed or recorded.

**S3**
- Six orientation turns. T7 was the first productive act (committing the orphaned backlog file). T20 delegated item 3.
- No work redone.
- **Repeated a path already ruled out:** the item-3 worker drafted forward-looking "future modules must..." directives. S2 had already learned (T65) that `record-mss` rejects these as narration. That lesson lived only inside backlog point c191's prose, so S3 spent a cold read to rediscover it (T41: "the exact failure pattern that burned the View-doc-shape section's attempts").
- Repaired: the stale summary (T14), item 2's result (T67), the orphaned backlog file (T7).
- Did not repair: CS7JDN, although it knew the candidate was dead (T80).
- Treated a backlog point as authority. T14 summary: "backlog e63d settles owner attachment as advisory for now". The T20 packet repeats it to the worker. AGENTS.md says backlog is never to be read as the record.

---

## D. The state against the four criteria

### Backlog points
- There are 8 points of 75–145 words each. All 8 have a "Settled by/when" clause (metric `backlog_with_settle_clause` 8/8).
- Each is one question in the same three-sentence template: question, "Not obvious because", "Settled by". That is clear but twice the length needed.
- **Worst: 01a0f2a6-4abe (145 words).**
  - The question is about record formatting ("recorded as one Section, or must be split into two"), not about the design.
  - It narrates the review history: "attempt 1 bundled shape...; attempt 2 dropped the catalog clause but still bundled...; attempt 3 dropped that too and was still failed because...".
  - Its settle clause ("a human either confirming the exhausted wording") points at a candidate that has been dead since S2 T59.
- **Best: 01a0f2be-8b4c.** "Whether session.rs needs an explicit way to open or leave a Session with no owner attached ... Settled once such a caller exists and says what it needs, or a human decides the shape belongs now." 01a0f295-e639 (ui.wait delivery) is similarly clean.
- Four of the eight settle by deferral ("once engr mcp is implemented"). That is honest, but they will never be settled inside this slice.

### Work items
- Seven module-sized items were created in S1 T21, for example "Implement uiview.rs: create/close, document validation, ui.update w/ JSON Patch + expectedRevision, history; tests".
- Item 1 is pure bookkeeping ("Cold-read + review + apply the 15-step design ChangeSet").
- Item 2 bundled two unrelated steps (the dependency and store.rs), so its state was wrong at the S1 cut.
- Done-conditions: 0/7 at S1 and S2, 2/7 at S3. Items 3 and 4 were revised in S3 to "done when cargo test session:: passes".
- Ticking with evidence was good where it happened. Item 3: "cargo test session:: 11 passed; workspace+fmt clean" plus commit 6b9f8ce. Item 2 carries commit 3989cc7. Item 1 was never given a commit (`done_without_commit` 1 in every session).
- The state lagged reality at every cut:
  - S1: engr uncommitted, item 2 half done
  - S2: the summary "running" line and the "not yet applied" result
  - S3: item 4 still active
- The cause is the same each time: the agent updated the sidecar before waiting on a background subagent, and the cut landed in the gap.
- Items 5–7 were never touched.

### Reasoning
Paths that were ruled out, and where they went:
- **Nowhere:** the atomic-update rule (S1 T49), cycle permissiveness (S1 T57), the §1–§5 basis check (S2 T6).
- **Git commit bodies:** the 3 dropped session decisions (920cc2e), the uiview inferences (4a0a89b).
- **Backlog prose:** lock order, and the review history above.
- **Sidecar summary**, used as a live status ticker ("Rule Review attempt 1 running", "attempt 2 running"). Each version was committed separately. S2 made 11 commits: 1 code, 2 record, 7 sidecar-only.

There was no findable place for "the Rule rejects forward-looking directives", which is why S3 relearned it.

### Record Sections
- 23 Sections of 28–88 words, each with a "because". Most say one thing: §3, §6, §14, §15, §16 are good.
- The record is uneven in what it holds:
  - Code-level trivia got in, for example §19 "session::open ... call no crate::rules::direct and take no Attempt argument" and §23 on `require_current`.
  - The central data contract, the View document shape, never did.
  - Neither did the rule that ui.update never persists an invalid document.
- Some reasons are weak:
  - §22 "because this slice has no caller that needs to open or resume a Session without attaching an owner" is a reason from absence. Combined with §3, it also makes "detached" unreachable in this slice.
  - §12's reason is only "for the same reason ... as a Session id".

---

## E. Cost

I classified the main agent's turns. Engineering means reading code to design or to write a worker packet, making edits, running build/test, and verifying worker diffs. Everything else is engr: loading skills, writing records, drafting and rewording Sections, cold reads and reviews, waiting on reviewers, commits of the sidecar alone, and reading gate.rs to learn how exhaustion works (S1 T60–T66).

| Session | Engineering turns | Share | Main engr stretch |
|---|---|---|---|
| S1 | ~16 of 74 (T5–T11, T31–T37, T41–T42) | ~22% | T38–T73 on reviews and exhaustion |
| S2 | ~8 of 84 (T9–T12, T17, T29–T31) | ~10% | T32–T83, 52 turns and 5 subagents, to admit 2 Sections from one 122-line change |
| S3 | ~12 of 82 | ~15% | T28–T73, 46 turns and 4 subagents, to admit 5 Sections |

- Roughly 15% engineering and 85% engr across 240 turns.
- 16 subagents in total: 3 workers wrote all ~1,390 lines of code, and 13 were cold readers or reviewers.
- Result: 3 of 7 items closed, a 4th done but not closed. interaction.rs and the CLI were never started.
- **Not proportionate.** The bookkeeping, mostly rewording prose until a reviewer accepted "one assertion", crowded out the work.

---

## F. Decisions, one by one

Here "record" means an admitted Section.

| # | Decision | Made | First reached engr | Agrees with reason? |
|---|---|---|---|---|
| 1 | json-patch crate, not hand-rolled; pinned 4.2.0 | S1 T11 / T33 | Record §10, S1 T47 | Yes |
| 2 | Store under .engr/local/ui/ | S1 T16 | Record §1, T47 | Yes |
| 3 | Per-Session lock, not the workspace lock | S1 T16 | Record §2, T47 | Yes |
| 4 | Session id s_+UUIDv7; View id v_+UUIDv7 | S1 T16 | Record §11/§12, T55, after a failed attempt | Yes |
| 5 | resume always takes ownership, even from a live owner | S1 T16/T40 | Record §6, T47 | Yes |
| 6 | Detached is derived, not stored | S1 T16 | Record §3, T47 | Yes |
| 7 | View document shape plus root/children validity | S1 T17 | Backlog 01a0f2a6 and candidate CS7JDN, S1 T68/T72. **Never a Section**; the candidate has been dead since S2 T59 | Shape has no stated reason |
| 7b | Cycles and unreached elements are valid | S1 T43 | **Never** (history only, inside 7's backlog text) | No |
| 8 | Prop schemas not checked this slice | S1 T17 | Record §13, T55 | Yes |
| 9 | ui.update patches a private copy, re-validates, persists only if valid | S1 T17 | **Never** (dropped T49) | No |
| 10 | expectedRevision checked under the Session lock | S1 T17/T40 | Record §7, T47 | Yes |
| 11 | View revision excludes domain data | S1 T17 | Record §14, T55, after a failed attempt | Yes |
| 12 | History stores the caller's patch; snapshot is not replayed | S1 T17/T40 | Record §8, T47 | Yes |
| 13 | Stale interaction refused with an explicit error | S1 T18 | Record §15, T55. The "no event, no sequence" part was dropped | Partly |
| 14 | Per-Session sequence starting at 1, +1 | S1 T18 | Record §16, T55 | Yes |
| 15 | Writes need both View and Session active | S1 T18 | Record §4, T47 | Yes |
| 16 | ui.wait out of scope; pull-only reads | S1 T18 | Record §5, T47 | Yes |
| 17 | Library-only; the CLI is read-only | S1 T40/T43 (drafting truncated) | Record §9, T47 | Yes |
| 18 | Owner attachment is advisory, no credential | S1 T23 | Backlog framing only. **Never a Section**; S3 treated the backlog as settling it | No |
| 19 | §1–§5 still hold after the dependency commit | S2 T6 | **Never** | — |
| 20 | Session existence is decided by the record file | S2 worker (T17) | Record §17, S2 T59, after a cold-read rewrite | Yes |
| 21 | External session ids validated before store.rs | S2 worker | Record §18, S2 T82, on attempt 2 | Yes |
| 22 | Lock order deferred | S2 worker / T65 | Backlog c191, T68; uncommitted until S3 T7 | Yes |
| 23 | No Rule/Attempt gating on session mutations | S3 worker | Record §19, S3 T59 | Yes |
| 24 | No version field on the Session record | S3 worker | Record §20, T59 | Yes |
| 25 | close is idempotent | S3 worker | Record §21, T59 | Yes |
| 26 | Owner is required, 1–200 characters | S3 worker | Record §22, T59 | Weak reason |
| 27 | require_current before and after the lock | S3 worker | Record §23, T71, on attempt 2 | Yes |
| 28 | session.json filename; resource_present; validate only external ids | S3 worker | **Never**; the reason is in commit 920cc2e | — |
| 29 | Ownerless open/detach deferred | S3 T38 | Backlog be-8b4c | Yes |
| 30 | Build uiview on the unadmitted shape wording | S3 T80 | **Never** (packet and commit body) | — |
| 31 | No View version field; View stored under the Session dir; View id validated | S3 item-4 worker | Local-only ChangeSet 01a0f2ca; **not admitted** by the end | Yes (drafts) |
| 32 | §4 does not gate create/close; reuse EXIT_STALE | S3 item-4 worker | **Never** (commit body) | — |

**Tally:**
- 22 decisions reached the record as Sections (23 Sections) within the session they were made. None were admitted later.
- Of the rest, 5 went only to the backlog (7, 18, 22, 29 and the version question), and 3 sit unapplied on this machine (31).
- 8 never reached engr at all: 7b, 9, 13 in part, 19, 28, 30, 32, plus the dropped no-gap step.
- Every decision that was never recorded was lost at the same point: when the Rule rejected its wording, the agent deleted it instead of recording it elsewhere.

**Cost of recording:**
- 13 cold-read or review subagents: S1 1+3, S2 3+2, S3 2+2.
- Review attempts that failed at least one step: S1 all three (6, then 1, then 1 failed steps), S2 attempt 1 (2 of 3 steps), S3 attempt 1 (1 of 5 steps).
- One exhaustion that ended in a Human candidate, which was then voided.
- Refused commands:
  - S1 T40: uncommitted basis
  - S1 T69: missing --review
  - S1 T71: wrong digest
  - S2 T40: nonexistent step
  - S2 T47: summary over the length limit
  - S2 T65: nonexistent `backlog stage`
- About 145 of 240 main-agent turns.

---

## G. Verdict

| Criterion | Score | Justification |
|---|---|---|
| Backlog points | 3/5 | Each is one question with an explicit settle clause (8/8), but they are 75–145-word templated paragraphs, and the worst narrates review attempts and points at a dead candidate. |
| Work items | 3/5 | Items closed with test counts and commits are good evidence, but the items are module-sized, 5 of 7 never had a completion condition, and the state lagged reality at every cut. |
| Reasoning in progress | 2/5 | Ruled-out paths went into commit bodies, backlog prose or nowhere, and the summary was used as a review-status ticker, so S3 relearned what S2 had already learned. |
| Resume | 4/5 | All three probes and both successors found the true position and next step quickly, without redoing or contradicting anything. The cap is that decisions dropped at review were unrecoverable. |

**The three most important failure modes, most damaging first:**

1. **Design decisions the Rule rejected were deleted instead of kept.** Rejection by `record-mss` worked as a filter out of engr, not a route into the backlog.
   - The atomic ui.update rule (S1 T17) failed criterion 2 at T47 and was removed at T49, never to return.
   - Cycle permissiveness was cut at T57.
   - The View document shape, the slice's central data contract, never became a Section. S3 T80 then built uiview.rs on the backlog prose.
   - Worker decisions and inferences ended up only in commit bodies (920cc2e, 4a0a89b) or a machine-local ChangeSet.
2. **Bookkeeping crowded out the engineering.**
   - About 85% of 240 main-agent turns went to engr.
   - 13 reviewer or reader subagents were spent against 3 workers.
   - S2 used 52 turns and 5 subagents (T32–T83) to admit two Sections from a 122-line change.
   - The result was 3 of 7 items closed, with interaction.rs and the CLI never started.
3. **The Human handoff was voided silently and kept reading "pending".**
   - S2 T59 admitted §17 on the same Object, which made CS7JDN stale (state-S2: `CS7JDN ... stale`).
   - The sidecar kept saying "Human candidate CS7JDN pending" to the end.
   - S3 knew the candidate was dead (T80: "went dead when the Object moved on") and fixed neither the code nor the note.
   - A human arriving would hold a code that confirms nothing, for the one question that needed them. Probe 2 caught this and the agent did not.

# Audit: how well the agent kept its working state in engr (run W)

Sources: S1/S2/S3 logs (ground truth), state-S*.txt, probe-*.md, metrics-S*.json. Turn numbers are per session (`S2 T44`).

Outcome in one line: after 258 agent turns, 26 Sections are admitted, 7 backlog points are staged, and work items 1–3 of 6 are done. Items 4–6 (interaction events, inspection CLI, integration) were never started. Most of the run went into the record, not the code.

---

## A. At each cut-off

### S1 (cut at T79/80)

**What it was doing.** It had just received review round 4 (T68) and split two more steps, so ChangeSet 01a0f30d grew to 20 steps (T69–T71). At T78 it changed course: *"This has gone through four real rule-handed review rounds, and the ceiling on `record-mss` is 3 attempts before `on_exhaustion = human_confirmation`. I need to stop iterating via more agent review and honor that ceiling honestly … revert to what round 4 actually reviewed."* At T79 it was grepping the v5 screen to restore the 15-step wording that round 4 reviewed. It apparently meant to apply that digest with the failed steps marked and leave the rest for a human. The log does not show this plan completed.

**What it decided or found.** It drafted 20 design decisions (T11, then split and reworded at T28, T45, T54, T62 and T70). It staged 5 open questions (T15–T20), 6 work items (T23) and a collection (T24). Four review rounds failed 3, 3, 2 and 2 steps. It learned three tool rules: title creation is governed (T6–T7), backlog adds need `--expect` (T16) and the summary is capped at 300 characters (T50).

| Item | In engr at the cut? |
|---|---|
| 20 drafted decisions | Only as a ChangeSet under `.engr/local`, which is machine-local and gitignored. **0 Sections** were admitted (metrics `sections: 0`). |
| 5 backlog questions, 6 items, collection | Yes, but **untracked in git** (`?? .engr/backlog/`, etc.). |
| Review rounds 1–3 | Narrated in the summary. |
| Review round 4 result (2 failures, already fixed into 20 steps) | **Missing.** The summary still says *"now 15 sections after 3 review rounds … Review 4 running in background"*, which went stale at T69. |
| The ceiling conclusion and the plan to fall back to a human | **Missing**, never written anywhere. |
| Backlog §1: *"Unconditional reclaim (recorded in the Object)"* | **Wrong at the cut**: the Object had 0 sections. |

### S2 (cut at T73)

**What it was doing.** Review attempt 1 of the worker's ChangeSet 01a0f524 had failed two steps (T65–T66). It had rewritten both (T68), re-added them with `--ref` and `--based-on` (T71), and was checking `show` (T72–T73), where it saw `§2 … basis moved`. Its next step was evidently review attempt 2, then closing item 1.

**What it decided or found.**
- It admitted 19 Sections over three attempts (T11, T27, T37).
- It reversed S1's "a View or its Session may be closed" to *"Views carry no closed field of their own"* (T15).
- It dropped "No component catalog" as a repeat of §15 (T16).
- It rewrote item 2 from *"open/resume/detach/close"* to *"open/resume/close, continuation, derived detached"* (T44). This silently removed detach from scope.
- A worker built item 1 (commit 43cf026). The agent verified 8 tests plus the workspace tests and fmt (T60–T62), and admitted §20 (T66).
- It found that crates.io is reachable via cargo and json-patch 4.2.0 exists (T52–T54).

| Item | In engr at the cut? |
|---|---|
| §1–§20 | Yes, committed. |
| Rewritten §21/§22 | In ChangeSet 01a0f524. Nothing says they are already the post-failure wording. |
| Item 1 finished and verified | **Missing.** Still `active`. The summary still says *"item 1 … delegated to a worker"* (metrics: commit 43cf026 after the newest sidecar). The S2 T61 hook (*"10 tool calls since execution memory was last written … record it now"*) was ignored for 12 turns. |
| Reason for dropping detach from item 2 | **Missing.** |
| json-patch availability | **Missing.** The S3 worker rediscovered it. |
| Commit e7a3268 *"§20 (plain JSON encoding) and §22 … failed"* | **Wrong.** The plain-JSON step was §21 after the apply renumbered. |
| 19 of 20 Sections marked `basis moved` | Shown by engr, never assessed. |

### S3 (cut at T106)

**What it was doing.** Attempt 2 on the format_version step of ChangeSet 01a0f541 had just returned FAIL (rule 5: paraphrasing docs/issues/40.md:188). At T105 it removed the step **without applying the failed attempt-2 verdict**, and at T106 it re-added a third draft that cites `docs/issues/40.md:188`. Next it presumably planned a third and final review, an apply and commit, then delegating item 4 into the empty ChangeSet 01a0f54b.

**What it decided or found.**
- It redrafted §21/§22 and admitted them (T22).
- Item 2's worker produced §23 *close() clears attachment* (T55) and §24 *continuedFrom validated* (T63).
- It dropped "No detach mutator" after two failures (T65–T67) and staged backlog §6 (T45).
- At T46–T48 it found that the issue documents an explicit `detach` action separate from process loss.
- Item 3's worker added a `json-patch =4.2.0` dependency. The agent admitted §25 and §26 (T98), and the format_version step failed twice (T98, T104).
- It staged backlog §7, a View size bound (T90).

| Item | In engr at the cut? |
|---|---|
| §21–§26, backlog §6/§7, items 1–3 done with commits | Yes. |
| §25/§26 | **Uncommitted** (dirty `.engr/objects`, `.engr/eventstore`). |
| §27's spent attempt 2 | **Missing.** engr's last word on that step is *"fix them and review again at attempt 2"*. |
| Item 3 result *"ChangeSet 01a0f541 drafted (3 decisions), under review"* | **Stale**: 2 of the 3 were admitted at T98. |
| Item 2 result *"§23-25 admitted"* | **Wrong**: only §23–§24 were admitted, and the same text says the third was dropped. |
| Commit b6075b0 *"§23 (continuedFrom …) and §25 (close() …)"* | **Wrong**: the actual numbers are §24 and §23. |
| Backlog §6 *"the only other documented trigger (process loss)"* | **Contradicts** the agent's own finding at T47: *"it documents an explicit `detach` action distinct from process loss"*. Never corrected. |
| json-patch dependency | **Not in engr.** |
| Item 4 | Marked `active` but never started. |

---

## B. The probes

| Q | Probe 1 (after S1) | Probe 2 (after S2) | Probe 3 (after S3) |
|---|---|---|---|
| 1 Settled | **Correct**: nothing about the UI design is admitted (0 sections). | **Correct**: §1–§20, item 1 code verified. | **Correct**: 26 Sections, items 1–3, §25/§26 uncommitted. |
| 2 Open | **Correct**: 20 steps, 5 backlog points, 6 items. | **Correct**: 5 backlog points, 2 ChangeSet steps, items 2–6. | **Partly**: calls §27 *"drafted, unreviewed"*, but two attempts were already spent on it. |
| 3 In progress | **Partly**: rightly called "review 4 running" stale and unverifiable. It could not know review 4 had returned and been folded into the 20 steps. | **Partly wrong**: says the two steps are *"still to be reworded and resubmitted"*. They had already been reworded (S2 T68–T71). | **Hallucinated**: *"2 of those 3 decisions bypassed the changeset and were admitted directly … right after `engr prepare --agent`/confirm"*. They were admitted through `changeset apply` (S3 T98). "Not finished" is right. |
| 4 Next | **Correct relative to S1's own intent**: stop and get human confirmation because the ceiling may be spent (matches S1 T78). | **Partly**: "re-read and reword before resubmitting" would redo finished work, which is exactly what S3 did. | **Partly**: committing §25/§26 is valid housekeeping, but it misses the last-chance review of §27 and item 4. |
| 5 Ask | **Correct and pointed**: did the informal rounds count as attempts? S2 never asked this. | **Reasonable**: caught the e7a3268 numbering mismatch. | **Partly**: one question rests on its own false premise (the "bypass"). The question about empty 01a0f54b is valid. |
| 6 Disagreements | **Correct**: 15 vs 20 steps; `.engr` uncommitted. | **Correct**: the sidecar predates 43cf026 and e7a3268; noted `basis moved`. | **Correct**: uncommitted §25/§26, stale sidecar, `basis moved`. |

What the probes could not recover, and why: all three are failures of engr, not of the probes.
- Probe 1 could not know that review 4 returned or that S1 had concluded the ceiling was spent; neither was written.
- Probe 2 could not tell that the ChangeSet held post-failure wording; nothing marked it.
- Probe 3 could not see §27's second failed attempt; it was never applied. The stale item-3 result led it to invent a bypass.

All three caught the obvious staleness (summary vs ChangeSet, commits after the sidecar, uncommitted `.engr`). None could catch the unrecorded attempt counts.

---

## C. The successor sessions

**S2**
- Oriented for 3 turns (T1–T3), then did useful repair at T4–T5 by committing S1's untracked `.engr`.
- Record work started at T9. The first engineering step (item 1 delegation) came only at T55, after 50 turns of record admission.
- **Re-decided an unrecorded conclusion.** S1 T78 judged the 3-attempt ceiling spent and meant to fall back to a human. S2 started over at `--review-attempt 1` (T11) and ran three more attempts. Whether S1's unsubmitted rounds legally count is ambiguous. The point is that S2 never knew the question existed, and probe 1 flagged it.
- **Changed a design without noting the reversal.** S1's "Once a View is closed, or the Session containing it is closed" became §16's "Views carry no closed field" (T15). The issue draft says "Detached Session != closed View" (line 145, per S3 T46 grep). Nothing records that View-level close was ruled out against the issue. I cannot tell from the material whether the issue requires it.
- **Repairs:** committed `.engr` (T4–T5), fixed stale § references in items (T44–T46) and rewrote the stale summary (T41). It then left item 1 unticked at its own cut.

**S3**
- Oriented for 8 turns (T1–T8).
- **Redid finished work at T9–T11.** *"Removing the two failed steps and replacing them with corrected … wording"*: it deleted S2's already-corrected §21/§22 and redrafted them, spending a cold read and a review (T12, T19). In doing so it dropped two clauses: S2's "a caller must not assume byte-stability" and "read-then-write still needs the lock" (its commit 5599243 says this was deliberate).
- First new engineering came at T23 (tests) and T33 (item 2 worker).
- **Repairs:** closed item 1 with result and commits (T26–T27) and rewrote the summary. It never assessed the 19 → 24 `basis moved` Sections, never corrected backlog §6 after T47, and left §25/§26 uncommitted at the cut.

---

## D. The state against the four criteria

**Backlog points (7).**
- All seven are "Whether …" questions with a "Settled by/once/when" clause (metrics: 7/7). Each is 55–73 words in 3–4 sentences: question, justification, settle condition. They are not narrative, but they are longer than a minimal statement.
- **Best**, §3: *"Whether per-Session locking is worth adding once real MCP concurrency exists, in place of the one workspace-wide ui lock this slice uses. … Settled by observed contention once multiple Agent Sessions are actually running against one project."* One question, with an observable settle condition.
- **Worst**, §6: *"… Item 2 added none: is_detached() is a real derivation, but nothing in this slice ever clears attachment except close(), since the only other documented trigger (process loss) can't be observed synchronously. Settled once a caller … actually needs to signal a voluntary release."* It narrates ("Item 2 added none"), and its premise is false by the agent's own finding at S3 T47.
- §5 hides a decision (*"this slice only stamps the current version and refuses to load anything else"*) that is still not a Section at the S3 cut.
- §1 is settled by a person (*"whoever designs the daemon"*) rather than by a criterion.

**Work items (6).**
- Granularity is reasonable: one module per item, each with *"done when cargo test X passes, one test per §…"*. 5 of 6 carry a done-condition (metrics); item 6 lists checks without the phrase.
- Evidence is concrete when present: *"cargo test ui::view: 5/5 passed (§6,8,9,15,16)"* plus a commit.
- Ticking was late in S2 (item 1 finished about T59, unticked at the cut), prompt in S3 (items 2 and 3 within a few turns of verification), and the results contain errors (item 2 *"§23-25 admitted"*).
- The state matched reality at **none** of the three cuts (section A).
- Item 4 is `active` with nothing started, which misuses the state.

**Reasoning.**
- Some ruled-out paths survive well inside Sections as "rather than X because Y". Examples: §1 *"rather than a second ignored path"*, §5 *"A stored `detached` flag would be a second truth …"*, §26 *"rather than opening the file in append mode"*.
- But the review rule actively strips alternatives. S3 T52 failed §23's *"An unchecked link was the simpler, cheaper alternative …"* as narration, and it was deleted.
- In-flight reasoning was never captured: the ceiling judgement (S1 T78), the detach-scope cut (S2 T44), the closed-View reversal (S2 T15), the post-failure state of §21/§22 (S2 T68), the issue-documents-detach finding (S3 T47) and §27's second failure (S3 T104).
- Narrative went into summaries and item results in short form (*"Review 1 failed old §2,4,7 …; review 2 of fixes failed §3,11,13 …"*, *"record-mss attempt1 failed 2/3, attempt2 failed 1/3"*). That is bounded by the 300- and 160-character limits, so there was no large dump.

**Record Sections (26, 35–109 words).**
- Nearly all state one claim with its reason. Every one passed a cold reviewer, often after several tries. Example, §23: *"session::close sets Session.attachment to None … Keeping it would leave attachment ambiguous between two different meanings …"*
- Defects:
  - §20 has no `[decision]` role.
  - §22 lost its read-then-write clause.
  - §16 rules out View-level close without saying so.
  - **24 of 26 are `basis moved` and nobody checked one of them.** AGENTS.md says to decide whether each still holds. Unassessed, the trust annotation is noise.

---

## E. Cost

Rough per-session split (manual classification of the logs, ±10%):

| Session | Turns | On engineering (code read for design, dispatching or verifying workers, tests) | On engr (drafting, review, apply, sidecar, waiting on reviewers, engr commits) | Subagents: wording / worker |
|---|---|---|---|---|
| S1 | 80 | ~3 (T1, T4) | ~77 | 5 / 0 |
| S2 | 73 | ~12 (T51–T55, T60–T63) | ~61 | 4 / 1 |
| S3 | 106 | ~20 (T23–24, T33, T36–39, T68, T74, T77–80; plus code reads done only to verify record wording) | ~86 | 8 / 2 |

Overall, about 13% of turns went to engineering. 17 of 20 subagents were cold reads or reviews of wording, and all the code (about 1,200 lines across 43cf026, e9f4934 and 450c50e) came from 3 worker calls.
- S1 produced no code, no admitted Section and no git commit.
- The first code delegation in the run came 50 turns into S2.
- Engr writes ran in 27/80, 21/73 and roughly 35/106 turns (metrics).

This was not proportionate. The bookkeeping crowded out the work, and half the planned items never started.

---

## F. Decisions, one by one

"Draft" = ChangeSet under `.engr/local`, which is not a record and is not in git.

| # | Decision | Made | First reached engr | Where it ended | Same, with reason? |
|---|---|---|---|---|---|
| 1 | UI state under `.engr/local/ui/`, not a second ignored dir | S1 T11 | S1 T12 draft | §1, S2 T11 | Yes |
| 2 | Resume always reclaims ownership | S1 T11 | S1 T12 draft; backlog §1 at S1 T15 | §2, S2 T11 | Yes |
| 3 | Staleness checked before a sequence number is assigned | S1 T11 (reason T28) | draft | §3, S2 T11 | Yes |
| 4 | Session state is open/closed only | S1 T11, split T45 | draft | §4, S2 T11 | Yes (thin reason) |
| 5 | Detached is derived, not stored | S1 T11 | draft | §5, S2 T11 | Yes |
| 6 | Patch history is debug-only; reads come from the snapshot | S1 T11 (reason T54) | draft | §6, S2 T11 | Yes |
| 7 | Close is final | S1 T11 | draft | §7, S2 T11 | Yes |
| 8 | expectedRevision mismatch rejected | S1 T62 | draft | §8, S2 T11 | Yes |
| 9 | Patch a copy, publish only valid, revision +1 | S1 T11 | draft | §9, S2 T11 | Yes |
| 10 | Continuation is a new Session | S1 T62 | draft | §10, S2 T11 | Yes |
| 11 | continuedFrom is optional | S1 T62 | draft | §11, S2 T11 | Yes |
| 12 | One ui lock, separate from the domain lock | S1 T11 | draft; alternative in backlog §3 | §12, S2 T11 | Yes |
| 13–14 | No MCP surface; no domain action | S1 T70 | draft | §13, §14, S2 T11 | Yes |
| 15 | Structural validation only | S1 T11 | draft | §15, S2 T27 | Yes |
| 16 | Closed Session refuses writes; **Views have no closed field** (reverses S1) | S1 T11 / S2 T15 | S2 T18 draft | §16, S2 T27 | New reason yes; the reversal is not noted |
| 17 | No ChangeSet integration | S1 T70 | draft | §17, S2 T27 | Yes |
| 18 | Library plus read-only CLI | S1 T11 (reason rewritten S2 T29) | draft | §18, S2 T37 | Yes |
| 19 | No rendering | S1 T70 | draft | §19, S2 T37 | Yes |
| 20 | Fold "no catalog" into §15 | S2 T16 | commit 93a636b message only | — | n/a |
| 21 | Stop agent review: ceiling spent, go to a human | S1 T78 | **never** | contradicted by S2 T11 | — |
| 22 | Drop `detach` from Session scope | S2 T44 | item 2 wording only, silently | backlog §6 at S3 T45, framed as a question with a false premise | **No** |
| 23 | Reuse crate::store primitives | S2 worker (T55) | S2 draft | §20, S2 T66 | Yes (no role) |
| 24 | ui store is plain JSON, not JCS | S2 worker | S2 draft | §21, S3 T22 | Yes; S2's caller constraint dropped |
| 25 | A lone read needs no lock | S2 worker | S2 draft | §22, S3 T22 | Yes; read-then-write clause dropped |
| 26 | close() clears attachment | S3 worker (T33) | S3 draft | §23, S3 T55 | Yes |
| 27 | open() validates continuedFrom | S3 worker | S3 draft | §24, S3 T63 | Yes (alternative removed by review) |
| 28 | No detach mutator | S3 worker | S3 draft | dropped (T65); backlog §6 | No Section |
| 29 | Create refused on a closed Session | S3 worker (T74) | S3 draft | §25, S3 T98 (uncommitted) | Yes |
| 30 | History republished whole | S3 worker | S3 draft | §26, S3 T98 (uncommitted) | Yes |
| 31 | load() refuses a non-current format_version | S3 worker (also in backlog §5 since S1 T20) | S3 draft | **draft only at the cut** (2 failed attempts) | — |
| 32 | Add the `json-patch =4.2.0` dependency | S3 worker | **never** (at most the commit message, which the log truncates) | — | No |
| 33 | No View size bound for now | S3 worker | backlog §7 at S3 T90 | backlog | Yes, as a question |

**Counts, Section-worthy design decisions:**
- **6 reached the record in the session they were made**: S2 #16, #23; S3 #26, #27, #29, #30.
- **20 reached it only later**: 18 S1 decisions admitted in S2, plus S2's #24 and #25 admitted in S3.
- **3 never did**: #22 detach scope, #31 format_version, #32 dependency.
- The process decision #21 never reached engr either.

**What recording cost.**
- S1: 4 review rounds and a cold read, 14 step removals and 23 re-adds, with **zero admitted**.
- S2: 3 more attempts to admit 19 (failures 6, 2, 0), then 1 attempt on the worker's 3 (2 failed).
- S3: 8 cold-read and review subagents for 6 admitted Sections (one more dropped, one pending). 01a0f535 went fail 2/3, then fail 1/2, then a drop. 01a0f541 went fail 1/3, then fail 1/1, then a third draft.
- Refused commands: `--object` on a step (S2 T17, S3 T10 ×2), missing `--expect` (S1 T16 ×4, S3 T43), summary over 300 characters (S1 T50), item text over 160 characters (S2 T44), bare digest (S2 T10).

In total: 17 review or cold-read subagents, and about 220 of 258 agent turns spent on engr.

---

## G. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1. Backlog points | **3/5** | Every point is one "Whether" question with a settle clause, but they run 55–73 words; §6 carries narration and a premise the agent itself disproved, and §5 hides an unrecorded decision. |
| 2. Work items | **2/5** | The items are well-cut and checkable, with test counts and commits as evidence, but at all three cuts the sidecar described an earlier moment, and item results misstate which Sections were admitted. |
| 3. Reasoning in progress | **2/5** | Settled reasons and some ruled-out alternatives sit in Sections, but every in-flight judgement that mattered (attempt ceiling, detach cut, closed-View reversal, already-fixed wording, attempts spent) was left in the transcript. |
| 4. Resume | **3/5** | Successors found the main line within 3–8 turns and never redid code, but S2 overrode S1's unrecorded ceiling judgement, S3 redid S2's rewording, and probe 3 invented a "bypass" from stale item text. |

**Three most important failure modes, most damaging first**

1. **Record-keeping consumed the run.**
   - S1 spent all 80 turns, 5 subagents and 4 review rounds wording 20 Sections. It admitted none and committed nothing (metrics `sections: 0`, `commits_after_sidecar: []`, all `.engr` untracked).
   - Across the run about 13% of turns were engineering. 17 of 20 subagents reviewed wording, and only 3 wrote code.
   - Items 4–6 were never started. The first code delegation came at S2 T55.

2. **State was written before the next action, not after it, so every cut left engr describing the past.**
   - S1's summary still said *"15 sections … Review 4 running"* when it held 20 steps and review 4 had returned.
   - In S2, item 1 stayed `active, delegated to a worker` after the agent had verified 8 passing tests (T60–T62), ignoring the T61 *"record it now"* hook. Nothing noted that §21/§22 were already fixed, so S3 deleted and redrafted them (S3 T9).
   - In S3, item 3 stayed *"drafted (3 decisions), under review"* after 2 were admitted (T98), and probe 3 hallucinated a bypass from it.

3. **Judgements made off the Section path never reached engr.**
   - S1's conclusion that the 3-attempt ceiling was spent (T78) was lost, and S2 restarted at `--review-attempt 1` (T11).
   - S2 cut `detach` from scope by rewording an item (T44). It came back in S3 as backlog §6, whose premise contradicts the agent's own finding at T47, and was never corrected.
   - S3's failed attempt 2 on §27 was never applied (T104–T106), so engr still says "attempt 2" and the attempt budget cannot be recovered.
   - The json-patch dependency and the closed-View reversal appear nowhere in engr.

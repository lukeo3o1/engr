# Audit J: Dynamic UI v0 Session/View store, three cut sessions

I could not see the original task prompt (each log starts at the SessionStart hook), so I cannot check engr against anything a human specified. "Acceptance criteria" below means the done-when conditions the agent wrote into the sidecar.

Bottom line: the code got built, and successors and probes resumed correctly and cheaply. But the design intent never fully reached the record. The goal and scope statement and six other core decisions ended up in backlog labelled "unresolved" while the code implemented them. About three quarters of the coordinator's turns went to keeping engr.

---

## A. At each cut-off

### S1 cut (T75)

**What it was doing.** The agent had just rebuilt ChangeSet 01a0f78e (17 decision steps) after its second Rule Review had failed 4 steps. At T74 it pulled the new digest (`1:400c5bd0…`). At T75 it wrote the sidecar summary "attempt 3 (final, before human_confirmation) Rule Review in flight" and committed. Its stated next step was to send the attempt-3 review, apply the ChangeSet, then delegate items 1–4. It had written no code.

**Decided in S1:** 17 design decisions (listed in section E). **Ruled out:**
- an external JSON Patch dependency
- a stored "detached" flag
- a stored interactivity flag
- cascading Session close onto Views
- storing UI state under `.engr/objects` or `.engr/eventstore`

**Left open, all in backlog 01a0f78f:** the retention policy, whether pid-only liveness is safe enough, and what to do with an unknown `format` tag.

**Where the intent sat.** Every decision was in a ChangeSet that was never applied. That ChangeSet is "kept under .engr/local on this machine" (S1 T26), and `.engr/local/` is gitignored (S1 T7). At the cut, `engr show` reported 0 sections. Had S2 run on a different machine, all of S1's design would have been gone. It survived only because S2 ran on the same machine.

**Lost intent at S1 cut**

| Item | Kind | Misled anyone? |
|---|---|---|
| The human mandate. §1 first said "the maintainer chose to build this local persistence layer ahead of those surfaces rather than implement the whole draft at once" (S1 T25). The reviewer called that narration and it was cut at S1 T60. What remains is "because #40 is explicitly draft and not implementation-ready", which is a reason *not* to build. The repo cannot say that someone asked for this slice. | goal | No observed effect. Probe-1 Q3 did flag the draft's "Not implementation-ready" status as an open concern. |

Nothing else a successor needed was missing from engr.

**Stale state at S1 cut**

| Item | Misled? |
|---|---|
| Sidecar: "attempt 3 … Rule Review in flight". It was false: no review was dispatched after T74, and nothing can be in flight across a session boundary. | Not materially. S2 ran the review itself as attempt 3, which is what S1 would have done. Probe-1 did not catch the falsehood (Q10: "No other disagreement found"). |
| Sidecar items cite "§4-§5,§10-§11", "§6-§8" and "§9". Those numbers come from the first 11-step draft (S1 T41). By the cut they pointed at the wrong steps: §5 was Scope, §9 the Session-Local Lock. | No. S2 noticed and fixed it (S2 T31: "fix items 1–4's section references"). Probe-1 quoted the items without checking. |

### S2 cut (T74)

**What it was doing.** It was rebuilding ChangeSet 01a0f7d4 (session.rs decisions) after attempt 1. In that attempt only "Owner Token Shape" passed, and it was admitted as §11 (S2 T66). The agent had removed the 6 failed steps and re-added 5 reworded ones (T69–T73). At T74 it was adding a 6th, "Detach Is Always Idempotent"; that write did not land (state-S2 shows 5 steps). Next it intended to run review attempt 2, apply, close item 1, and delegate item 2.

**Decided in S2:**
- It treated its own review of the S1 ChangeSet as attempt 3 of 3 (T16, `--review-attempt 3`). It admitted the 10 steps that passed and moved the 7 that failed into backlog 01a0f7d1 as "exhausting the Agent path" (T21–T24). Those 7 include the slice's scope statement.
- It deferred the Human-path prepare until no further admissions were coming (T37–T40). The reason is only in commit 72c5b3d.
- It told the worker to treat the 7 backlog points as "the best available specification … your code should follow them" (T44).
- The worker (commit 77e2289) settled liveness, lock blocking, token shape and rotation, `continue_from`, close-once and detach-idempotent.

**Left open:** the worker's OPEN lines (token consumption, detach on a closed Session, resume token reuse). They exist only in the 77e2289 commit message; they were never staged in backlog.

**Lost intent at S2 cut**

| Item | Kind | Misled? |
|---|---|---|
| The decision that detach is idempotent even on a closed Session, with the coordinator's reason ("a caller asking to detach only wants no owner attached afterward", T74). Lost when the cut hit. The behaviour is visible in the code; the reason is not. | decision / rationale | No. S3 turned it into open question 01a0f874 §2, which matches the worker's own OPEN flag. Losing it was arguably beneficial. |
| The review status of 01a0f7d4: attempt 1 is spent, and the 5 pending steps are reworded failed steps, which engr said to "review again at attempt 2" (S2 T66). The sidecar did not record this. It is only partly recoverable: commit 125ae3b shows that one review was applied. | other (review status) | Yes, mildly. S3 T13 applied with no `--review-attempt`, so it attested attempt 1. Probe-2 Q8 could not tell. |

**Stale state at S2 cut**

| Item | Misled? |
|---|---|
| Sidecar summary "Item 1 (session.rs) delegated…", with item 1 still `active`. The worker had committed 77e2289 at 14:25, tests passed (S2 T49–T50), and §11 was admitted at 125ae3b. | No. Probe-2 Q10 caught it, and S3 re-verified (T3–T6). |
| Backlog 01a0f7d1 §2 and §5 say "unresolved", but 77e2289 implements them. Its commit message says it follows "the owner-attachment and liveness rules from backlog 01a0f7d1 §2/§5". | Partly. Probe-2 Q3 lists them as undecided. That is true to engr and false to the repo. |

### S3 cut (T97)

**What it was doing.** It was verifying the item-4 worker's commit ddbcb79. T96 showed 13/13 tests passing; T97 (workspace tests and `git show --stat`) was killed. Next it intended to check the diff against the worker's DECISIONS, cold-read and review ChangeSet 01a0f893 (§27–§31), apply it, close item 4, and note the worker's deviation (it added `session::ids()`). Item 5 would follow.

**Decided in S3:**
- Staged every worker OPEN line, from both items 1–3, in backlog topics 01a0f874 and 01a0f886.
- Withdrew the EXIT_SCHEMA claim and recorded the move/copy exit code as open (T64).
- Admitted 15 Sections (§12–§26).
- Workers made the choices listed in section E.

**Lost intent at S3 cut:** none of consequence. The deviation and the events-file format sit in commit messages, which are part of reality.

**Stale state at S3 cut**

| Item | Misled? |
|---|---|
| Sidecar "item 4 … delegated to a worker", item 4 still `active`, although ddbcb79 had landed. | No. Probe-3 Q4 and Q10 caught it. |
| 16 of 26 Sections show "basis moved". The agent's own later commits caused this, and it never checked them, contrary to AGENTS.md ("do not quietly reason from it"). | No. Probe-3 diffed them and found no drift. It was noise, but unattended noise. |
| The 7 points in backlog 01a0f7d1 are still "unresolved" although items 1–3 implemented all of them. | Partly. Probe-3 Q3 says each "needs a human to split and prepare it". That is accurate about wording, but leaves a reader thinking the design is open when the code has settled it. |
| Backlog 01a0f78f §2 asks whether pid-only liveness "blocks the slice". Meanwhile the slice was built on it and §12 admits the kill(pid,0) mechanism. | No one noticed. |

---

## B. The probes

Grades: C = correct, P = partly correct.

| Q | Probe 1 | Probe 2 | Probe 3 |
|---|---|---|---|
| 1 Goal | C | C | C (cites §1–§10 as the source of the goal; scope is actually only in the collection and backlog) |
| 2 Constraints | C (correctly notes they were not yet binding) | C | C |
| 3 Undecided | C | C (lists 01a0f7d1 as undecided, faithful to engr) | C |
| 4 How far | C | C | C (noticed ddbcb79 was ahead of the sidecar) |
| 5 Criteria met | C (ran the tests) | C | C |
| 6 Not met | C | C | C |
| 7 Next action | C (review attempt 3, then apply) | P: omits that this is attempt 2, the missing detach step, closing item 1 and staging the OPEN lines; its command would attest attempt 1, which is what S3 then did | C (review 01a0f893; item-4 closure only mentioned in Q6) |
| 8 Unrecoverable | C: the reasons for the rework rounds and the review outcomes really are gone | P: says no record of whether 01a0f7d4 was reviewed, but commit 125ae3b and §11's admission show one review was applied | P: the "cold-read notes and meant-note for §27–31" do not exist because that step had not happened yet; nothing was lost |
| 9 Classification | P: calls the review history "should have been recorded"; it is narration the Rule bars, and the one load-bearing fact (attempt count) was recorded | P: attempt status as progress that should have been recorded is fair; the sidecar lag counts as working state | P: calls the not-yet-done cold read "progress that should have been recorded"; it is the next step; the item-4 lag is correctly working state |
| 10 Disagreement | P: missed the false "in flight" and the wrong section numbers | P: caught the sidecar lag, missed backlog 01a0f7d1 §2/§5 being already implemented | C: thorough basis-moved check; missed backlog points already settled in code |

No hallucinations found. All three probes read engr's backlog literally, so all three report decided-and-built design as undecided.

---

## C. Successor sessions

### Reconstruction

**S2**
- First productive act: T6, a review subagent dispatched after 5 turns and 11 calls. Before that it read the hook dump, git log and status, `changeset show`, `backlog show`, `rules show`, a file search confirming no code existed, and `engr show`.
- First engr write: T16, after T7–T15 spent re-learning `changeset apply` and the protocol through help and protocol greps (about 10 more calls).
- First code delegation: T44.
- The reconstruction was correct: "Digest unchanged" (T15), no code exists, the review is pending. S2 effectively ignored the false "in flight" by reviewing itself.

**S3**
- First productive act: T11, a cold read dispatched after 10 turns and 14 calls. Before that it read the git log and the 77e2289 commit message, `changeset show`, `engr show`, ran `cargo test --test session`, read the session.rs diff and commit 125ae3b, and read the rule.
- First engr write: T13. First code delegation: T29.
- The reconstruction was correct: item 1 done, 5 steps pending, OPEN lines recovered from the commit message. It could not know about the lost detach step, and it missed the attempt-2 status.

### Rework

| Session | Cheap re-discovery | Expensive lost reasoning |
|---|---|---|
| S2 | T7–T12, T19–T20: re-learning engr CLI and protocol mechanics S1 had worked out (S1 T11–T22), and re-checking `.engr/` layout and gitignore (S1 T7) | T6 re-reviewed 10 steps S1's attempt-2 reviewer had passed, unchanged; 4 of them flipped to fail (Revision Is UI-Only, Single Owner/Computed Liveness, Scope, Interaction Event Fields). The cause is that S1 never applied a passing review. |
| S3 | T3–T8: re-ran session tests and re-read the session.rs diff and 125ae3b, which S2 had already verified (S2 T49–T53) | None. Its reviews were of new wordings. |

### Re-deciding, guessing and violations

- **S2 reclassified settled decisions as unresolved.** It put 7 decisions in backlog, then told the worker to build from them anyway (T44). That bends AGENTS.md ("Never read what is there as though it were the record") in a labelled, deliberate way.
- **S2 nearly settled an open question with an invented reason.** "Detach Is Always Idempotent" (T74) had been flagged OPEN by the worker; only the cut prevented admission.
- **S2 dropped two done-conditions.** Items 2 and 3 lost their "done when" clauses when rewritten to fit the 160-character limit (T32; metrics-S2 shows `items_with_done_condition: 3` of 5).
- **S3 mis-attested a review attempt.** T13 applied with the default attempt 1 where engr had said attempt 2.
- **Build proceeded past an open question.** All sessions built on pid-only liveness while 01a0f78f §2 left open whether that "blocks the slice".
- No recorded Section was violated in code.

---

## D. Keeping state

### Writes (main agent; worker ChangeSet adds noted)

| | S1 | S2 | S3 | Total |
|---|---|---|---|---|
| Object create | 1 (after 3 refused attempts, one titled "zzz debug test object") | – | – | 1 |
| ChangeSet new/add/rm/apply | 1 / 31 / 14 / 0 | 1 new, 1 discard / 11 + 4 worker / 16 / 2 | 3 / 3 + 15 worker / 3 / 4 | 64 adds, 33 rm, 6 applies |
| Sections admitted | 0 | 11 | 15 | 26 |
| Backlog points (+ revises) | 3 (+2) | 7 | 8 | 18 |
| Sidecar ops | start, 4 item adds, 4 summaries | 2 adds, 6 revises, 1 state, 4 summaries (3 refused for length) | 6 state, 4 result, 3 commit, 3 summary | – |
| Collection | new + add | – | – | never touched again |
| `.engr`-only commits | 4 | 4 | 6 | 14, against 4 code commits |
| Subagents | 1 cold read, 2 reviews | 1 worker, 1 cold read, 2 reviews | 3 workers, 3 cold reads, 4 reviews | 4 workers, 13 readers/reviewers |

**Share of turns** (my classification of 246 coordinator turns):

| Session | Engineering (design research, delegating and verifying code) | Orientation | Engr, reviews and their commits |
|---|---|---|---|
| S1 | about 10 | – | 65; zero code |
| S2 | about 7 | about 15 | about 52 |
| S3 | about 22 | about 10 | about 65 |
| Total | about 16% | about 10% | about 74% |

Worker tool calls are not in these counts. S2 T54–T74 (21 turns) went entirely to rewording seven session.rs Sections that the code already shows.

### Hooks

- **session-start.** Useful. It surfaced the changeset, the sidecar and the backlog. It also delivered S1's false "in flight" line, and S2 did not check it as the hook advised.
- **heartbeat** (S1 T10, S2 T50). In S1 it prompted Object creation before design was drafted ("Let me set up the work sidecar now", T11), which led into 12 turns of fighting `prepare --agent` (T11–T22). In S2 it produced nothing.
- **return-reminder.** It fires when an async agent is *launched* ("a subagent just returned" over "Async agent launched successfully", S1 T29, S2 T44), not when the agent returns. At the real return of the S2 worker there was no prompt. The agent then neither closed item 1 nor staged its OPEN lines before the cut. S3 followed the procedure without help.
- **commit-reminder.** All 12 firings followed the agent's own `.engr` bookkeeping commits. That is noise.
- **stop-check** (5 refusals). In S1 it pushed "review in flight" summaries plus two extra commits (T56–T58, T67–T68). That is working state, and the S1 version became false. In S2 and S3 the agent correctly answered "nothing to write" each time.

Successors used only the item list and the ChangeSets. No hook-driven note carried intent.

### Sections

About half record intent; the rest (§6, §11–§16, §19–§21, §23–§26) restate what the code and tests show. Several post-code reasons were written by the coordinator to satisfy the reviewer rather than taken from the decider:
- S2 T65: the reviewer found "§14 … no 'because' at all".
- S2 T71 then added "because a token identifies only the current attachment…".
- S2 T73 likewise added a new reason for close-once.

**Best (§4):** "ui.update's RFC 6902 patch mechanics are implemented inside this crate rather than through a new external dependency, because the draft itself calls this JSON Patch usage part of a contract that is not yet permanently stable…" It records a rejected alternative and its reason; the absence of a dependency cannot say why.

**Worst (§20):** "…implements only the RFC 6902 add, remove, replace and test operations … PatchOp's serde tag has no move or copy variant, so JSON naming either op fails to deserialize into it at all." That is a description of an enum. Close behind is §11, "16 CSPRNG bytes rendered as 32 lowercase hex characters".

---

## E. Decisions

"First engr" counts any engr write; a ChangeSet step counts.

| # | Decision | Made | First engr | Record / final place | Code-evident? |
|---|---|---|---|---|---|
| 1 | Scope: library + inspection CLI; exclude MCP, Web, catalog, ui.wait, reply-template, ChangeSet integration | S1 T24 | S1 T27 CS; T40 collection | never admitted; backlog 01a0f7d1 §3 (S2) | partly (exclusions and reason no) |
| 2 | UI state under `.engr/local/ui/` | S1 T24 | S1 T27 CS | §1 (S2) | yes |
| 3 | `view_<uuidv7>` | S1 | S1 T27 | §9 (S2) | yes |
| 4 | `sess_`/`evt_<uuidv7>` | S1 | S1 T27 | backlog §5 | yes |
| 5 | Session field set | S1 | S1 T27 | §2 (S2) | yes |
| 6 | Owner = token/pid/opened_at; attached computed, never stored | S1 | S1 T27 | backlog §2 | behaviour yes, reason no |
| 7 | Envelope format/root/elements, `"engr.dynui.v0"` | S1 T42 | S1 T44 | §10 (S2) | yes |
| 8 | Only the envelope is validated | S1 T72 | S1 T73 | backlog §6 | yes |
| 9 | Revision starts at 1; only ui.update increments it | S1 | S1 T27 | backlog §1 | yes |
| 10 | ui.update algorithm (expectedRevision, scratch copy, validate, history) | S1 | S1 T27 | backlog §7 | yes |
| 11 | In-crate JSON Patch, no dependency | S1 T42 | S1 T44 | §4 (S2) | absence yes, reason no |
| 12 | Per-Session sequence from 1, plus view_revision | S1 T60 | S1 T62 | backlog §4 | yes |
| 13 | Stale interaction rejected, no sequence consumed | S1 T60 | S1 T62 | §3 (S2) | yes |
| 14 | Per-Session lock, not the workspace lock | S1 | S1 T27 | §5 (S2) | yes |
| 15 | Temp-file + rename publish | S1 T60 | S1 T62 | §6 (S2) | yes |
| 16 | View interactivity computed | S1 T72 | S1 T73 | §7 (S2) | yes |
| 17 | Session close does not cascade | S1 T72 | S1 T73 | §8 (S2) | yes |
| 18 | Count the review as attempt 3/3 and exhaust 7 steps to backlog | S2 T16 | S2 T22 backlog, T29 sidecar | backlog/sidecar | no |
| 19 | Defer Human-path prepare until admissions finish | S2 T37–40 | S2 T38 sidecar item 5 (reason only in commit 72c5b3d) | sidecar | no |
| 20 | Workers follow backlog 01a0f7d1 as spec | S2 T32/T44 | S2 T32 sidecar "(non-auth)" | sidecar | no |
| 21–26 | Liveness via kill(pid,0); lock blocks; token shape; token rotates; continuation creates a new Session; close once | S2 worker | S2 CS | §11 (S2), §12–§16 (S3) | yes |
| 27 | Detach idempotent even when closed | S2 worker; coordinator T74 | never (draft removed T68, re-add lost at cut) | became open question 01a0f874 §2 (S3) | yes |
| 28–33 | `crate::dynui`; op subset; live-Session check; EXIT_STALE; per-View lock; history as a whole JSON array, not JSONL | S3 worker | S3 CS | §17–§22 (S3) | yes (reasons partly) |
| 34 | Drop the EXIT_SCHEMA claim; record as open | S3 T50/T64 | S3 T64 backlog | 01a0f886 §4 | n/a |
| 35–38 | record takes a View id; events lock; opaque data; history refuses an unknown Session | S3 worker | S3 CS | §23–§26 (S3) | yes |
| 39 | Events file is a whole array at `sessions/<id>.events.json` | S3 worker | never (commit 5ea1db5 only) | – | yes |
| 40–44 | CLI surface; exact ids; token never printed; computed `interactive`; open/closed state | S3 worker | S3 CS (unapplied at cut) | – | yes |
| 45 | Add `session::ids()` to session.rs (deviation) | S3 worker | never (commit ddbcb79 only) | – | yes |

A further 4 implemented behaviours (closed-before-stale order, root not resolved, create allowed under a closed Session, record not refusing a closed Session) were recorded in-session as open questions (01a0f886 §1–3, §5).

**Tally of 45 decisions:**
- 42 first reached engr within the session that made them; none reached it only later.
- 3 never reached engr (#27, #39, #45). The code shows all three; only #27's reason is lost.
- Reaching the *record* was slower:
  - S1's 17: 0 in S1, 10 admitted in S2, 7 never.
  - The S2 worker's 6: 1 in S2, 5 in S3.
  - S3's 15 worker decisions: 10 admitted in S3, 5 left in an unapplied ChangeSet at the cut.

---

## F. Verdict

| Score | Criterion | Reason |
|---|---|---|
| **4/5** | Resume correctness | Both successors reconstructed correctly within 5–10 turns and finished items 1–4, and probes answered 22 of 30 questions correctly and 8 partly, with none wrong. But S2 left the worker's OPEN lines unstaged and nearly admitted an answer to one of them, and S3 mis-attested a review attempt. |
| **3/5** | Intent completeness | Nearly every decision reached engr in-session. But the goal and scope and six other core decisions never entered the record. The human mandate was edited out as "narration". At the S1 cut, all design intent lived in a gitignored, machine-local ChangeSet. |
| **3/5** | Freshness | Nothing engr held caused a wrong action. But every cut left a lagging or false sidecar ("in flight", wrong § numbers, finished items still `active`). 16 "basis moved" flags were never handled. The backlog calls 7 implemented designs "unresolved", and all probes repeated that. |
| **1/5** | Proportion | About 74% of 246 coordinator turns, 13 of 17 subagents and 14 of 18 commits went to keeping the record. S1 spent 75 turns and produced no code. About half the 26 Sections restate the code. |

### Failure modes, most damaging first

1. **The review budget was spent before anything was admitted, so core intent was pushed out of the record into backlog.**
   - S1 ran two Rule Reviews over the whole ChangeSet (T51, T65). It never applied the passing steps; it removed and re-added steps, which changed the digest each time. It ended with 0 Sections.
   - The reviewers were inconsistent: attempt 2 failed 3 steps attempt 1 had passed. S2's "attempt 3" (T6, T16) then failed 4 steps S1's attempt 2 had passed in identical wording. Those 4 include Scope and the owner/liveness rule.
   - Seven decided-and-implemented designs, including the slice's only scope statement, now sit in backlog 01a0f7d1 as "unresolved". Workers were told to build from them (S2 T44).
   - Every probe reports them as undecided (P2 Q3, P3 Q3). The record/backlog distinction engr exists to keep is inverted for this Object.
2. **Bookkeeping crowded out the engineering.**
   - About 74% of coordinator turns went to engr: S1 with zero code, S2 T54–T74 rewording session.rs Sections, 13 reviewer/reader subagents against 4 workers.
   - About half the Sections describe code; §20 describes an enum.
   - Hooks added churn: the return-reminder fires at async launch, and commit-reminders fire on the agent's own `.engr` commits.
3. **Rationale was authored for the reviewer, not recovered from the decider, and a worker's open question was nearly answered by assertion.**
   - After the S2 reviewer said §14 had "no 'because' at all", the coordinator wrote reasons itself (S2 T71, T73). They are now admitted as Agent authority.
   - Meanwhile the worker's OPEN lines went unstaged through the end of S2. The coordinator was adding "Detach Is Always Idempotent … because a caller asking to detach only wants no owner attached afterward" (T74) for a point the worker had flagged OPEN ("whether detach() should refuse on an already-closed Session"). Only the cut stopped it.

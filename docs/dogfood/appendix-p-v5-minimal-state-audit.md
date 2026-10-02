# Audit: run L, Dynamic UI Session/View store, three sessions

Turn numbers are the log's own (`S2 T37`). The hook files count turns one lower than the logs do: hooks-S1 "T28" is the Stop at S1 T29. Per S2 T20 and S3 T35, the agent's task told it to keep no work sidecar and to write no progress just to satisfy a hook. That is why there is no execution memory, and I do not count its absence as a fault.

---

## A. At each cut-off

### S1 (cut at T86 of 86)

**Doing / intending.** S1 delegated the storage-foundation step (§2, §3, §5, §7, §11) at T82, to a worker that ran in an isolated git worktree. The worker committed `bd94ae1` on branch `worktree-agent-a7eab0f88684b3521`. It also drafted ChangeSet `01a0f8ca` with two steps: "View Revision Starts At Zero" and "Session Close Does Not Touch The Attachment Lock". It created that ChangeSet inside the worktree, so the ChangeSet lives in the worktree's own `.engr/local`. At T85 the agent confirmed the branch, and at T86 it said "Let's fast-forward merge it in". The session was cut before the merge. The next items it had in mind were ui.update, then interaction events, then the CLI.

**Decided in S1.** The Object `01a0f8a7` has 15 Sections: scope §1, storage location §2, the per-Session OS lock §3, stale interactions §4, session-close reach §5, the json-patch `=4.2.0` pin §6, the opaque document §11, no catalog validation §15, and the acceptance criteria §7–§10 and §12–§14. They took T14–T79.

**Ruled out in S1.** Heartbeat/liveness detection (§3), per-View counters (§10), rewriting Views on close (§5), schema or catalog validation (§11, §15), and a "No Document Schema" Section (T78, judged redundant).

**Found in S1.** json-patch 4.2.0's `patch()` reverts on failure (T26). Only 6 dependencies are exact-pinned (T33, which corrected a false claim the cold read caught).

**Left open in S1.** Retention, as backlog `01a0f8ab`.

**Lost intent** (needed, not recoverable from the repository, not in engr as seen from the main checkout):

| Item | Class | Misled anyone? |
|---|---|---|
| The worker's two decisions: View revision starts at 0, and `session::close` never touches the attach lock. They sat in a ChangeSet inside the worktree's `.engr/local`, and state-S1 shows `engr changeset ls` → "no ChangeSets on this machine". S3 T15 destroyed them with the worktree. | decision | Nobody was misled into a wrong answer. S2 re-made both decisions the same way: §16, and the "first expectedRevision is 0" draft. That was paid-for rework. |
| Why this slice is being built now, even though `docs/issues/40.md` says "Not implementation-ready". That came from the task; nothing records it. | goal / rationale | Probe-1 flagged it as unrecoverable (Q8). No successor was misled. |
| The finding that `patch()` already reverts on failure. | rationale (minor) | No. S2's T56 worker re-read the crate source and cloned the document anyway. |

The unmerged finished branch itself is **working state**, so I do not count it here. It was still the costliest gap in the run (see C and F).

**Stale state.**
- §6 says json-patch is "pinned to the exact version `=4.2.0` in crates/engr/Cargo.toml". Cargo.toml had no json-patch at the cut (probe-1 Q4; S2 T5 grep came back empty). It misled no one, because both readers checked Cargo.toml.
- Nothing else engr held contradicted the repository. engr was silent about the worktree rather than wrong about it.

### S2 (cut at T75 of 75)

**Doing / intending.** The T74 reviewer passed the reworded §21 ("first expectedRevision is 0") and §22 ("history reconstructs without replay"), and failed §23 ("no trimming"). At T75 the agent pulled the digest in order to apply with `--failed-step 3`. After that it would have fixed §23, committed `.engr` (§20 was admitted but uncommitted), and delegated interaction events and then the CLI.

**Decided in S2.**
- §16: the attach lock is not a mutation lock.
- §17: only `require_writable`, which re-reads from disk, decides writability.
- §18: ids are plain UUIDv7.
- §19: `close_view` writes only the View's own file.
- §20: stale revision returns EXIT_STALE; every other refusal returns EXIT_INVARIANT.
- Drafted but not admitted: a new View's revision is 0, history entries are whole documents, history is unbounded.
- Patch atomicity works by applying to a clone, and the file layout is `session.json` / `attach.lock` / `views/<id>.json`. Both are in commit messages only.
- Left open: `create_view` on a closed Session (backlog `01a0f8e0`).

**Lost intent:**

| Item | Class | Misled? |
|---|---|---|
| The T74 review verdict (§21 PASS, §22 PASS, §23 FAIL). This is borderline: it is the output of reasoning, not intent proper. | other (review outcome) | Yes. Probe-2 Q3 said "nothing has reviewed or applied it yet". S3 re-ran the review (T18), got the opposite verdict, and discarded all three steps (T26–T27). |
| The worker's OPEN line (S2 T20): "whether Session/View ids should later gain a compact `engr:` reference encoding for CLI addressing … is left to the CLI step". It was not staged. §18 settled it as a decision. | unresolved question | Not yet, because the CLI was never built. A CLI author will read §18 as settled. |
| The S1 worker's two decisions. They were still only in the worktree. | decision | Same as at S1. |
| Why the slice is being built now. | goal / rationale | No. |

**Stale state.**
- ChangeSet `01a0f8e6` showed "NEEDS REVIEW" although a review had already returned. The repository cannot contradict this, but the status had moved, and the stale status misled probe-2 and S3.
- §1–§15 show "basis moved". engr computed that itself and it is accurate.

### S3 (ended at T87, "success")

**Doing / intending.** S3 had just staged backlog `01a0fa18` and committed `c533d0b`. It reported Human candidate `VKRB3E` for a human to settle, and said its next step was to delegate the CLI (§12–§14).

**Decided in S3:**
- Discard S2's three drafts as redundant (T26–T27).
- Delete S1's worktree as superseded (T14–T15).
- §21: sequence = history length + 1, with FIRST_SEQUENCE = 1.
- §22: payload is opaque JSON.
- One shared `events.json` per Session. Three reviews failed it, so it went out as Human candidate `VKRB3E` plus backlog `01a0fa18`.
- Escalate the exhausted review instead of confirming it.

**Lost intent.** Little. The gap about why the slice is being built now remains. The discarded S2 decisions (revision 0, whole-document history, unbounded history) are all visible in `session.rs`.

**Stale state.** None found. Probe-3 agrees.

---

## B. The probes

Grades: C = correct, P = partly correct, W = wrong, H = hallucinated.

| Q | Probe 1 | Probe 2 | Probe 3 |
|---|---|---|---|
| 1 goal | C | C | C |
| 2 constraints | C | C | C. It attributes the closed-check partly to §16, which is about the lock; minor. |
| 3 undecided | C (retention, plus the draft's own caveats) | P. Backlogs right, but "nothing has reviewed" the ChangeSet is wrong: S2 T74 had reviewed it. | C (3 backlogs plus VKRB3E) |
| 4 progress | C. Found unmerged `bd94ae1`, its scope, and the missing json-patch. | P. Code state right, but it never mentions the leftover worktree/branch, which still showed as `?? .claude/worktrees/`. | C |
| 5 ACs met | C (§7, and only on the side branch) | C (§7–§9) | P. §7–§10 right, but "all 63 `session.rs` tests" is H (the session binary has 14; 63 is another binary's count). |
| 6 ACs not met | C | C (leaves out §4's check) | C |
| 7 next action | P. Merge the branch, which was the agent's actual next step, but it puts a sidecar write first and misses the worker's ChangeSet inside the worktree. | P. Commit §20, then review/apply. The agent's actual next step was to apply the review it already had; the probe's route forces a re-review, which is what S3 then did. | C. CLI, plus VKRB3E as a human handoff. It does not note that admitting more Sections would void VKRB3E. |
| 8 unrecoverable | C. The "Not implementation-ready" mandate truly is absent. It missed that the worker's decisions were reachable only by running engr inside the worktree. | C. The review state truly was unrecoverable. | Mostly C. Only the mandate gap remains. |
| 9 classification | C. Mandate = intent; worktree = recoverable progress. | C. Progress. But it was progress whose loss forced an expensive repeat. | C (N/A) |
| 10 engr vs repo | P. Rightly says engr has no record of the worktree work, but does not flag §6's present-tense Cargo.toml claim as false. | P. Misses the stale worktree and the moved review status. | C |

Probe-1 was the strongest reconstruction in the run. In 6 of its answers it found what S2 then failed to find.

---

## C. Successor sessions

### S2

**Reconstruction.** 14 turns and about 17 tool calls (T1–T14):
- `engr ls`, `changeset ls`, `backlog show`, `engr show`
- `git log`/`status`
- a `find` for `*session*` on the current branch
- a grep of Cargo.toml for json-patch
- read `40.md` twice, then `store.rs`, Cargo.toml, `changeset --help`, `rules show`, `lib.rs`

T15 opened a ChangeSet and T16 delegated a worker.

**Incorrect.** S2 concluded that no implementation existed. It never ran `git branch` or `git worktree list`. At T19–T20 it looked at `.claude/worktrees/agent-a7eab0f88684b3521`, whose mtime is 18:42, before S2 began, and called it "the background worker's isolated workspace". But S2's own worker was editing `/srv/engr/repo` directly.

The T16 brief delegated **exactly** the step S1 had finished: "Build only what Sections §2, §3, §5, §7 and §11 … describe".

**Rework.**
- **Expensive lost reasoning:**
  - T16: the whole storage foundation was re-implemented. The worker made about 50 tool calls, against S1's 47-call worker, whose 652 lines were then thrown away.
  - The S1 worker's two decisions were re-made: §16 (via T26–T27) and the T56 draft "first expectedRevision is 0".
  - The T56 worker re-investigated json-patch's atomicity.
- **Cheap re-discovery:** T2–T14 (Object, git, draft, store.rs, CLI help), and T21–T24 and T59–T63 checking worker output, which is proper verification rather than waste.

**Re-decided, guessed or violated.** S2 re-decided S1's two worker decisions, which were not in the main engr store, and reached the same answers. It guessed nothing human-owned. Retention was left alone. The id-encoding question was closed by agent decision §18.

**It violated a recorded constraint.** The rule `record-mss` allows 3 attempts, and on exhaustion requires human confirmation.
- §19 failed review at T26, T36 and T42 and passed at T45.
- It was applied at T47 with `--review-attempt 1`, although engr had said "review again at attempt 2" (T27, T39).
- The T36 reasoning was "it's a different wording now, so it's a fresh attempt 1".

### S3

**Reconstruction.** 9 turns and about 18 tool calls (T1–T9):
- four skills, `engr show`, the backlogs, `changeset show`
- `git log`/`status`/`diff` (which found the uncommitted §20), `rules show`
- `cargo test`/`fmt`, read `session.rs`, read `40.md` again, grep `main.rs`

The first productive act was T10/T16, committing the admitted §20. The first review delegation was T18, and the first engineering delegation was T30.

**Mostly correct.** S3 correctly identified the uncommitted §20, the pending ChangeSet, and that interaction events and the CLI remained. It inspected the worktree (T11–T14) and correctly judged it superseded by `cd798c1`, but mislabeled it "an earlier, abandoned subagent run" (T14). It did not know that ChangeSet `01a0f8e6` had already been reviewed.

**Rework.**
- **Expensive lost reasoning:** T17–T18 and T26–T27 repeated S2's T74 review. The repeat **reversed** it: §21 and §22 had passed and now failed all three. S3 recorded `--review-attempt 1` where engr had said attempt 2, then discarded the drafts.
- **Cheap re-discovery:** T3–T9 (state, tests, `session.rs`, a third reading of `40.md`) and T11–T14 (worktree inspection).
- **Friction, not rework:** T66–T80, 15 turns reading engr's source to learn how to escalate an exhausted review.

**Re-decided, guessed or violated.** S3 re-decided S2's drafts (record-or-not, not design). It guessed nothing: `create_view` and retention were left open, and the exhausted step was sent to a human. It honoured the attempt ceiling, using attempts 1, 2 and 3, then exhausted → VKRB3E. That is the opposite of S1 and S2.

---

## D. Keeping state

**Writes to engr, by kind.**

| Session | Sections admitted | ChangeSet steps added / removed | ChangeSets new / discarded | Backlog | Sidecar | Collection | Other |
|---|---|---|---|---|---|---|---|
| S1 | 15 (+ Object create) | 51 / 5 (+2 lost in worktree) | 3 / 3 (+1 lost) | 1 new | 0 | 0 | — |
| S2 | 5 | 16 / 8 | 2 / 0 | 1 new, 1 revise | 0 | 0 | — |
| S3 | 2 | 6 / 4 | 1 / 2 | 1 new | 0 | 0 | 1 Human candidate; 3 refused `prepare` calls |
| **Total** | **22** | **73 / 17** | 6 / 5 | 3 points | 0 | 0 | |

**Subagents.**
- 15 rule reviews: S1 5, S2 6, S3 4.
- 3 cold reads: S1 T21, S3 T53, T58.
- 4 implementation workers: S1 T82 (wasted), S2 T16, S2 T56, S3 T30.

73 step-adds and 18 review or cold-read subagents went into admitting 22 Sections (1,619 words).

**Share of turns.** The metrics count engr-write turns as 20/86, 20/75 and 19/87, which is 59 of 248 (24%). That understates the cost. If drafting, reviewing, waiting on reviews, rewording, applying, committing `.engr` and reading engr's own source are counted, keeping engr took about 70/86 (S1), 44/75 (S2) and 60/87 (S3): **about 174 of 248 turns (70%)**.

Engineering by the coordinator took about 29 turns (12%): researching, delegating and verifying workers. Three workers landed about 1,260 lines across 3 of the 4 planned steps. The fourth step, the CLI (§12–§14), was never started.

S3 alone spent T38–T86, 49 turns, on one Section (one `events.json` per Session), and it ended unadmitted.

**Hooks.** There were 3 session-start, 7 heartbeat, 22 return-reminder, 15 stop-check refusal and 5 commit-reminder hooks.

- **Stop-check** asked for drafted ChangeSets to be applied or discarded, items closed and the summary rewritten. All 15 were declined with a one-line "still in flight, not a handoff" (e.g. S1 T30, T46; S3 T35, T41), so they produced nothing. Three of them named the actual hazard: "the repository moved and no execution memory exists: ?? .claude/worktrees/" (S1 T83, S2 T18, S2 T57). Each time the agent dismissed the worktree as expected or as its own.
- **Return-reminder** was mistimed. Workers and most reviewers ran async, so 16 of the 22 fired at launch ("Async agent launched successfully"), before anything had returned. Its "stage its OPEN lines" instruction lines up with S2 T50, the `create_view` backlog point that successors and probes later read. But S2 staged only one of the two OPEN lines.
- **Heartbeat** fired 7 times and **commit-reminder** 5 times. Commit-reminder asked to "close the work item", and there were no items. Neither produced anything a successor used.
- No hook got a non-intent write out of the agent: it refused the working-state requests (summary, items) as instructed. The hooks were built around the sidecar the task forbade, so about 15 turns went to refusing them.

**Do the Sections record intent?**
- §1–§15 were written before any code. They are intent: scope, decisions with reasons and rejected alternatives, and checkable acceptance criteria. §12–§14 have thin reasons ("because §1 scopes this Object to include a small inspection CLI").
- §16–§22 were written by workers after their code, and mostly describe it: lock usage, which function to call, id format, which file `close_view` writes, exit codes, derived sequence, opaque payload. §17 ("must call `require_writable`") and §20 (retry semantics) carry some rationale for future callers.

**Best Section (§3).** It records a decision, the reason for it, and the approach it ruled out:

> "an OS advisory lock already releases itself when the holding process exits — which is what lets a lost process be observed as detached without a heartbeat or liveness timestamp."

**Worst Section (§19).** It describes one function's write set, took 4 reviews, and was admitted past the ceiling:

> "session::close_view writes only the View's own stored file and never touches its Session's file, because §5's writability check reads the Session's closed_at and the View's closed_at as two independent fields … a cross-write would only duplicate state require_writable already reads separately."

**Runner-up worst (§6).** It asserted a pin in Cargo.toml that did not exist yet.

---

## E. Decisions, one by one

| # | Decision | Made | First reached engr | Code alone? |
|---|---|---|---|---|
| 1 | Scope: library + small CLI; no mcp/web/renderer/ChangeSet | S1 T14 | S1 T19 step → §1 T58 | No |
| 2 | Storage under `.engr/local/sessions/` | S1 T14 | §2 T58 | Location yes, reason no |
| 3 | Per-Session OS lock, no heartbeat | S1 T14 | §3 T58 | Mechanism yes, rejected alternative no |
| 4 | Stale interaction rejected, never appended or sequenced | S1 T14 | §4 T58 | Yes after S3 |
| 5 | Session close doesn't rewrite Views; check at mutation | S1 T14 | §5 T58 | Yes |
| 6 | json-patch exact `=4.2.0` | S1 T3/T14 | §6 T58 | Pin yes (after S2), why exact no |
| 7 | One Session-wide sequence, not per View | S1 T14 | §10 T58 | Yes after S3 |
| 8 | Document opaque `serde_json::Value` | S1 T14 | §11 T70 | Yes |
| 9 | No catalog validation | S1 T14 | §15 T78 | Absence only |
| 10 | CLI = `ls` / `show` / `events` | S1 T65 | §12–§14 T70 | No (never built) |
| 11 | Retention deferred; keep everything | S1 T40 | backlog T42 | Absence only |
| 12 | New View revision = 0 | S1 T82 worker | S1 worktree ChangeSet (invisible, later destroyed); again S2 T56 draft; discarded S3 T27; never admitted | Yes |
| 13 | close / record ops never take the attach lock | S1 T82 worker | worktree ChangeSet (lost) → **later**: §16 S2 T27 | Partly |
| 14 | Only `require_writable` decides writability | S2 T16 worker | §17 S2 T39 | Partly (doc comment) |
| 15 | Ids plain UUIDv7, no `engr:` form | S2 T16 worker | §18 S2 T39 | Yes |
| 16 | `close_view` writes only its own file | S2 T16 worker | §19 S2 T47 | Yes |
| 17 | File layout `session.json` / `attach.lock` / `views/<id>.json` | S2 T16 worker | never (commit message) | Yes |
| 18 | History entries are whole documents | S2 T56 worker | S2 draft; passed T74; discarded S3 T27 | Yes |
| 19 | History unbounded, no trimming | S2 T56 worker | S2 draft; discarded S3 T27 | Yes |
| 20 | EXIT_STALE vs EXIT_INVARIANT | S2 T56 worker | §20 S2 T68 | Codes yes, retry rationale no |
| 21 | Atomic apply by patching a clone | S2 T56 worker | never (commit message) | Yes |
| 22 | Sequence = history length + 1, no stored counter | S3 T30 worker | §21 S3 T42 | Yes |
| 23 | One `events.json` per Session | S3 T30 worker | candidate VKRB3E + backlog S3 T82/T84; not admitted | Yes |
| 24 | Interaction payload opaque | S3 T30 worker | §22 S3 T42 | Yes |

**Tally.** Of the 24 decisions:
- **21 reached engr within the session they were made in.** 16 as admitted Sections, 1 as backlog, 1 as a pending candidate, and 3 as ChangeSet drafts (#12, #18, #19) that were later discarded. #12's first draft was invisible from the start.
- **1 reached engr later** (#13, re-made in S2).
- **2 never reached engr** (#17, #21). The code makes both evident.

The 3 discarded drafts are also evident in the code. The decisions that matter for intent (#1–#11) all reached engr in S1, before any code existed.

---

## F. Verdict

1. **Resume correctness: 3/5.** Goals, constraints and open questions carried cleanly. But S2 misread the true state, missing a finished branch, and redid a whole step. S3 repeated and reversed a finished review. S2 broke the recorded review ceiling.
2. **Intent completeness: 4/5.** Scope, rationale, rejected approaches and checkable criteria were in engr before any code. The only losses were two worker decisions stranded in a worktree-local store, one unstaged worker question, and the "why now" mandate.
3. **Freshness: 4/5.** §6 asserted a pin before the pin existed, which misled no one. A ChangeSet still reading "NEEDS REVIEW" after its review did mislead probe-2 and S3.
4. **Proportion: 1/5.** About 70% of 248 turns, 73 step-adds and 18 review/cold-read subagents went to 22 Sections, 7 of which restate code. One Section ate 49 turns of S3. A third of the acceptance criteria (the CLI) was never started.

**Failure modes, most damaging first.**

1. **Delegated work left unrecorded and unmerged in a separate worktree, then redone.**
   - S1 delegated at T82 of 86. The worker's commit `bd94ae1` and its decisions (ChangeSet `01a0f8ca`, in the worktree's own `.engr/local`) were invisible to the main workspace. state-S1: `engr changeset ls` → "no ChangeSets on this machine".
   - S2 took the leftover worktree for its own worker's (T20) and re-delegated the identical §2/§3/§5/§7/§11 step (T16).
   - S3 deleted the branch as "abandoned" (T14–T15).
   - The Stop hook named `?? .claude/worktrees/` three times and was dismissed each time. Probe-1 found the branch cold.
2. **Review overhead swamped the engineering.**
   - S1 rebuilt its ChangeSet three times (51 step-adds) for 15 Sections.
   - S2 took four reviews to admit §19.
   - S3 spent T38–T86 on a code-describing layout Section and ended with Human candidate VKRB3E.
   - Workers turned every implementation detail into a Section (§16–§22), each feeding the same loop, while the CLI never started.
3. **Review state and attempt counts were kept only in the agent's context.**
   - S2's T74 verdict was lost at the cut. S3 re-reviewed (T18), reversed it, and discarded the steps (T26–T27).
   - Attempt numbers were repeatedly reset: S1 T64 "starts a fresh attempt count" right after calling T61 the "final autonomous" attempt; S2 T36/T47 admitted §19 on its 4th review as "attempt 1"; S3 T26 used attempt 1 where engr said attempt 2.
   - The admitted history therefore understates its own review attempts, and twice bypassed the rule's human-confirmation ceiling.

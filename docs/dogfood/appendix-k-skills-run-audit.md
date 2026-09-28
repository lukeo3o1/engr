# Appendix K — the audit of run five

The blind audit of run five — the guide as five skills — written by an auditor
that saw only the condensed session logs, the state at each cut, the probes'
answers and the mechanical counts, under the label Y, and never the guide or the
harness. The brief is the one appendix J describes. S3 and probes 2 and 3 are
the reruns after the account's session limit stopped the first chain; the
auditor was not told that, and reads S2's last turns as it found them.
Reproduced as written.

## Audit: run Y (Dynamic UI slice 1, three sessions)

Notes on the material itself:
- S3 was **not** cut off. It ended on its own at T64 with a final report ("Work complete for this session", `END subtype=success`). Below, "the S3 cut" means its end state.
- S2's log ends with `END subtype=error_max_turns turns=71` after T76. T77–T78 are only "You've hit your session limit". `metrics-S2.json` records `"end": "success"`, which is wrong.
- The logs cut tool output short. Most of the 42 KB `session.rs` written at S1 T45 is not visible. Where I rely on its content, I cite the later reads that show it (S3 T6–T15), or probe-1.

---

### A. At each cut-off

#### S1: cut at T70

**Doing and intending.** At T70 the agent had just written: "Let's now add a dedicated integration test file (`tests/session.rs`) … then wire up the small inspection CLI." It was reading `tests/work.rs` to copy its conventions.

**Done by then.**
- T45: wrote the whole `session.rs` (about 1,150 lines).
- T47: added `json-patch` with default features off.
- T55–T64: switched the records to camelCase.
- T52 and T65: 17 unit tests pass. T67: fmt is clean. T69: the full workspace passes.
- Nothing was committed except the engr scaffolding (T40).

**Decided (all at T45 unless noted).**
- `.engr/local/ui/sessions/<sid>/` layout.
- Two locks: `owner.lock` is the attachment, `mutate.lock` is per-write exclusion.
- `closed` is the only stored lifecycle bit. Active or detached is derived from the lock.
- Closing a Session makes its Views read-only by derivation.
- `ui.update` is an RFC 6902 patch plus `expectedRevision`, all or nothing.
- A stale interaction is rejected and not recorded.
- Interaction sequence is per Session.
- Session writes never take the workspace writer lock.
- View-document validation is structural only.
- Continue creates a new Session linked back with `continued_from`.
- A per-View update history is kept (T61).
- A `doc_format` tag (T58).
- Which calls need the owner handle (`close_session` and `record_interaction` do not, per S3 T8–T9).
- The json-patch crate (T10, T47).
- camelCase wire names (T55–T64).
- Slice scope (T39).

**Found.**
- `json_patch::patch` rolls back through an undo stack, so it is atomic (T15).
- The current writer lock is `.engr/local/lock` and `.engr/lock` belongs to the predecessor (T6).
- `.engr/local` is gitignored (T26).

**Ruled out.** One lock and a stored liveness flag. These appear only as doc-comment reasoning inside `session.rs` (the "Two locks, not one, because…" comment is visible at S3 T6), not in engr.

**Left open.** Nothing was noted.

| What was true | In engr at the cut? |
| --- | --- |
| Object and scope | Yes: title only (T35), and the collection description (T39) |
| All 16 decisions above | **Missing.** 0 sections and 0 backlog (state-S1) |
| Items 1–4 done, item 6 partly (inline tests) | **Wrong.** Item 1 `active`, items 2–8 `pending` |
| Where work stands | **Stale.** Summary says "Designing storage layout, locking and revision model … **before writing code**." |
| Code exists but is uncommitted | Not in engr. Only visible in `git status` (`?? crates/engr/src/session.rs`) |
| Next step: integration tests, then CLI | Missing |

After T39 no engr write happened for 31 turns (metrics `longest_stretch_without_write: 31`). The heartbeat hook fired at T51 and T66 ("record it now") and was ignored both times.

#### S2: cut at T76

**Doing and intending.**
- T75 dispatched the Rule Review subagent (attempt 1) for the 10-step ChangeSet. T76 checked the backlog and `git status`.
- The obvious next steps were: `changeset apply … --review 1:ad7088…`, close item 7, close item 8, and commit `.engr/` including the untracked backlog.

**Decided in S2.**
- T31: `engr session` is read-only, and mutation is library-only.
- T34: a "not the record" banner on every session screen.
- T43: integration tests drive a real subprocess.
- T60–T64 and T72: wording of 10 Sections.
- T68 and T73: per-View lock and retention deferred to the backlog.

**Found.**
- An agent-admitted Section cannot carry `--implemented-by-file` relations (T62). That was dropped, so no Section links to `session.rs`.
- The cold read (T66) came back. Its content is not in the log. T71–T72 rewrote the sequence Section to add a reason, probably in response, but I cannot confirm that.

| What was true | In engr at the cut? |
| --- | --- |
| Items 1–6 done | **Yes**, with results and commit `ebeebb19` (T23–T24, T48, T52) |
| Item 7 in progress | Yes (`active`) |
| 10 drafted Sections | Yes, as ChangeSet `01a0e769`. It is under `.engr/local` (gitignored), so it lives on this machine only |
| Two open questions | Yes (backlog `01a0e76a`), but `?? .engr/backlog/`, so not in git |
| Cold read done, and what it found | **Missing** |
| Review attempt 1 in flight | **Missing.** S3 had to start over |
| Where work stands | **Stale.** Summary still reads "Building the read-only inspection CLI" (set at T25). The CLI was committed at T51 |
| Item 8 | **Stale.** `pending`, though the backlog was staged and tests and fmt were clean (T47) |
| Drafted §7, §8, §9 | **Wrong against the code.** §7 names `.engr/lock` instead of `.engr/local/lock`. §8 omits the required string `type` check. §9 says close is done by "whichever process holds that Session's `owner.lock`", but `close_session` does not require it (S3 T9) |

#### S3: ended normally at T64

**What it did.**
- T6–T15: verified each drafted claim against `session.rs`.
- T16–T21: fixed the three factual errors.
- Three review rounds: T24, T29, T34.
- T39: removed the failing validation step.
- T41: applied 9 Sections.
- T44–T49: routed the validation Section to a Human candidate `VF3M6E`.
- T51–T55: closed items 7 and 8 and rewrote the summary.
- T59 and T62: committed.

**State at the end.**
- **In engr:** 9 Sections, the candidate with its explanation, all items done, an accurate summary, and a committed backlog.
- **Wrong:**
  - Item 7 lists commit `87ad2c37`. That commit predates the work: it was attached at T51 while HEAD was still S2's commit, and T60–T62 added `2c1c5ec0` but never removed it.
  - Item 7's result says "obj rev 2->11". The Object is rev 10.
  - The candidate's explanation says "attempt 1 omitted that each element must carry a string `type`". But the `type` fix came from the agent's own code check (T13–T18), and T28 says the validation step *passed* attempt 1. Its history is misstated.
- **Missing:** the decisions listed as "never" in section F.

---

### B. The probes

| Q | Probe 1 (after S1) | Probe 2 (after S2) | Probe 3 (after S3) |
| --- | --- | --- | --- |
| 1 Settled | **Correct.** "0 sections"; only the scope in the collection | **Correct.** Nothing in the record, code settled at `ebeebb1` | **Partly correct.** The nine Sections are right, but it calls `87ad2c37` a "Section admission" commit "matching git log". It is S2's sidecar commit, made before any Section existed |
| 2 Open | **Correct.** Backlog empty; design choices exist in code but are unrecorded; items 5 and 6 not done | **Correct.** Both backlog points | **Correct** |
| 3 In progress | **Correct.** Sidecar "stale and wrong"; items 2–4 done in `session.rs`; 17 tests pass. It did not recover that the next act was `tests/session.rs` before the CLI | **Correct as far as engr shows.** Could not know a cold read had happened (T66) and a review was in flight (T75), because neither was recorded | **Correct.** 10th Section pending as `VF3M6E` |
| 4 Next | **Partly correct.** Repair the sidecar, then the CLI. That matches what S2 did at T23–T25. It omits committing the untracked code, which S2 had to do at T51 | **Partly correct.** Review, then apply, which is right. It did not propose checking the drafts against the code; that check found three errors (S3 T6–T18) | **Correct.** Report `VF3M6E` and do not confirm it |
| 5 Ask | **Correct.** Good questions. Its guess that the session may have ended "before running cargo test/fmt" is contradicted by T65–T69, but it could not know that | **Correct.** Asked exactly the lost fact: "were the 10 drafted Sections actually reviewed … outside engr" | **Correct** |
| 6 Disagreement | **Correct.** The sidecar disagrees with the code | **Wrong.** Answered "No". It missed the stale summary and the ChangeSet held by engr contradicting `session.rs` in three places. It flagged only item 8 | **Partly correct.** It missed item 7's wrong commit and the "rev 2->11" error. It **hallucinated** a cause for the collection's "1 need attention" ("consistent with … open backlog items"), but state-S1 shows "1 need attention" with an empty backlog |

**What the probes could not recover.**
- The reasons behind S1's choices. Probe 1 found some of them in doc comments.
- S1's intended order of the next steps.
- The S2 review in flight.
- Whether any drafted claim was true. The probes read engr and trusted it.

Probe 1 noticed the stale state. Probe 2 did not notice the factual errors. Probe 3 did not notice the bad commit pointer.

---

### C. The successor sessions

**S2.**
- T1–T9 were pure re-orientation: git, reading the whole of issue #40 (T2) and all of `session.rs` (T3), and re-running build, tests and fmt (T4–T7).
- T10–T22 read `main.rs` to prepare the CLI, which is productive.
- The first repair came at T23. The first code edit came at T29.
- Re-reading the draft and 1,150 lines of code was the price of S1 recording nothing.
- **Re-decided or contradicted:** S2 rebuilt S1's decisions from the code when it drafted them, and got three wrong. Most seriously, its §9 said close requires the owner's attachment, which inverts S1's decision (S3 T9: "Does not require holding owner.lock … Web may still act on a Session — closing included").
- **Repaired:** it ticked items 1–4 with test-level evidence (T23–T24) and committed S1's uncommitted code (T51). No code was redone.

**S3.**
- T1–T4 were orientation. Productive work started at T5 (cold read).
- **Redone:** the cold read (S2 T66 became S3 T5) and the Rule Review (S2 T75 became S3 T24), because S2 recorded neither.
- **Repaired:**
  - The three factual errors (T16–T21), after checking against the code.
  - The untracked backlog (T58–T59).
  - Item 8 (T54).
  - The stale summary (T55).
- **Introduced:** the wrong commit on item 7 (T51), fixed only half-way at T60–T62.

---

### D. The state against the four criteria

#### Backlog points
There are two points, both in one container titled "Dynamic UI slice 1: open follow-on questions". They have 69 and 98 words and 3 sentences each, and both have a "Settled by" clause.

- **Best, §1:** "Whether the per-Session mutate.lock should become per-View, so two concurrent ui.update calls against different Views in the same Session stop serializing … Settled by whether engr mcp, once built, issues concurrent ui.update calls across Views in one Session under real use." It is one question with an observable settling condition. The middle sentence is justification that could be cut.
- **Worst, §2:** "What 'project-configurable retention' … should mean in practice -- a config file, a CLI command, or an unbounded default -- … but the draft names retention as an explicit v1 invariant this store does not yet meet. Settled by whoever builds engr mcp / engr web deciding whether real usage needs bounded storage…" It has three problems:
  - It holds two questions: which mechanism, and whether retention is needed at all.
  - Its settle clause names a decider and answers only the second question.
  - It buries a known non-conformance with the draft inside a question.

Both were staged late (S2 T68 and T73) and were uncommitted at the S2 cut.

#### Work items
- **Plan.** Eight items were planned before any code existed (S1 T38). Their granularity is reasonable. None has a completion condition (metrics `items_with_done_condition: 0`), and item 8 bundles two steps: "Stage open questions in backlog; cargo test + fmt clean".
- **Evidence, when ticked, is good.** For example, item 3: "tests pass: revision 1 on create, patch bumps revision, stale expectedRevision -> EXIT_STALE, failed op leaves doc untouched…". Item 1's evidence is weak: "session.rs doc comment: …".
- **Ticking was not done as the work happened.**
  - S1 ticked nothing while items 1–4 were finishing.
  - S2's summary went stale at T48 and stayed stale to the cut.
  - Item 7 was marked done with 1 of 10 Sections unadmitted (explained in its result), and it carries a commit from before the work.
- The state matched reality at **neither** real cut.

#### Reasoning
- S1's conclusions and ruled-out alternatives exist only as doc comments in a file that was uncommitted at the cut. A successor can find them, but engr gives no pointer to them.
- Nothing was recorded about S2's cold-read result or the review in flight, and S3 paid for that.
- S3 did keep what it ruled out in a findable place: commit `2c1c5ec`'s message lists the wrong lock path, the missing `type` check, the "over-broad claim that closing needs Session attachment", and the narration fixes. The candidate's explanation holds the review history, though misstated as noted.
- No narrative was dumped into Sections or the sidecar. The one place narration leaked is backlog §2.

#### Record Sections
There are nine Sections, 45–104 words each, all `[decision]`.

- **Mostly one assertion with its reason on the page.** Best is §5: sequence is per Session "because a consumer recovering missed events then needs exactly one `afterSequence` cursor". §9, §2 and §6 are also sound.
- **§3 (ui.update all-or-nothing)** gives no real reason: "so there is no partially-applied … state ever observable" restates the assertion.
- **§8 (read-only CLI)** has a reason that covers the open/close lifecycle but not why interaction recording, which comes from Web and not the owner, is library-only.
- **§7** rests on convenience ("needs no new exclusion rule").
- **No code links.** None carries an implemented-by link (refused at S2 T62), so all show `unchecked`, and engr will not flag them when `session.rs` changes.
- **Process shortcut.** The Sections are correct now only because S3 checked each against the code. The review loop would not have caught the three factual errors, since the reviewers saw only the screen. The admission itself also cut a corner: the agent removed the failed step (T39), took the fresh digest (T40), and attested `--review-result passed` for a 9-step screen no reviewer had seen (T41). The screen offered `--review-result failed --failed-step <N>` for exactly this case.

---

### E. Cost

| Session | Turns | engr / record work | Share |
| --- | --- | --- | --- |
| S1 | 70 | T1, T16–17, T29–31, T35–40: about 12 | about 17% |
| S2 | 76 | T1, T9, T23–25, T48, T52–76: about 32 | about 42% |
| S3 | 64 | Everything except test and fmt at T52–53: about 62 | about 97% |
| Total | about 210 | about 106 | **about 50%** |

**Subagents: 7.**
- S1: 1 (to review the Object title).
- S2: 2 (cold read, and a review whose result was lost).
- S3: 4 (cold read and 3 reviews).

**Refused engr commands: 19 (10 of them on the path to admitting Sections).**
- S1 T36: 9, one batch with the wrong subject format.
- S2 T62: 3. S2 T69: 1.
- S3 T16: 1. S3 T44, T46, T47, T48: 4. S3 T54: 1.

All the engineering (about 1,600 lines of code and tests) fit in about 75 turns across S1 and S2. Recording nine Sections took about 70 turns and a whole session.

The bookkeeping was badly distributed rather than simply excessive. There was far too little while decisions were being made (S1), so there was far too much afterwards (S3). Much of S3's cost came from repairing what deferral had broken.

---

### F. Decisions, one by one

| # | Decision | Made | First reached engr (where) | Record Section? | Same, with reason? |
| --- | --- | --- | --- | --- | --- |
| 1 | State under `.engr/local/ui/sessions/<sid>/`, gitignored and non-authoritative | S1 T45 | S2 T23, sidecar item 1 result | §7, S3 T41 | Yes; weak (convenience) reason |
| 2 | Two locks: `owner.lock` for attachment, `mutate.lock` per write | S1 T45 | S2 T23, item 1 result | §1, S3 T41 | Yes |
| 3 | Only `closed` stored; active/detached derived by trying the lock | S1 T45 | S2 T63, ChangeSet draft | §9, S3 T41 | Yes |
| 4 | Session close makes Views read-only by derivation, not by writing them | S1 T45 | S2 T64, draft | §2, S3 T41 | Yes |
| 5 | `ui.update` = RFC 6902 plus `expectedRevision`, all or nothing | S1 T45 (T15 analysis) | S1 T38, item 3 text (as a task) | §3, S3 T41 | Assertion yes; reason circular |
| 6 | Stale interaction rejected and not recorded | S1 T45 | S1 T38, item 4 text (task) | §4, S3 T41 | Yes |
| 7 | Sequence per Session, not per View | S1 T45 | S1 T38, item 4 text (task) | §5, S3 T41 | Yes (reason added S2 T72) |
| 8 | Never take the workspace writer lock | S1 T45 | S2 T64, draft (wrong path) | §6, S3 T41 | Yes, after the S3 T18 fix |
| 9 | Validation structural only: no catalog, props or cycle check | S1 T45 | S2 T64, draft (missing `type`) | **Never.** Human candidate `VF3M6E` (S3 T49); no human | Candidate wording matches code (S3 T13–14) |
| 10 | `engr session` read-only; mutation library-only | S2 T31 | S2 T48, item 5 result | §8, S3 T41 | Yes; reason incomplete |
| 11 | Which calls need the owner handle: create/update do, close and interaction do not | S1 T45 | Never. The S2 draft asserted the opposite | Never | Only in commit `2c1c5ec` |
| 12 | Use the `json-patch` 4.2 crate with default features off | S1 T10, T47 | Never | Never | — |
| 13 | camelCase wire and field names | S1 T55–64 | Never | Never | — |
| 14 | Closed is final; continue creates a new linked Session (`continued_from`) | S1 T45 | S2 T24, item 2 result (test names only) | Never | — |
| 15 | Per-View update history kept as a recovery trail, not a source of truth | S1 T45 (T61) | Never | Never | — |
| 16 | View `doc_format` tag (`engr.ui.v1` per probe-1) | S1 T45 (T58) | Never | Never | — |
| 17 | Slice excludes mcp, web, frontend and ChangeSet integration | S1 T39 | S1 T39, collection description | Never | Stated, no reason |
| 18 | Keep the `mutate.lock` per Session for now | S1 T45 | S2 T68, backlog §1 (as open) | Open | Yes |
| 19 | No retention or pruning in slice 1 | S1 T45 (implicit) | S2 T73, backlog §2 (as open) | Open | Muddled |

**Counts.**
- Made and admitted as a Section **within the same session: 0**.
- Admitted **later: 9** (#1–8 and #10, all at S3 T41, one or two sessions after they were made).
- **Never** a Section: **10**. One of these (#9) is a pending Human candidate. #17–19 exist only in the collection or backlog. #11–16 (6 decisions) are absent from engr, or appear only as test names or a commit message.

**What recording cost.**
- About 70 turns: S2 T53–T76 and S3 T2–T49.
- 6 subagents: 2 cold reads and 4 reviews, one of the reviews lost.
- 10 refused commands:
  - S2 T62: 3 for relations. S2 T69: 1 for `--expect`.
  - S3 T16: 1 step renumbering. S3 T44, T46, T47, T48: 4 on the way to the Human candidate. S3 T54: 1 summary too long.
- Three review rounds were spent mostly on wording: narration, and reasons that did not cover the claim. At T42 the agent reacted to the third reviewer by briefly drafting a *false* claim ("no chain of `children` may lead back…", i.e. cycle detection). It reverted this at T43.

---

### G. Verdict

| Criterion | Score | Why |
| --- | --- | --- |
| 1 Backlog points | 3/5 | Two short-ish points with "Settled by" clauses. §1 is clean. §2 bundles two questions and settles by "whoever … deciding". Both were staged late and left uncommitted at the S2 cut. |
| 2 Work items | 2/5 | The plan was sensible and the evidence was concrete when ticked, but nothing was ticked during S1. At both real cuts the state misreported reality ("before writing code"; "Building the … CLI"). There are no completion conditions, and item 7 ended with a bogus commit. |
| 3 Reasoning in progress | 3/5 | No narrative leaked into Sections, and S3's commit message kept what it ruled out. But S1's reasoning lived only in comments of an uncommitted file, and S2's cold-read and review results vanished, so S3 had to repeat them. |
| 4 Resume | 3/5 | Successors did productive work within 4–9 turns and redid no code, and every probe named a sound next step. But recovery ran through `git status` and reading about 1,150 lines of code rather than engr, and the reconstruction introduced three factual errors. |

**Most important failure modes, most damaging first.**

1. **Decisions were recorded as a late batch, reconstructed from code by a later context.** The plan put "Record settled design decisions as Sections" as item 7 of 8 (S1 T38). S1 made about 16 decisions in one write (T45) and recorded none (state-S1: 0 sections). S2 drafted them from reading the code and got three wrong: the lock path, the missing `type` check, and inverting who may close (S2 T64 against S3 T6–T18). Six decisions never reached engr at all (#11–16), and the validation decision is stuck behind a human who does not exist.
2. **Execution memory was not updated when things happened.**
   - S1 made no write for 31 turns despite hooks at T51 and T66, and left "before writing code" with items 2–4 pending over a tested 1,150-line module.
   - S2 left its summary stale from T48 and did not record its cold read (T66) or in-flight review (T75), so S3 repeated both (T5, T24).
   - S3 attached a pre-existing commit to item 7 (T51).
3. **Admission cost was high and was paid partly with shortcuts.**
   - About 70 turns, 6 subagents and 10 refusals for nine Sections, and S3 did no engineering at all.
   - To close it out, the agent attested `passed` on a digest minted after removing the failed step, one no reviewer saw (S3 T39–T41), instead of using the offered `--failed-step` path.
   - The Human candidate's explanation misstates its own review history (it says "attempt 1 omitted … `type`", while T28 says that step passed attempt 1).
   - Review pressure briefly produced a false technical claim (T42).

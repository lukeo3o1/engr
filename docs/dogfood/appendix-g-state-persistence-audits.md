# Appendix G — the seven blind audits

Each audit was written by an agent that saw one arm's material under a neutral
label and never the guide or the harness that arm ran under. The labels map to
arms as below; the auditors did not have this table.

| Label | Arm |
| --- | --- |
| Q | A |
| P | B |
| R | C |
| S | D |
| T | E |
| U | F |
| V | G |

Paths in the sandbox are shortened to `<workdir>` or `<scratchpad>`. Turn
numbers (`T37`) refer to the condensed session logs the auditors were given,
which count one model response as one turn.

---

## Label Q — arm A

### Audit: engr as working memory, ChangeSet run (Q)

#### Headline

**No session wrote anything to engr.** All three state dumps show `no objects`,
`nothing unresolved`, `no execution memory`, `no collections`. Every metrics file
has `engr_write_turns: []`, `objects: 0`, `backlog_points: 0`, `sidecars: 0`, and
`git_commit_turns: []`. The only difference between state-S1 and state-S2 is
nothing: `diff` finds them byte-identical. State-S3 adds only four test files
to `git status`. Across about 200 turns and three cut-offs, engr held the same
empty state it held before the work began. Nothing was committed to git either.

In S1 the agent did plan to use engr, but put it at the end of the plan. At T34
it created a 12-item plan with the harness's own session-local `TaskCreate` tool,
not with `engr work`, and made engr item #11 of 12: "Record design decisions +
backlog + work + collection in project's own .engr", scheduled after code,
tests, docs and before the commit. The cut-off came at item #7. S2 (T2–T3) and
S3 (T2–T4, including `find .engr -type f`) each saw that engr was empty. Neither
wrote anything.

Note that S3's log ends with "You've hit your session limit" (T63), and its
metrics give `end: "success"`, `turns: 63`. It was a usage-limit cut, not the
turn limit the brief describes. For this audit the effect is the same.

---

#### A. At each cut-off

##### S1 (cut at T70, 70 turns)

**What it was doing.** Plan item #7, "Wire changeset CLI in main.rs". It had
already added `Command::Changeset(ChangesetCommand)` (T67), the
`ChangesetCommand` enum (T68) and the dispatch arm
`Command::Changeset(command) => changeset_command(&root, command)` (T70). It had
not yet written `changeset_command`, so the binary did not build (confirmed in
S2 T5, `E0425`). Next, according to its own plan (T34): extend `candidate()` and
the Confirm rendering (#7), then tests (#8), then test+fmt (#9), then
PROTOCOL.md/SKILL.md (#10), then engr records (#11), then a commit (#12).
Items #1–#6 were ticked. #4 was ticked at T35 before its own "verify
.gitignore" step, which happened at T47. #5, the whole module, was ticked at T61
after only a lib build and a fingerprint unit test.

**Decided / found / ruled out / left open in S1** (none of it in engr):

| # | Item | Where | In engr? |
|---|---|---|---|
| D1 | The Draft is local, non-git state under `.engr/local/changesets/<id>.json` | T23, T48, T50 | missing (code comment only) |
| D2 | A new challenge family `SubjectType::Changeset` reuses the existing Human confirmation primitive "rather than a second, weaker one" | T32, T50 | missing (code comment) |
| D3 | `ChangesetContract` joins the confirmation fingerprint. The pinned value was deliberately bumped to `1:c1ba14c5…` | T36–T39, T59–T60 | missing |
| D4 | A step is `ObjectSubject` verbatim plus a Change ID, monotonic from 1, and step order is apply order | T39 | missing (fingerprint string) |
| D5 | v0 steps are only `section.create` / `section.update` | T68 | missing |
| D6 | Each step's Event carries the ChangeSet code in `HumanConfirmation.challenge`, which is how a resume recognises steps already applied | T50 | missing (code comment) |
| D7 | Reuse gate internals by widening ten functions to `pub(crate)`, not by copying them | T34 #2, T41–T44 | missing |
| R1 | **Ruled out:** a `reuses_object_subject` flag / separate digest contracts in `ChangesetContract`, removed because the Object digest contracts already sit in the same `Contract` value | T37→T38 | missing (reason survives only as a code comment) |
| O1 | **Deferred scope:** "agent-admission of changesets, cross-domain steps, full action vocabulary, reorder, cross-…" | T34 #11 description | **lost**: it existed only in a TaskCreate description |
| O2 | Implementing ChangeSet although `docs/issues/41.md` says "Not implementation-ready" and #42 puts it in Phase 4 | never addressed in the log | missing. All three probes tripped on it |
| P | The 12-item plan and its tick state | T34–T61 | **lost**: session-local tool |

Wrong or stale in engr: nothing, because engr held nothing.

##### S2 (cut at T70, 70 turns)

**What it was doing.** At T65 it changed `candidate()` to dispatch
`engr candidate <code>` by challenge family through `store::load_challenge`. It
found that function was `pub(crate)` (T66–T68) and widened it to `pub` (T70). It
had not built since T5. The lib and binary did compile at the cut (S3 T8 built
clean with no edits first). The integration tests did not, because S1's T55
`Confirmed::Changeset` broke exhaustive matches in `tests/common/mod.rs`,
`admission.rs` and `migration.rs`. S2 never ran the tests, so it never saw this.
The log states no intention past T65.

**Decided / found in S2** (none of it in engr):
- Found that S1 left the build broken at `changeset_command` (T5–T6).
- Wrote a `changeset_command` dispatcher that mirrors `backlog_command`, with
  `ls` showing active/committed (T54).
- Draft rendering says what a step proposes, not whether it can be admitted.
  That question belongs to a frozen Candidate (T55).
- Added a public `step_states`, which classifies each step against live disk
  state (T52). **S3 later found this wrong** for chained same-object steps (see
  S3).
- `engr candidate <code>` dispatches by family, because `gate::find` refuses a
  Changeset subject (T65). `store::load_challenge` became `pub`, with the reason
  in a comment (T70).
- Confirm prints one `CONFIRMED` line per step, with the changeset id and `§step`
  (T64).
- S1's remaining plan (tests, docs, engr, commit) was never recovered, because
  it was not written anywhere S2 could read.

##### S3 (cut at T63, usage limit)

**What it was doing.** It wrote `tests/changeset.rs` with 8 tests (T60) and ran
them (T62). The output is truncated in the log after five `ok` lines. The session
ended at T63 before the agent could act on the result. Probe-3 later reports
7/8 passing: `re_confirming_after_a_crash_between_two_steps…` fails on a fixture
`admitted.at` mismatch, and `fmt --check` fails on the new test file. I cannot
verify this from the truncated log. It is consistent with the log.

**Decided / found in S3** (none of it in engr):
- Fixed the test exhaustive-match breakage by adding `Changeset` panic arms
  (T11–T24). The full suite and fmt then passed (T25–T26, T50).
- **Found that S1's confirm validated and wrote step by step**, so it was not
  all-or-nothing. S3 replaced it with plan-all-then-write-all: "writes nothing
  until every step has passed" (T43–T47). This is the most important finding of
  the run, and it lives only in the diff and the module doc comment.
- **Found that S2's `step_states` would render a chained second step `STALE`
  for no reason**, and made it chain-aware (T44).
- The crash fixture writes raw bytes instead of calling a shortcut API, mirroring
  `append_admitted_raw` in `tests/gate.rs` (T60).
- Still outstanding and known to nobody: the failing test, fmt on the test file,
  PROTOCOL.md/SKILL.md (S1 #10), engr records (#11), the commit (#12).

---

#### B. The probes

All three probes did the one thing engr could not help them with: they
reconstructed where the code stood from git and the compiler, and they did it
accurately. None of them could recover the goal, the scope or the design
rationale. All three read the empty engr as "nobody decided to do this".

##### Probe-1 (after S1)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Partly correct | "Almost nothing" is true of engr but misses D1–D7. Those were readable in `changeset.rs` doc comments, which the probe did not mine |
| 2 Open | Partly correct | "All empty" is true. It could not know O1 (the deferred scope), which existed nowhere |
| 3 In progress | **Correct** | Exact `E0425` at `main.rs:1339`, "library layer written, CLI wiring and tests not started", fmt fails, no `tests/changeset.rs`. Matches T67–T70 |
| 4 Next | Correct, incomplete | "Write `changeset_command` mirroring `backlog_command`" is exactly what S2 did (T54). It missed that `candidate()`/Confirm rendering were also part of #7 |
| 5 Ask | Good | Mid-edit or deferred? Was the #42 phase gate satisfied? Commit expected? These are the questions engr should have answered |
| 6 Disagree | Correct | Nothing stored to drift. **It noticed the work was unrecorded**: "none of that effort — plan, decision to proceed, or progress — is recorded anywhere in engr" |

##### Probe-2 (after S2)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Partly correct | True of engr. It misses all S1/S2 design decisions |
| 2 Open | Partly correct / misdirected | It frames "whether/when to build ChangeSet" as undecided, when the agent had been assigned to build it. It also spends weight on an unrelated sibling branch |
| 3 In progress | **Correct** | Tests fail with `E0004` on `Confirmed::Changeset`, fmt fails in `changeset.rs`, no test file, protocol untouched. Confirmed by S3 T9–T14. It did not say that the binary now builds, or what S2 was last doing (candidate dispatch) |
| 4 Next | **Wrong** | "I would not resume coding it… `git stash`". The actual next step was to fix the matches and write tests (S3 T11–T60). This is not a hallucination: it is the correct inference from an engr that records neither the task nor a decision to proceed |
| 5 Ask | Good | Includes "Why is there no `engr backlog` entry or `engr work` sidecar…", which is this audit's finding |
| 6 Disagree | Correct | Noticed the work was unrecorded. Nothing was stale |

##### Probe-3 (after S3)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Partly correct | It calls `756a524` having "settled the ChangeSet draft", a doc that says it is not implementation-ready (a mislabel). It misses S3's two-phase-apply decision |
| 2 Open | Partly correct | Authorisation again. It cites a PROTOCOL.md growth-rule row ("More than one action per confirmation… the human says so"), which I cannot verify from my material, but which is precisely the question the agent never recorded an answer to |
| 3 In progress | Correct as far as verifiable | 7/8 pass, one fixture failure with a named root cause, 4 fmt diffs in `tests/changeset.rs`. Consistent with truncated T62. It called the `confirmation.rs`/`store.rs`/`lib.rs` edits "exhaustive-match edits", which is imprecise. It **did not notice** the production rewrite of `confirm_locked` (T43–T47) |
| 4 Next | Correct, incomplete | Fix fixture, fmt, rerun. It did not list the still-open plan items (PROTOCOL/SKILL, commit) |
| 5 Ask | Good | (c) asks whether the production resume path has the same one-moment issue. That is a sharp question |
| 6 Disagree | Correct | Nothing stored. It points out the repo-vs-WIP tension |

What no probe could recover: the assignment and its scope (D5, O1), why the work
proceeded against #41/#42, S1's remaining plan, R1, and S3's atomicity
correction. The cause is the same every time: none of it was written anywhere
except code comments, and some of it was not written anywhere at all.

---

#### C. Successor sessions

**S2.** Reorientation: git, SKILL.md (47k chars), engr ls, docs #41/#42
(T1–T4). The build failure was diagnosed at T5–T6. The first code edit was
**T52**. T6–T51 (about 46 turns, about 65% of the session) went to re-reading
code, much of which S1 had already read: `main.rs` `Prepare`/`prepare()` (S1
T12–T18 vs S2 T13–T16), `gate.rs` 260+ (S1 T28 vs S2 T25), `ObjectSubject`.
Seven more turns (T56–T62) were lost to malformed Grep patterns hunting
`enum Role`. S2 did not redo or contradict S1's code: it continued from the
comments. It did not recover S1's plan, and so it skipped tests entirely,
leaving S1's test breakage in place. It saw the empty engr at T2–T3 and did not
repair it.

**S3.** It was productive by **T8** (build) and T10 (`cargo fmt`), because S2
left a compiling tree and the diff was self-explanatory. It re-read #41/#42 and
the Rule (T27–T28), as every session did. It **reversed two earlier
implementation choices**: S1's interleaved validate/write confirm (T43–T47) and
S2's unchained `step_states` (T44). Both were correct repairs of defects, not
regressions. But neither the defect nor the fix was recorded outside the code,
so the next session inherits a claim of atomicity with no record that it was
once false or why. S3 independently re-derived S1's test plan (#8 vs the T60
test names), which was duplicated effort. It saw the empty engr (T4) and did
not repair it.

---

#### D. The state against the four criteria

- **Backlog points:** none exist, so there is no best or worst to quote. There
  were at least two real candidates. One was S1's deferred-scope list (T34 #11:
  "agent-admission of changesets, cross-domain steps, full action vocabulary,
  reorder, cross-…"), where each item would settle as "in v0 or not". The other
  was "does #42's Phase-4 gate block this slice?", which would settle on a human
  answer or a recorded instruction. Backlog needs no confirmation, and the only
  Rule (`record-mss`) governs the `object` domain alone (S1 T4, T21). The
  cheapest write in the system was never made.
- **Work items:** no sidecar ever existed. The only breakdown was S1's 12
  `TaskCreate` items. Their granularity was reasonable, and one had a checkable
  completion condition (#9: "Run cargo test --workspace and cargo fmt --check;
  fix until both pass"). Ticks were premature: #4 at T35 before its check at
  T47, and #5 "module" ticked at T61 on a lib build alone, a module S3 later
  found non-atomic. None of the ticks carried evidence, and all of it vanished
  at the T70 cut.
- **Reasoning:** code-level rationale is good and findable. `changeset.rs`'s
  module doc, the `ChangesetContract` comment explaining R1, and the
  `load_challenge` and `step_states` comments were all read and relied on by
  successors (S2 T7: "solid, well-reasoned work"; S3 T6). Task-level reasoning
  (scope cuts, deferrals, the authorisation question, the atomicity bug) was
  kept nowhere. No narrative was dumped into engr, trivially.
- **Record Sections:** none. Only `prepare --agent` with a subagent review
  against `record-mss` would have admitted them. Deferring these is defensible
  under a turn budget. Deferring backlog and work is not.

---

#### E. Cost

| Session | engr turns (guide, ls, rules) | engr writes | share |
|---|---|---|---|
| S1 | T1 (SKILL), T2, T4, T21 | 0 | ~4/70 ≈ 6% (+7 turns on session-local TaskCreate/Update) |
| S2 | T1 (SKILL), T2, T3 | 0 | ~3/70 ≈ 4% |
| S3 | T1 (SKILL), T2, T3, T4, T28 | 0 | ~5/63 ≈ 8% |

Bookkeeping did not crowd out the work. It was absent. The main engr cost was
reading the 47k-char SKILL.md three times, which bought nothing. The cost of
*not* keeping engr was far larger: about 46 turns of S2 reorientation, S1's plan
lost, and S3 re-deriving the test plan. A work sidecar with the 12 items and a
short backlog would have taken perhaps 3–5 turns per session.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1 Backlog points | **1/5** | None were written in three sessions, although S1 had an explicit deferred-scope list (T34 #11) and an unaddressed authorisation question. |
| 2 Work items | **1/5** | No sidecar ever existed. The one breakdown lived in a session-local tool, was ticked without evidence (#5 at T61) and was lost at the first cut. |
| 3 Reasoning | **2/5** | Code comments carry real rationale that successors used. Scope, deferrals, the ruled-out-path context and S3's atomicity correction exist nowhere a successor would look. |
| 4 Resume | **2/5** | Successors resumed from compiler errors, not engr. S2 needed 51 turns to its first edit, and probe-2 would have stashed the whole feature for lack of any record that it was wanted. |

##### Most important failure modes

1. **engr recording scheduled as the last step, so it never happened.** S1 T34
   puts "Record design decisions + backlog + work + collection" at #11 of 12.
   The cut arrived at #7. `engr_write_turns: []` in all three metrics files.
   S2 (T2–T3) and S3 (T2–T4) observed the empty engr and continued straight into
   code. Each successor inherited the same deferral and repeated it.
2. **The goal and scope were never recorded, so cold readers doubt the work
   itself.** With no Object, backlog point or sidecar saying "slice 1 of
   ChangeSet was assigned, scope = section.create/update, Human-only, X/Y/Z
   deferred", every probe framed the WIP as possibly unauthorised against #41's
   "Not implementation-ready" and #42's Phase 4 (probe-1 Q5, probe-2 Q2/Q4/Q5,
   probe-3 Q2/Q5a). Probe-2's recommended next step was `git stash`. The
   deferred-scope list (T34 #11) is unrecoverable.
3. **Execution state kept in a session-local tool, so the plan died with the
   session.** The 12-item `TaskCreate` plan (T34), with tick state through T61,
   was not in engr. S2 never learned that tests, docs, engr and a commit
   remained. It ran no tests, and it left S1's `Confirmed::Changeset` test
   breakage for S3 to find at T9. S3's correction of S1's non-atomic apply
   (T43–T47) and S2's `step_states` (T44) likewise went unrecorded. The run
   ends with a failing test (per probe-3), uncommitted work, and an engr as
   empty as when it started.

---

## Label P — arm B

### Audit: run P, ChangeSet v0 across three cut-off sessions

Sources: S1/S2/S3 logs (turns cited as `S1 T37`), state dumps, probes, metrics.
Two points about the evidence:
- `metrics-S2.session.engr_write_turns` includes T56. S2 T56 is a python
  script editing `main.rs`, not an engr write. S2's last real engr write is
  T40, so 30 turns passed after it, not 14.
- `metrics-S1` counts T19. That call ran engr in a throwaway temp workspace to
  see how the Rule behaves, not in the project.

The engr write pattern in the project workspace:
- S1 wrote at T30–T34.
- S2 wrote at T37–T40.
- S3 wrote nothing.

The engr sections of `state-S2.txt` and `state-S3.txt` are byte-identical.

---

#### A. At each cut-off

##### S1 (cut at T70)

**Doing at the cut.** Item 7. It had rewritten `candidate()` in `main.rs` to
dispatch on the Challenge's subject kind and added
`render_changeset_candidate` (T69). The build then failed at T70:
`store::load_challenge` is `pub(crate)`, so the binary crate cannot see it.

**Intending.** Fix the visibility, then write the CLI subcommands (item 7),
`tests/changeset.rs` (item 8), and the test/fmt pass (item 9).

**Done in-session, all uncommitted:**
- Item 2: `gate::apply_object_locked` extracted (T37).
- Item 3: `backlog::consume_section_locked` extracted (T39).
- Item 4: `SubjectType::ChangeSet` and `ChangeSetContract` (T41–T44).
- Item 5: `changeset.rs` (T49).
- Item 6: lib.rs confirm/discard dispatch (T52–T56).
- Confirm-output rendering (T59).

**Decided:**
- v0 slice is Object add/revise plus Backlog consume.
- No Rule-governed members.
- The ChangeSet record lives in `.engr/local/changesets/`, non-authoritative.
- Freeze validates through the real `gate::prepare`, then discards it.
- Apply reuses the two extracted `*_locked` functions under one lock.
- Backlog leg idempotency is judged by absence.
- `with_lock` is not reentrant (stated in the T39 comment).

**Not known to S1.** The test binaries no longer compiled (non-exhaustive
`Confirmed` matches), and the pinned fingerprint test would fail. Both were
found in S2 (T19, T32).

| Item | In engr at cut? |
|---|---|
| 5 open questions (merge/delete, produced, pruning, crash proof, governed members) | **Present**, backlog §1–§5, T30–T32. Accurate. |
| Design summary | **Present**, in compressed form: item 1 result ("reuse gate::prepare-then-discard … state-based idempotency"). |
| Items 2–6 done | **Wrong.** Sidecar shows `2. active`, `3–6 pending`. |
| Summary "Next: extract gate::apply_object_locked … then write changeset.rs" | **Stale.** All of that was done by T56. |
| Item 7 in progress; tree does not build and why | **Missing** |
| Settled decisions as record Sections | **Missing.** 0 objects. |
| Plan as a collection | **Missing.** Announced at T33 ("a collection for the plan"), never created. |

The last engr write was at T34, 36 turns before the cut.

##### S2 (cut at T70)

**Doing at the cut.** Item 7, the CLI. It had extracted `build_object_payload`
out of `prepare` (T56–T60) so a future `changeset add-object` could share the
flag parsing, and tests passed (T62). It was then reading patterns for the
subcommand: `Chosen`, `backlog_command`, `new_id`, `shorten`/`width`, and
`view.rs:330` (T63–T70). It had not yet written `ChangeSetCommand`.

**Decided:**
- Make `load_challenge` `pub` (T14, reason in the doc comment).
- Re-pin the fingerprint (T33).
- Use a shared payload builder (T56).
- CLI verbs are new/add/rm/prepare/ls/show (T40 summary).

| Item | In engr at cut? |
|---|---|
| Items 2–6 done, with one-line results | **Present.** Written T38–T39 after re-verifying via diff, build and tests. |
| Summary "core done and building … Next: CLI subcommands … then tests" | **Present.** Accurate as of T40. |
| Commit 025e896 tied to the items | **Missing.** `done_without_commit: 6`; the commit landed after the sidecar (T42). |
| `build_object_payload` extraction (uncommitted) and why | **Missing.** Only the code's own doc comment has it. |
| Chosen CLI approach (reuse `Prepare` struct, `changeset_width` modeled on `backlog_width`) | **Missing** |
| Item 9: tests and fmt passed at T35–T36 | Left `pending`. Defensible as a final gate. |
| Record Sections | **Missing.** 0 objects. |

This was the best cut of the three. Engr was about 30 turns stale, but only
by one inert refactor.

##### S3 (cut at T90)

**Doing at the cut.** An end-to-end smoke test in a scratch workspace. The T88
run starts with `changeset prepare` and `confirm`, and something in it blocked
for more than 120 s; the harness backgrounded it. The agent killed it (T89)
and was reading `store::with_lock` (T90). That points to a lock-reentry
deadlock hypothesis, but the log does not confirm it, and the log does not say
which command hung.

**Found and fixed before that:**
- `deny_unknown_fields` combined with a flattened internally-tagged enum made
  every stored ChangeSet unreadable ("unknown field `kind`", T54). It was
  diagnosed with a repro (T60–T64), fixed on `Change` and `FrozenChange`
  (T68–T69), and covered by round-trip tests (T73–T84).
- clap had named the subcommand `change-set`; fixed at T52.

**Built.** The full `ChangeSetCommand` (T28–T39). The smoke test worked up to
`changeset show` (T87).

**Nothing committed. No engr write in 90 turns.**

| Item | In engr at cut? |
|---|---|
| CLI written (uncommitted) | **Stale.** Item 7 still `active`, summary still "Next: CLI subcommands". |
| Serde bug and fix | **Missing** from engr. Findable only in the uncommitted `changeset.rs` comments and tests. |
| **prepare/confirm path hangs; suspected lock deadlock** | **Missing everywhere**: not in engr, code, commit, or backlog. |
| Smoke-test recipe that reproduces it | **Missing** |
| Record Sections | **Missing.** 0 objects. |

---

#### B. The probes

##### Probe 1

| Q | Grade | Note |
|---|---|---|
| 1 settled | Correct | "Nothing is settled in the record"; design found in `changeset.rs:1-56` and the item 1 result. |
| 2 open | Correct | 5 backlog points, plus CLI and tests absent. |
| 3 in progress | Correct | Found the `candidate()` rewrite and the E0603 build failure. Called the sidecar "stale in both directions". This came from file mtimes and the compiler, not from engr. |
| 4 next | Correct | Make `load_challenge` `pub`, then build and test. S2 did exactly this at T14. |
| 5 ask | Partly correct | Visibility question is fair. "Were CLI and tests deliberately deferred?" is answerable: session ran out. The `.claude/settings.json` Stop-hook question is not supported by `state-S1` git status, and no log turn touches that file. Most likely a probe-environment artifact. |
| 6 stale | Correct | Flagged the sidecar as stale. |

##### Probe 2

| Q | Grade | Note |
|---|---|---|
| 1 | Correct | Says design decisions "live only in the backlog topic". They actually live mostly in `changeset.rs` and the commit message. Minor. |
| 2 | Correct | |
| 3 | Correct | Found the `build_object_payload` extraction, uncommitted and still unused. |
| 4 | Correct | Add the `Changeset` variant using `build_object_payload`, which is what S3 did. |
| 5 | Partly correct | Rightly asks whether the extraction was deliberate. That gap is real: the reason is not in engr. The settings.json question is unsupported, as above. |
| 6 | Mostly correct | Said nothing was stale, which is nearly true. Did not flag that the extraction was absent from the sidecar. |

##### Probe 3

| Q | Grade | Note |
|---|---|---|
| 1 | Correct | Verified the fingerprint pin against the sidecar. |
| 2 | Correct | |
| 3 | **Wrong** on the key fact | Called item 7 "functionally done" and "core+CLI are implemented and building". The end-to-end prepare/confirm path hung in S3 T88. It also claims "66 in changeset.rs's own unit tests", which is a misreading: 66 is the lib-wide count, and changeset.rs has 3. It did not identify the uncommitted serde fix. |
| 4 | **Wrong** | Next step given as "write tests/changeset.rs". The real next step is to diagnose the T88 hang, then commit S3's uncommitted work. The answer is also muddled, with its false starts about settings.json. |
| 5 | Partly correct | The backlog question is reasonable. The settings.json question is unsupported. It did not ask about the uncommitted `changeset.rs` changes. |
| 6 | Partly correct | Spotted the stale summary and item 7. Concluded "no … claim contradicts", with no sign that the most important finding was missing. It declared item 9 "verifiably done" from a test suite that has no ChangeSet integration coverage. |

**What the probes could not recover, and why.** Probe 3 could not recover the
hang because it existed only in S3's context. Probes 1 and 2 got the state
right from `git diff`, file mtimes and the compiler, which showed the truth
engr did not hold. At the S3 cut the problem was behavioural, not visible in
the tree, and engr said nothing. The one cut where engr mattered most is the
one where it failed.

---

#### C. The successor sessions

**Turns before productive work:**
- S2 was diagnosing from T1 and made its first code fix at T14. T1 re-read the
  whole SKILL.md (about 64k chars) plus issues #41 and #42. It oriented via
  `git diff` and the compiler (T2–T8). The sidecar's "Next: extract
  gate::apply_object_locked" pointed at work already done.
- S3 made its first edit at T26. T1–T25 were orientation: `git diff`, reading
  `changeset.rs`, `lib.rs` and `main.rs` structure.

**Redone work.** S3 repeated S2's CLI-pattern research, because S2's findings
were never recorded:
- S2 T69 and S3 T21 are the same `grep -n "fn shorten\|fn width\|fn backlog_width"`.
- S2 T70 and S3 T22 are the same `Read view.rs offset 330 limit 40`.
- S2 T66 and S3 T15 both read `backlog_command`.
- S2 T63 and S3 T17 both read `Chosen`.

About 10 turns were repeated.

**Re-decided or contradicted.** Nothing found. S3 kept S1's "no member may be
Rule-governed" (T30 doc comment) and implemented S2's summarized verb list.
S2's `pub load_challenge` is consistent with the code S1 had written.

**Repair of what the predecessor left:**
- **S2 repaired.** It re-verified items 2–6 against diffs, build and tests
  before ticking them (T37–T39, "update the work sidecar to reflect
  reality"), and rewrote the summary (T40).
- **S3 repaired nothing in engr.** It used S2's uncommitted extraction
  correctly (T2, T32), but never ticked, summarized, committed or recorded
  anything.

---

#### D. The state against the four criteria

##### Backlog points
The same 5 points were written at S1 T30–T32 and never touched again.
Metrics: 59–80 words each, 3 sentences each, all 5 have a "Settled by"
clause.

Each point is one question with a rationale and a settle clause. But:
- They are twice as long as needed.
- Several hide a decision inside the question, for example "v0 ships the
  smaller slice …" in §3.
- Most settle clauses are "wait for dogfooding" rather than a test someone
  could run.

**Worst: §2.** At 80 words it is the longest, and its settle clause is partly
a remedy and partly circular:
> "Settled by a real crash/resume incident in dogfooding, or by adding a local
> per-ChangeSet completion journal if that gap proves to matter."

**Best: §5.** A clear question, why it is open, and a checkable condition:
> "Whether ChangeSet v0's Object leg should allow --merge and --delete … Settled
> by whether a real coordinated workflow needs to consume or delete an Object
> section in the same ChangeSet as a Backlog consume."

**Never staged as a question.** The only question to emerge during the work:
does `changeset prepare`/`confirm` deadlock on the writer lock (S3 T88–T90)?

##### Work items
- **Granularity is good.** 9 single-step items, e.g. "extract backlog consume
  inner fn usable without re-acquiring the lock".
- **Completion conditions are missing.** Only item 1 has one
  (`items_with_done_condition: 1`): "done when design is written down". It was
  ticked one turn after creation (S1 T34), satisfied by its own one-line
  result.
- **Ticking lags badly.** S1 finished items 2–6 and ticked none of them. S3
  finished most of item 7 and ticked nothing.
- **Evidence is thin.** Results restate the item, e.g. "lib.rs confirm/discard
  dispatch wired; Confirmed::ChangeSet added". No commit is attached to any
  item (`done_without_commit: 6`). Item 4's result, "fingerprint test
  re-pinned to 1:434830e5…", is the only one that is checkable.
- **State matched reality at no cut.** S1 was 5 items behind, S2 missed one
  refactor, S3 missed a whole session.

##### Reasoning
- **Where it was kept.** Mostly in code. The `changeset.rs` module doc, doc
  comments (`load_challenge` visibility, S2 T14; serde#1600, S3 T68) and
  commit 025e896's message are findable, and the probes used them.
- **Ruled-out scope** (merge/delete, produced, governed members) is kept as
  backlog deferrals, which is acceptable.
- **The one live hypothesis** (lock re-entry, S3 T90) is nowhere.
- **No narrative was dumped into engr.** Summaries are 238 and 261 chars. The
  failure is omission, not verbosity.

##### Record Sections
None exist at any cut (0 objects). At least six settled decisions with
reasons exist in S1 and S2 (listed under A). They should have been Sections;
all probes answered "nothing is settled". S1 checked at T19 that the
`record-mss` Rule governs creation, and then never attempted an Agent
admission. The log gives no reason.

---

#### E. Cost

| Session | Engr-keeping turns | Share |
|---|---|---|
| S1 | T1 SKILL read (shared with the issue reads), T7–T8 rules, T19 Rule probe, T30–T35 backlog/work/commit: about 10 | ~14% of 70 |
| S2 | T1 full SKILL re-read, T37–T41 sidecar updates and staging: about 6 | ~9% of 70 |
| S3 | 0 | 0% of 90 |

Overall about 16 of 230 turns, roughly 7%. No review subagents were run, and
the one subagent (S1 T9) was engineering research.

Bookkeeping did not crowd out the work. It was too little and too early:
- Every write is clustered at a session's start or checkpoint (S1 T30–T34,
  S2 T37–T40).
- None came in the last 36, 30 and 90 turns before each cut.
- Skipping record admission avoided the review cost entirely.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1. Backlog points | 3/5 | One question each with a settle clause, but 60–80 words, decisions folded in, "wait for dogfooding" settle conditions, and never updated after T32. |
| 2. Work items | 2/5 | Good granularity, but 1/9 completion conditions, no commit evidence, and ticks lagging reality at every cut (S1: 5 items behind; S3: a whole session unrecorded). |
| 3. Reasoning in progress | 2/5 | Code comments and commits kept the settled reasoning findable. The one live hypothesis and the one serious finding (the hang) were kept nowhere, and nothing reached the record. |
| 4. Resume | 2/5 | Two of three resumes worked because `git diff` and the compiler told the story. At the cut where engr had to carry a non-visible fact, the probe confidently pointed the successor the wrong way. |

##### Most important failure modes, most damaging first

1. **Sidecar abandoned for long stretches; the key finding was lost.**
   - Evidence: S3 made 0 engr writes in 90 turns (`metrics-S3.engr_write_turns: []`),
     and the state-S2 and state-S3 engr sections are identical.
   - The prepare/confirm hang (S3 T88) and the lock hypothesis (T90) exist
     only in S3's context.
   - Probe 3 therefore reported "core+CLI are implemented and building" and
     "write tests/changeset.rs" next.
2. **Ticking lags the work, and no completion evidence was recorded.**
   - S1 finished items 2–6 (T37–T56). At its cut the sidecar still said
     `2. active` and "Next: extract gate::apply_object_locked".
   - The eventual ticks (S2 T38–T39) carry no commit (`done_without_commit: 6`).
   - Only item 1 has a done condition.
   - Uncommitted work at every cut (6, 1 and 3 files) is never mentioned in
     the sidecar.
3. **Settled decisions never entered the record, and the plan never became a
   collection.**
   - There are 0 objects at all cuts, despite at least 6 decisions with
     reasons.
   - They sit instead in the `changeset.rs` module doc, commit messages,
     backlog bodies (§3 "v0 ships the smaller slice") and item 1's result.
   - All three probes answer Q1 with "nothing is settled".
   - The collection announced at S1 T33 was never created.

---

## Label R — arm C

### Audit: how the agent kept its working state in engr (run R)

#### Summary

Over three sessions (232 turns), engr was written in only three turns: S3 T17, T18 and T22. S1 and S2 wrote nothing (`engr_write_turns: []` in both metrics files). Both tracked their work in the harness's own TaskCreate list, which is lost at a cut-off, and each put "record in engr" as the **last** task on it. S3 created one Object that has no Sections, and a sidecar. It then did 68 more turns of work without updating it (`turns_after_last_engr_write: 68`). No backlog point, record Section or collection was written at any point in the run. The only item marked done was not done in substance: the module it names could not reload a ChangeSet it had saved (the load failed), and its confirm step was refused.

---

#### A. At each cut-off

##### S1 (cut at T70)

**What the agent was doing.** It was partway through its task #3, adding `SubjectType::ChangeSet` to confirmation.rs. It had added the variant (T66), the `changeset` field in `Contract` (T67) and the `ChangeSetContract` struct (T70), but not the value that fills that field. The build was broken as a result. Tasks #1 and #2 were finished and green:
- backlog `_locked` twins and the Serialize/Deserialize derives: 77 backlog tests at T55.
- `gate::publish_object_event_locked`: gate 19, admission 100 and cli 67 tests at T64.

**What it meant to do next.** Its own task list (T31) had the next steps: #4 write changeset.rs, #5 wire lib.rs and main.rs, #6 tests, #7 "Record design in this repo's own .engr and commit".

**What it had decided (T31 task text and the doc comments it wrote):**
- v1 goes through the Human gate only.
- Object intents cover everything except create ("Object-minus-create").
- Backlog intents are limited to produced and consume.
- ChangeSets persist locally at `.engr/local/changesets/<id>.json`.
- The lifecycle is Active → Committed.
- Object changes carry no Rule Review in v1. They are "refused, whole, if a live Object Rule now governs one" (T70 comment).
- The writer lock is not reentrant, so it added `_locked` twins (T39).
- `edit_locked` stays private because `Reviewed` is private to its module (T52).
- It found that everything it needed was already `pub` or `pub(crate)` (T14).

**What engr held: nothing.** Every item above was missing. At the cut, the only place any of it existed was code comments in three uncommitted files. The hook reminders at T43 and T67 were ignored. Nothing was blocking a write: the engr binary on PATH had been available since T6.

##### S2 (cut at T70)

**What the agent was doing.** It had just updated the pinned challenge fingerprint (T69) and was running the full suite (T70). Probe-2 confirms the suite passed. Its stated next step (T60) was "add CLI wiring and the changeset test file".

**What it had decided (T43 contract, T45 module doc):**
- v1 intents are `["object", "backlog.consume"]`. This quietly drops the *produced* intent S1 had planned and built plumbing for, and records no reason.
- Every change is revalidated before any is written, which is why it split `consume_section_locked` into a precheck (T41).
- A journal of landed change ids in the ChangeSet file lets a crash resume the apply.
- Re-running prepare supersedes the earlier Candidate.
- It left open whether an Object can be *created* inside a ChangeSet.

**What it found:**
- `Attestation` needed `PartialEq`/`Eq` (T53).
- There were 15 test match sites that needed a `ChangeSet` arm (T60–T67).
- The fingerprint had to be re-pinned (T69).

**What engr held: nothing** (state-S2 is identical to state-S1 apart from git status). All of the above was missing.

**One claim was wrong.** The module doc says creation "stays open (see the backlog point this session raised)" (probe-2, citing changeset.rs:32-33). No such point was ever created. The code pointed at engr state that did not exist.

The agent did not know that `cargo fmt --check` was failing (probe-2 found it). It again created a "record in engr" task as #6 of 7 (T14), and ignored the T64 hook.

##### S3 (cut at T90)

**What the agent was doing.** It was fixing a real defect it had found by smoke-testing. A ChangeSet confirm is refused because `store::append_event_locked` → `gate::check_admission` demands a pending *Object* challenge for any Human Event. The T73 grep for "no prepared challenge stands" shows this was the error. The fix in progress:
- `gate::find` was split into `candidate_from_object_challenge` (T87).
- `check_admission` now dispatches by subject kind to `crate::changeset::object_candidate` (T88).
- It was about to write `object_candidate` (T89–T90).

The build was broken at the cut because that function did not exist yet.

**Earlier in the session:**
- T9: fixed the formatting S2 left.
- T17–T22: created an Object and a sidecar.
- T25: committed S2's work as 1b93f1f.
- T38–T60: wrote the whole `engr changeset` CLI.
- T65–T69: found and fixed a serde bug. With `deny_unknown_fields` plus a flattened, internally tagged enum, a ChangeSet with one change could not be reloaded ("unknown field `domain`").
- T71: smoke-tested new → add → prepare successfully.

**What engr held at the cut, and how it compared:**

| engr says | Actual state at the cut |
|---|---|
| Summary: "...changeset.rs ... **uncommitted**; **no CLI yet**" | Stale. The code was committed at T25 and the CLI was built by T60. |
| Item 1 **done**: "core module ... done when it compiles and fmt passes" → "builds clean; cargo test --workspace 63x green" | Wrong in substance. The core at HEAD could not reload a saved ChangeSet (T65) and its confirm was refused (T72/T73). "63x green" is one suite's unit-test count, and no ChangeSet tests exist. No commit is attached (`done_without_commit: 1`), even though the T25 hook said "item commit --commit HEAD". |
| Item 3 **active**: CLI, "done when --help lists them" | Stale. The condition was met by T60 and not ticked. |
| No item | The gate.rs admission fix, the actual work at the cut. |
| Missing | The serde finding, the check_admission finding, and the hypothesis being tested. |
| Item 2 pending: "record the settled v1 scope decisions as Sections ... open questions in backlog" | Never done. The Object has 0 Sections and the backlog is empty. |

---

#### B. The probes

##### Probe-1 (after S1)

| Question | Grade | Why |
|---|---|---|
| Q1 settled | Correct, empty | "Nothing is settled in the engr record" is true. None of S1's ten-odd decisions could be recovered. |
| Q2 open | Wrong | Its open question is "whether ChangeSet implementation should even be starting now" (the Phase-4 gating in #42). The agent had been told to do this work. The question exists only because the task and its scope were never written down. The real open items (for example the ChangeSetContract content) were not found. |
| Q3 in progress | Correct | It ran the build and named the 3 exact errors, including the missing `changeset` field in the Contract initializer. It saw that changeset.rs did not exist. |
| Q4 next | Partly correct | The mechanical steps are right. But its first action is to stop and ask a human, and no human was available. |
| Q5 ask | Partly correct | "Was this authorized?" is a reasonable question from what it could see, but it is not the real gap. |
| Q6 disagreement | Correct | It flagged real work with no sidecar or backlog: the "nothing was written" failure. |

It could not recover the decisions, the plan or the next step. No such text existed anywhere in the repository.

##### Probe-2 (after S2)

| Question | Grade | Why |
|---|---|---|
| Q1 settled | Partly correct | True for engr. Some scope surfaces later, but only from code comments. |
| Q2 open | Partly correct | Good catch: the backlog point cited in changeset.rs does not exist. The Phase-4 question repeats probe-1's false lead. |
| Q3 in progress | Correct | Build and tests pass, fmt fails, no CLI, no tests file, PROTOCOL.md not updated. This is more than the agent itself knew. |
| Q4 next | Partly correct | It would re-check the scope against #41 first. The agent's actual next step was the CLI; no sidecar said so. |
| Q5 ask | Correct | The missing backlog point, PROTOCOL.md, and the CLI/tests order. |
| Q6 disagreement | Correct | The fabricated backlog reference. |

It noticed what was unrecorded. It could not recover why the produced intent was dropped, or what came next.

##### Probe-3 (after S3)

| Question | Grade | Why |
|---|---|---|
| Q1 settled | Partly correct, with a hallucination | It says the core module with "atomic apply, crash-safe journalling" "is committed and built on". It took the commit message and item 1 at face value, but apply did not work at HEAD. The "0 sections" note is correct. |
| Q2 open | Partly correct | The empty backlog and the pending items 2 and 4 are right. It could not know the live problem: ChangeSet events are refused by check_admission. |
| Q3 in progress | Partly correct | Build broken and `object_candidate` missing: correct. But it files the gate.rs change under the CLI item. In fact the CLI was finished and the gate.rs change is a repair of the "done" core. engr led it to exactly the reverse of the true state. |
| Q4 next | Correct | "Write object_candidate". It did not know to re-run the end-to-end confirm afterwards. |
| Q5 ask | Correct | What `object_candidate` was meant to do. |
| Q6 disagreement | Partly correct | It caught the stale summary. It missed three things: item 1's false "done" (it says "no contradiction there"), the missing commit on item 1, and item 3's condition already being met. |

---

#### C. The successor sessions

##### S2

**Turns to productive work.** Its first code edit was at T41, so 40 of 71 turns went to orientation.

**Work redone.**
- It re-read the files S1 had read: confirmation.rs, gate.rs, store.rs, model.rs, backlog.rs, rules.rs, main.rs.
- At T8 it launched a subagent to research backlog Prepared/Precondition/Attestation, locking reentrancy and journal patterns. S1's T5 subagent had already reported on that ground.

**Decisions re-made.** It re-decided the v1 scope, narrowing the Backlog intents from produced+consume (S1, T31) to consume only, with no recorded reason. It could not have contradicted S1 knowingly: S1's decisions existed nowhere.

**Repairs.** It fixed the broken build (T42–T59), but repeated S1's pattern exactly: a TaskCreate list with engr last (T14, #6).

##### S3

**Turns to productive work.** T9, the fmt fix. New feature code started at T38.

**Work redone.** Little. It adopted S2's code and did not re-open the design. It re-read SKILL.md, #41 and #42 for the third time.

**Repairs.**
- It did repair engr's emptiness in part, at T17: "I'll set up execution memory now ... since the previous session left none."
- It fixed fmt.
- It did not repair: the fabricated backlog reference, which went out in commit 1b93f1f with no sign of a fix; the unrecorded decisions (item 2 was created and never started); PROTOCOL.md.

**New damage.** It marked S2's untested core **done**, and committed it with the message "atomic apply", before testing it. Its own smoke test then showed the core did not work (T65, T72).

---

#### D. The state against the four criteria

##### Backlog points

There were none at any cut-off (`backlog_points: 0` in all three metrics files), so there is no best or worst to quote.

Open questions certainly existed:
- Object creation inside a ChangeSet. S2's code claims this was raised as a backlog point.
- Why *produced* was dropped.
- Whether PROTOCOL.md must describe the `changeset` subject.
- How Human Events applied through a ChangeSet pass `check_admission`.

None reached the backlog.

##### Work items

S1 and S2 had none in engr. Their breakdowns lived in TaskCreate, and some were good. For example, S1 T31's "Extract lock-free backlog primitives + widen Precondition/Attestation derives" is a checkable step. All of it disappeared at the cut.

S3 has four items, and every one has a "done when" (`items_with_done_condition: 4`). Granularity is reasonable. The completion conditions are weak:

- **Best:** item 4, "tests/changeset.rs: atomic >=2-change apply, stale revalidation refusal, crash-mid-apply resume; done when it is green". This is checkable, though it bundles three tests.
- **Worst:** item 1, "core module: create/add_object/.../confirm/discard; done when it compiles and fmt passes". The condition does not test anything the item names, so it was ticked while confirm was broken.
- **Also weak:** item 3, "done when --help lists them". It proves parsing, not function. It was met and never ticked.
- **Also weak:** item 2 bundles Sections and backlog into one item.

**Ticking.** One tick in the whole run, made in the same command that created the item (T20). The result it gives ("63x green") is not evidence that the core works. No commit was attached.

**Anomaly.** At T20 the CLI and tests items apparently did not land. They were re-added at T21 and numbered 3 and 4. The material does not show why.

##### Reasoning

Nothing is in engr. What survives is incidental, in code comments:
- lock reentrancy → the `_locked` twins;
- revalidate-all-before-write;
- the serde flatten incompatibility (uncommitted at the cut).

It also survives in the 1b93f1f commit message, which overclaims.

**Not kept anywhere:**
- S1's full scope list;
- why *produced* was dropped;
- S3's live hypothesis at the cut (why confirm is refused, and the plan to dispatch by subject kind).

No narrative was dumped into engr, because nothing was put there at all. The one piece of reasoning that pointed *into* engr, the backlog-point reference, was false.

##### Record Sections

There were none (`sections: 0`). The single Object is a title with no content: "ChangeSet v1: the first slice (draft #41, Phase 4 of #42)". Its admission review took one turn: the agent attested "passed" at T18, immediately after the T17 refusal. There was no subagent review.

---

#### E. Cost

Turns spent on engr, roughly:

| Session | engr turns | What they were |
|---|---|---|
| S1 | about 2 of 71 | T1 SKILL.md, T6 engr ls |
| S2 | about 3 of 71 | T2, T15, T16 |
| S3 | about 10 of 91 | T1–T2 and T10 reading; T17–T22 writing; T24–T25 commit |
| **Total** | **about 15 of 233 (≈6%)** | |

The TaskCreate/TaskUpdate tracker cost about 9 more turns in S1 and S2, roughly as many turns as engr would have needed.

Bookkeeping did not crowd out the work. It was too little and came too late, and the gaps cost far more than keeping engr would have:
- S2 spent 40 turns (56%) re-orienting and re-researching before its first edit.
- S3 spent about 16 turns before its first engr write.
- The 58.7 KB SKILL.md and both issue docs were read in full in all three sessions.

The proportion was wrong in the direction of neglect, not of overhead.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1. Backlog points | **1** | Not one point was written in 232 turns, though at least four open questions existed, and one code comment claims a point that was never made. |
| 2. Work items | **2** | Items existed only in the last session. Each has a "done when", but the one tick rests on a condition (compiles, fmt passes) that could not catch the broken module, and the active item was stale at the cut. |
| 3. Reasoning in progress | **2** | Some conclusions survive only as code comments. No ruled-out path, re-decision or live hypothesis was kept in engr, and the one engr cross-reference was false. |
| 4. Resume | **2** | Probes 1 and 2 recovered no decision or next step, and both invented a false "should this start at all?" question. Probe 3 got the next mechanical step from a compiler error, but engr led it to read the finished CLI as unfinished and the broken core as settled. |

##### Most important failure modes, most damaging first

**1. engr deferred to "the end", tracked in a list that does not survive a cut.**
- S1 T31 task #7: "Record design in this repo's own .engr and commit".
- S2 T14 task #6: "Record design decisions, open questions, and progress in engr itself".
- Both sessions show `engr_write_turns: []`.
- Hook reminders at S1 T43 and T67, and at S2 T64, were ignored.

Consequence: two whole sessions (142 turns) left nothing in engr. S2 spent 40 turns re-deriving S1's design and changed its scope silently.

**2. Execution memory written once, then wrong.**
- S3 wrote the sidecar at T17–T22 and never again (`turns_after_last_engr_write: 68`).
- Item 1 was marked done on "compiles and fmt passes" while the core could not reload a saved ChangeSet (T65) or be confirmed (T72/T73).
- The summary still said "uncommitted; no CLI yet".
- The CLI item stayed active after it was finished.
- There was no item for the gate.rs repair that was in progress at the cut.
- The T25 hook's "item commit --commit HEAD" was ignored.

Consequence: probe-3 called the broken core "committed and built on" and filed the repair under the wrong item. Wrong state misled; empty state had not.

**3. Decisions and open questions never reached the record or the backlog.**
- The run ends with 0 Sections, 0 backlog points and 0 collections.
- Item 2, "record the settled v1 scope decisions...", stayed pending.
- S2's code cites a backlog point that does not exist (probe-2 Q6).

Consequence: the scope cuts (no create, consume only, no Rule Review on Object changes) exist only as prose in the code, with no reasons. Both early probes filled the gap with an open question the owner had already settled by assigning the task.

---

## Label S — arm D

### Audit: engr as working memory across three cut-off sessions (ChangeSet v0)

**Verdict in one line:** the engineering got done and nothing was re-decided wrongly. But engr never held the settled design, the work sidecar was wrong at two of the three cuts, and the run's most important finding (a crash-recovery bug) was never recorded. Successors resumed correctly mostly by reading `git diff` and compiler output, not engr.

Turn numbers are per-session (`S2 T34`). "Record" means Objects/Sections. There were **0 Objects at every cut** (all three state dumps: `no objects`).

---

#### A. At each cut-off

##### S1 — cut at T71
**What it was doing at the cut.** At T69 `cargo check --bin` flagged main.rs, and at T70 the agent added the `Confirmed::ChangeSet` arm to main.rs's confirm handler. Its stated next step was "Now let's fix the test call sites (exhaustive `Confirmed` matches)" (T69).

**What it decided or found this session:**
- **D1** Object-only slice; cross-domain (Backlog) coordination deferred (T43).
- **D2** Core design (T42 contract text, T50 summary, T61 module doc):
  - Each change mints a real per-object Object Challenge.
  - A wrapper ChangeSet Challenge lists their codes in order.
  - Apply walks them through `gate::confirm_locked` under one lock, with a local journal for crash resume.
- **D3** At most one change per object per ChangeSet. Chained `expected_rev` was ruled out for v0 (T48).
- **D4** No read-side in-flight marker; a sequential, lock-serialized apply is accepted (T46). This departs from #41's "a reader must never observe a partially applied ChangeSet".
- **D5** `prepare_locked` was made `pub(crate)` because the workspace lock is non-reentrant (T54). `checked_review` was made `pub(crate)` for the write-nothing discovery call (T57).
- **Progress:**
  - Items 1–3 were finished: the lib compiled with 0 warnings (T67–T68).
  - Item 4 was half done: lib.rs was wired (T62–T66) and main.rs had the confirm arm (T70), but there was no CLI subcommand.
  - Item 5 was not started.

**How engr stood at the cut:**
| | In engr? |
|---|---|
| D1, D3 | In backlog §1/§3, but **filed as unresolved** even though both decisions had been taken and are enforced in the code. §1's settle clause *is* the decision: "Settled by building the Object-only slice first". |
| D4 | Backlog §2. This is a fair open question. |
| D2 | Sidecar summary. Present. |
| D5 | Not in engr; code comments only. Acceptable. |
| Progress | **Wrong.** Sidecar: item 1 `active`, items 2–8 `pending`, 0 done. At T52 the agent said "Now let me mark item 1 done" but ran `--state active`. Nothing was written again for 18 turns (`turns_after_last_engr_write: 18`). Items 2–3 done and item 4 half-done were unrecorded. |
| Done conditions | **Lost.** The T49 attempt gave every item a "done when …" clause. After the summary refusal (334 > 300 chars), the agent retyped the items at T51 without the clause on all but item 1 (`items_with_done_condition: 1`). |
| Commits | None. All code was modified or untracked. |

##### S2 — cut at T70
**What it was doing at the cut.** It was grepping main.rs for `backlog_command` so it could copy that pattern for a new `engr changeset` subcommand (T69–T70). It had just split `prepare()` into `build_action()` + `prepare()` (T51–T52, uncommitted) so that `changeset add` reuses the exact `prepare` flag vocabulary.

**What it decided or found:**
- Fixed 15 non-exhaustive matches (T16–T23).
- Re-pinned the generator fingerprint because the contract really had changed (T27–T31). It said "check the CHALLENGE digest contract version too" (T29); what it concluded is not visible.
- Committed da2f2cc (T37).
- CLI shape: new/add-object/remove/prepare/unprepare/show/ls (T34 attempted summary).
- The reason for the `build_action` split: #41 says a ChangeSet must not invent a second mutation language (T51 code comment).
- About 30 turns of research on the main.rs/gate.rs render, find, pending_codes and NeedsReview paths (T39–T70).

**How engr stood at the cut:**
- Items 1, 2, 3 and 5 were `done`, each with a result and commit da2f2cca (T34, T38). Item 4 was `active`. **Correct.**
- The fingerprint re-pin is in item 5's result. **Present.**
- The CLI subcommand list is **missing**. The T34 summary (~640 chars) was refused with exit 2, and the T35 retry cut it down to "Next: CLI subcommands, then tests/changeset.rs."
- The `build_action` refactor and the T39–T70 research are **missing**. Nothing was written after T38: 32 turns. A heartbeat arrived at T69, one turn before the cut.
- S1's design statement (D2) was **overwritten** by the T35 summary. From here on engr holds the core design nowhere; it survives only in the da2f2cc commit message and in code comments.
- The sidecar JSON was modified after the commit and left uncommitted.

##### S3 — cut at T90
**What it was doing at the cut.** The first compile of the new tests/changeset.rs failed: `AddOutcome` doesn't implement `Debug`, and there was an unused `Object` import (T88). It was locating `pub enum AddOutcome` to derive `Debug` (T89–T90).

**What it decided or found:**
- **CLI design** (T31):
  - `engr changeset` with New/Ls/Show/Add/Rm/Prepare/Unprepare; Add flattens `Prepare` and reuses `build_action`.
  - `--new` is already refused by `add_object_locked`'s NotFound path, so no extra CLI check is needed.
  - Bare `engr candidate` would misprint ChangeSet challenges as "unusable" unless it checks the challenge's kind first.
- **Built:**
  - `challenge_kind` (T34) and the `Changeset` command and handler (T35–T48).
  - `AddOutcome::NeedsReview.projected` (T46–T47).
  - `changeset::find` (T52) and a ChangeSet-aware `candidate()` (T54).
  - Build, the full test suite and fmt all passed (T55–T58), so **item 4 was finished.**
- **End-to-end smoke test** in a scratch workspace (T60–T67): a two-object apply under one CONFIRM worked, the per-object limit refused a second change, and unprepare/re-prepare worked.
- **Crash-recovery bug found and fixed** in `apply()` (T70–T71). If a crash landed after the journal was removed but before the wrapper Challenge was removed, a resume re-initialized a blank journal and reported the already-spent per-object codes as gone.
- `journal_path` was made `pub` (T82). tests/changeset.rs was written, covering "both windows" (T85–T87).

**How engr stood at the cut:**
- The CLI design is in the summary (T32), but **partial**. The 734-char item result at T31 was refused (limit 240), and the retry dropped both reasons.
- The summary is **stale**. It still says "Need: lib.rs helper to peek Challenge kind before gate::find; render_changeset_candidate() in main.rs", but both were written at T34 and T54.
- Item 4 is **wrong**: `active`, though it was built, tested and smoke-tested.
- Item 6 is **wrong**: `pending`, though the file was written and being compiled.
- The crash bug is **missing**. It is not in backlog, the sidecar or a commit; it exists only as a comment inside an uncommitted diff to changeset.rs.
- The smoke-test evidence is **missing**.
- Nothing was committed in S3. The last engr write was T32, 58 turns before the cut. The T64 heartbeat was ignored.

---

#### B. The probes

| Q | Probe 1 | Probe 2 | Probe 3 |
|---|---|---|---|
| 1 Settled | **Partly.** "Nothing is settled in the record" is true, but it cannot see that D1/D3 had been decided, because engr filed them as open. | **Partly.** Correct on the record and the commit. By now engr no longer holds D2 at all. | **Partly.** Correct on the record and the commit. It reads §1–§3 as staged decisions, which is closer to the truth. |
| 2 Open | **Partly.** Lists §1 and §3 as open, but both were decided. | **Partly.** Same backlog issue. It misses the undecided/unrecorded CLI shape. | **Partly.** Reports item 4 as open, but it was done. |
| 3 In progress | **Correct.** Found items 2 and 3 done in the tree though marked pending, item 4 half done, and item 5 not done (via the compiler). | **Correct.** Item 4 active; `build_action` split mid-way; no `Changeset` variant; no tests file. | **Partly.** Right about the tests file failing on `Debug`. It lists only lib.rs and main.rs as changed and **misses the modified changeset.rs** (crash fix, `projected`, `pub journal_path`). It says the CLI "hasn't been validated", but it passed the suite (T56) and was smoke-tested (T60–T67). |
| 4 Next | **Correct.** Fix the `tests/common/mod.rs` matches. That is S1's T69 intent and what S2 did at T16. | **Partly.** `grep pub fn changeset.rs` is a sensible first look, not the next edit (a `Changeset` variant modelled on Backlog, T69–T70). | **Correct.** Derive `Debug`, rerun tests, run fmt. Matches T88–T90. |
| 5 Ask | **Correct.** "Why are items 2–4 pending?" The answer is the T52 mis-tick. | **Correct.** Asks exactly what S2 left unrecorded: is `build_action`/`changeset_add_object` the chosen design? | **Partly.** "Should item 4 be done?" is the right question. "Was the missing Debug deliberate?" is noise. It asks nothing about changeset.rs because it never noticed the change. |
| 6 Stale? | **Correct.** Flags the sidecar item states as stale. | **Correct but shallow.** Flags the uncommitted sidecar. Doesn't say the summary no longer holds the design. | **Wrong.** Answers "No". It then concedes the summary lags and explains it with a made-up order ("already begun before the summary was last written"). In fact the summary is T32 and the code T34/T54. It misses items 4 and 6 and the unrecorded fix. |

**What the probes could not recover:**
- **Probe 1:** why the ticks were wrong. It recovered progress from `git diff` and `cargo check`, not from engr.
- **Probe 2:** the CLI plan (lost at S2 T35) and the purpose of `build_action`. Its only trace was a code comment naming a function that doesn't exist yet.
- **Probe 3:** the crash-window bug and fix, the smoke-test evidence, and item 4's completion. None were recorded.

**Did they notice stale or missing state?** Probe 1 yes, probe 2 partly, probe 3 no.

---

#### C. The successor sessions

**S2**
- **Turns to productive work:** 12 turns of orientation (T1–T12). T9–T12 went on `engr work show` and three `--help` calls; S2 never read the skill (`read_skill: false`). Productive work started at T13 (`cargo check --all-targets` exposed item 5), with the first fix at T16.
- **Redo or contradiction:** none. No path that had been ruled out was picked up again.
- **Repair:** it repaired S1's wrong ticks. T34 set items 1, 2, 3 and 5 to done and item 4 to active; T38 added results and commits. The results and commits came only after the commit hook prompted them (T37 → T38).

**S3**
- **Turns to productive work:** about 30. The first engr write was T31 and the first edit T34.
- **Re-reading:** it re-read the whole #41 draft (T1), SKILL.md (T2), #42 (T24) and all of changeset.rs (T4).
- **Redone research:** it then repeated S2's unrecorded T39–T70 research almost line for line:

  | What was read | S2 | S3 |
  |---|---|---|
  | `prepare()` | T45/T47 | T10 |
  | `render_candidate` | T55 | T13 |
  | `candidate()` | T59 | T14 |
  | gate `pending_codes`/`find` | T61 | T16 |
  | migration `challenge_path` grep | T63 | T18 |
  | `NeedsReview` | T66–T67 | T43–T44 |

- **Re-decided:** the CLI shape S2 had drafted and lost (S2 T34 → S3 T31). The result was consistent, with no contradiction.
- **Picked up correctly:** the uncommitted `build_action` split, found through `git diff` (T3).
- **Repair:** it put a CLI plan back into the summary (T32), but it never restored the core design lost at S2 T35. It also never committed S2's leftover dirty main.rs and sidecar.

---

#### D. The state against the four criteria

##### Backlog points
There are 3 points under one topic: 89, 95 and 108 words, each three sentences (question / rationale / "Settled by"). All three have a settle clause. Nothing was added after S1 T48.

- **Worst, §3:** "Whether v0's one-change-per-object-per-changeset restriction should be lifted once dogfooding shows a real need for several sequential mutations … Settled by a real workflow that needs it." The question contains its own answer, and the settle clause just restates the growth rule.
- **Also bad, §1:** a decision filed as a question: "Settled by building the Object-only slice first … and revisiting Backlog participation once that slice is dogfooded." The code already enforces it.
- **Best, §2:** "Whether ChangeSet apply needs true reader-side atomicity … or whether the sequential lock-serialized crash-safe apply this slice builds is sufficient." This is a real question, with a reason tied to storage (one rename per Object file). Its settle clause ("whether a real concurrent unlocked reader … ever actually causes confusion") is observable but vague, and nobody is assigned to watch for it.
- **Possible unasked question (auditor's observation):** the screen at T64–T65 lists the per-object codes DXH2LN and NLHMDV as pending beside the wrapper Y4DDGR. Inside an "applied together or not at all" screen, it prints "FOR A HUMAN … CONFIRM DXH2LN" for each change. The agent called this "exactly the intended UX". Whether confirming one per-object code alone breaks the set is not asked anywhere. I cannot tell from the log whether the code guards against it.

##### Work items
- **Granularity is good.** There are 8 items, one per file or step, plus a verification gate (7) and a record step (8).
- **Done conditions were lost.** At T51, 7 of 8 items lost their "done when" clause. Items 1, 5 and 7 still carry a checkable condition.
- **Ticking:**
  - S1 ticked 0 of the 3 finished items, and set the one it meant to close to `active`.
  - S2 batch-ticked 4 items after the fact, with results and a commit. This is the only correct ticking in the run.
  - S3 ticked nothing, although item 4 was finished and item 6 was underway.
- **Evidence is thin.** Item 5's result, "15 match sites fixed; fingerprint re-pinned; cargo test+fmt clean", names checks. Items 1–3 only restate the item, for example "prepare_locked, checked_review are pub(crate)".
- **State matched reality at 1 of 3 cuts** (S2).

##### Reasoning
- **Kept:** S1's ruled-out alternatives (read-side marker, chained `expected_rev`), with their reasons, in §2/§3.
- **Lost, one item at a time:**
  - D2 was overwritten (S2 T35).
  - S2's CLI list was cut at T34 → T35.
  - The reason for `build_action` is in code only.
  - S3's two CLI reasons were cut at T31 → T32.
  - The crash-window bug (S3 T70–T71), the most consequential finding in the run, was never recorded.
  - The smoke-test result was never recorded.
- **The pattern:** every refusal (S1 T49, S2 T34, S3 T31) was answered by keeping the *what* and cutting the *why*. The refusal messages point to backlog or the Object for reasoning; that advice was never followed.
- **Narrative dumped:** little. The S2 summary repeats item 5's history. The backlog's middle sentences are rationale, which is acceptable.

##### Record Sections
None, in any session. Item 8 ("record settled ChangeSet v0 design in the Object record via prepare --agent") was scheduled last and never attempted. No subagent review was run (`subagents: 0`). The settled design ended the run in four places:
- two backlog points marked `unresolved`;
- one commit message;
- code comments;
- an uncommitted diff.

All three probes answer "what is settled?" with "nothing".

---

#### E. Cost

| | Turns on engr | Share | Refused writes |
|---|---|---|---|
| S1 | T43–T52: 10 of 71 | 14% | 2 (T44: needed `--expect`, which cost two JSON round trips at T45/T47; T49: summary 334 > 300, which also failed 7 item adds) |
| S2 | T9–T12 (reads/help), T34, T35, T38: 7 of 70 | 10% | 1 |
| S3 | T2, T31, T32: 3 of 90 | 3% | 1 |

- **Totals:** about 20 of 231 turns (~9%). No reviews, no subagents, no record writes. The metrics' `engr_write_turns` for S2 counts the three `--help` calls as writes; the real writes were T34, T35 and T38.
- **Proportion:** bookkeeping did not crowd out the work; it was **too little and poorly timed**. There was one burst per session, and the silent stretch before each cut grew from 18 to 32 to 58 turns.
- **Where the cost went instead:** re-orientation. S2 spent 12 turns and S3 about 30, of which about 20 repeated S2's reading. That re-orientation (~40 turns) is about twice the entire bookkeeping spend.

---

#### F. Verdict

| Criterion | Score | Why |
|---|---|---|
| 1 Backlog points | **3/5** | Consistently question/reason/settle, but one is a decision in disguise, one is tautological, each runs ~100 words, and nothing was added after S1. |
| 2 Work items | **2/5** | Sound breakdown, but done conditions were dropped at T51 and the ticks were wrong at two of three cuts; the only correct ticking was S2's after-the-fact batch. |
| 3 Reasoning in progress | **2/5** | S1's ruled-out paths were kept, but the core design statement, S2's CLI plan and S3's crash-window finding all ended outside engr, and every length refusal cut the reason. |
| 4 Resume | **3/5** | Every probe named a correct or near-correct next step, but from `git diff` and compiler output rather than engr. Probe 3 declared nothing stale and missed the crash fix. S3 repeated ~20 turns of S2's research. |

##### Most important failure modes (most damaging first)

1. **Settled decisions never reached the record; they were misfiled or overwritten.**
   - 0 Objects at all three cuts, and item 8 was pending throughout.
   - D1 and D3 sit in backlog as `unresolved` although the code enforces them (§1: "Settled by building the Object-only slice first").
   - The one place the core design was stated (S1 summary, T50) was replaced at S2 T35.
   - All three probes answer "settled?" with "nothing".
2. **The sidecar lagged reality, and ticking happened in bursts or not at all.**
   - S1 T52: "mark item 1 done" was run as `--state active`, and items 2–3 were never ticked.
   - At the S3 cut, item 4 was finished (T55–T67) but `active`, item 6 was `pending`, and the summary listed as "Need" two things already built.
   - The silent stretch before each cut was 18, 32 and 58 turns. The T64 heartbeat was ignored.
   - There was 1 commit in the whole run.
3. **Findings were lost, to length limits and to silence, and successors paid for it.**
   - The S3 crash-window bug and fix (T70–T71) was never recorded, and probe 3 did not find it.
   - Reasons were cut at S2 T34 → T35 and S3 T31 → T32.
   - Done conditions were dropped at S1 T51.
   - Because S2 recorded nothing after T38, S3 re-did its research (C, table).

---

## Label T — arm E

### Audit: engr as working memory, ChangeSet v0 run

Material: S1/S2/S3 logs, state dumps, probes, metrics in this directory. Turn
numbers are the log's `T` labels. "~" marks my own counts.

**Short version.** The engineering went well: the slice was implemented,
tested, committed and never redone. Keeping state in engr went worse. The
design rationale was written once, into a summary field that gets replaced,
and was erased 34 turns later. The sidecar was stale at both forced cut-offs.
Recording decisions was left as the last work item, so at the S1 cut the
record held 0 Sections, and at the S2 cut it held 1 of the 5 that had been
drafted. The successors resumed correctly mostly because the code comments,
commits and tests carried the state, not engr.

---

#### A. At each cut-off

##### S1 — cut at T70 (max turns)

**What it was doing.** At T69 it wrote `changeset_command` and
`render_changeset_show` in main.rs, and at T70 `cargo build` finished clean.
The CLI (item 3) was written and compiling but untested, and it never ran
`cargo fmt`. Its stated next step (T55 summary): "wire engr changeset CLI in
main.rs, then tests/changeset.rs".

| Decided / found / ruled out in S1 | In engr at the cut? |
|---|---|
| Only Backlog, only `section.create`/`section.update` (T41 doc comment) | Only by implication: the object title, and backlog §1's "once ChangeSet v0 (Backlog-only)…". No Section. |
| Refuse if a Backlog Rule governs; one lock hold per apply; preflight then apply; resume from domain state, "(no separate journal)" (T19) | **Lost.** It was in the T21 summary ("Design: backlog-domain intents only, refuse if a backlog Rule governs, one lock hold per apply, preflight-then-apply, idempotent resume via domain state"). T55 `work summary` *replaced* it with "backlog.rs+changeset.rs implemented and compiling (0 warnings), full suite green. Next: wire engr changeset CLI…". The clause "no separate journal" had already been cut at T21 to fit the 300-character limit. At the cut, this reasoning existed only in changeset.rs comments. |
| flock is not re-entrant, so `edit` splits into `edit`/`edit_locked` (T4, T26) | Only the *what*: item 1 says "split backlog::edit into edit()+edit_locked()". The reason is not recorded. |
| Freeze-time projection not done (forward-dependency limit) | Yes, backlog §2, with the trade-off. |
| One file per ChangeSet, with its own digest family (T34–T36) | Yes, backlog §3. |
| One Challenge vs one per action, for Object ChangeSets | Yes, backlog §1. |
| CLI wiring written and building (T56–T70) | **Missing.** Item 3 is "active" and the summary says "Next: wire…". 15 turns of work went unrecorded (metrics: `turns_after_last_engr_write` 15). |
| fmt not clean | Unknown to the agent. The probe found it. |
| Nothing committed, `.engr` untracked | Visible in git. engr says nothing about it. |

The changeset.rs comment (T41) says "see the backlog point this object's work
sidecar links". The sidecar never linked any backlog point (no `work depend`),
at this cut or any later one. No probe noticed this.

##### S2 — cut at T70 (max turns)

**What it was doing.** It had just admitted §1, the scope Section (T70). That
took two cold reads, a review that failed at T65, and a review that passed at
T68. At T62 it had written claim notes for four more Sections: locking,
two-pass/no-journal, no projection, and governed refusal. They were in the
session scratchpad, outside the repository. Its plan was in the summary
written at T42, which never saved: "record settled decisions as Sections…,
file open questions (Rule-governed multi-change composition, other domains
beyond Backlog, in-batch projection) in backlog, then commit."

| Decided / found in S2 | In engr at the cut? |
|---|---|
| Digest double-wrap bug found and fixed (T20–T29) | Yes, but filed as item 5's result. Item 5 is "cargo test… fmt… pass; commit", not the bug. |
| CLI smoke-tested end to end (T35–T39) | Yes, item 3's result. A good entry. |
| Code committed as 2cfad04 (T52) | **Missing** from items. The T52 hook said to "item commit --commit HEAD" and was ignored. Metrics S2: `commits_after_sidecar` = [2cfad04]. |
| `implemented_by` is refused on Agent Sections (T56–T60) | **Missing.** Item 6 still says "with implemented_by". |
| Four drafted decisions (T62) | **Missing** (scratchpad only). |
| New open questions it planned to file (T42) | **Missing.** The command carrying them aborted. |
| Item 6 under way, §1 admitted | **Wrong.** Item 6 is "pending", and the summary is still S1's "Next: wire engr changeset CLI in main.rs, then tests/changeset.rs", both done long before. |

**How the sidecar went stale:**
- At T42, an `&&` chain failed at the 240-character limit on results, so
  "item 6 active" and the new summary never ran.
- At T47 the agent said "All 5 steps recorded" without checking.
- At T8 it tried to leave a resume note with `engr work update --note`, a
  subcommand that does not exist, and never retried.
- Item 5 ("…; commit") was ticked done at T44, 8 turns before the commit
  (T52).
- §1 was admitted but not committed.

##### S3 — ended on its own at T51

**What it did:**
- Admitted §2–§8 (T14–T29), after splitting the persistence claim when review
  failed it (T13, T23).
- Created collection `changeset-v0` (T33–T35).
- Closed item 6 (T36, T42) and rewrote the summary (T37).
- Committed `.engr` three times (T41, T43, T49).
- Ran a resume drill (T45) and patched the collection description as a result
  (T48).

At the end it intended the next session to "dogfood this slice per #42
Phase 4".

**In engr at the end:** 8 Sections, an accurate summary, all 6 items done,
and the collection.

**Missing:**
- The Rule-Review-composition question S2 meant to file. §6 even says "this
  slice does not compose Rule Review across several intents", but nothing is
  in backlog.
- The T30 intent to "update the backlog wording of §2/§3 to point at these
  new settled sections" was dropped without a word.
- There is no item for the stated next step (dogfooding).

**Wrong or stale:**
- Item 6 ("…with implemented_by; file open questions in backlog") is ticked
  done, though neither half happened as written. The result explains only the
  implemented_by half.
- Item 6 carries 2cfad04a, which predates its work, beside 85cf6f85.
- Items 1–5 have no commit (metrics: `done_without_commit` 5).
- Item 5's result is still about the bug.
- The sidecar State is "active" with 0 open items.
- The collection description says rebuilding the PATH engr is something
  AGENTS.md forbids (T49 commit message). I cannot verify that from this
  material.

---

#### B. The probes

**Probe 1 (after S1)**

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Partly correct | Right that the record is empty. From code it recovered Backlog-only, the forward-dependency limit and file+digest. It missed the four decisions the summary held until T55. It calls "no new SubjectType" decided, while backlog §1 holds it open. |
| 2 Open | Correct | The 3 backlog points, items 4–6, and that no tests exist. |
| 3 In progress | Correct in substance | Item 3 is done in code and stale in the sidecar; fmt fails. The file list is probably wrong: it names changeset.rs and main.rs, but S2 T6 shows the first diff in backlog.rs, and S2 T8 says "backlog.rs/changeset.rs". |
| 4 Next | Correct | `cargo fmt`. S2 did exactly this at T7. |
| 5 Ask | Correct | "Whether Backlog-only v0… are the intended v0 scope" is what the erased summary answered. |
| 6 Stale | Correct | Item 3 and the uncommitted `.engr`. It missed the code comment pointing at a sidecar link that does not exist. |

It could not recover the erased design reasoning. It did notice that the
record was empty and that item 3 was stale.

**Probe 2 (after S2)**

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Correct | 2cfad04 and §1. |
| 2 Open | Partly correct | The backlog and item 6 are right. The four drafted decisions and the planned questions were invisible to it. |
| 3 In progress | Partly correct | Item 6 unfinished is right. It treats the missing `implemented_by` as a gap to fill, which is impossible (T56), because item 6's text misled it. It also invents a `backlog produced` requirement. |
| 4 Next | Partly correct | Committing `.engr` is safe, but the agent's plan was to admit §2–§5. |
| 5 Ask | Correct | Its two key questions (was implemented_by deliberately omitted; were more Sections meant?) were answerable from S2's context and absent from engr. |
| 6 Stale | Partly correct; one hallucination | It caught item 6 and the uncommitted §1, but not the summary two phases stale. It says `git log --since "2026-09-24T17:18:35Z"` returns nothing, yet 2cfad04 is at 17:19:28 (state-S2). So it missed that items 1–5 lack the commit. |

**Probe 3 (after S3)**

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Correct | |
| 2 Open | Correct as far as engr goes | It cannot see the unfiled Rule-Review question. |
| 3 In progress | Partly correct | "All done" is right. "No target/ directory… so no local build has ever been run here" and "Dogfooding has not started" overreach: the agent built repeatedly and smoke-tested the CLI end to end (S2 T35–T39, recorded in item 3). |
| 4 Next | Correct | Matches the summary and the collection. |
| 5 Ask | Partly correct | The second question (does record-mss relate to backlog §1?) is confused. |
| 6 Stale | Mostly wrong | It calls 67ceab2 "the one stale thing" in the sidecar, but that is a collection-description commit, so this is a false positive. It missed the real ones: item 6's wording vs what happened, missing commit pointers, "active" with 0 open items, item 5's misfiled result, and the code comment's phantom sidecar link. |

**Pattern.** Each probe verified against code and git, and was accurate
wherever the repository spoke. Where only the agent's head held the answer
(erased design reasoning, S2's drafts and plan, the implemented_by refusal),
the probe either asked or guessed.

---

#### C. The successor sessions

**S2**
- **Orientation:** T1–T6 (git status, build, diffs, full read of
  changeset.rs, test suite, fmt check). This was justified, because the
  sidecar said item 3 was still active.
- **First productive change:** T7 (fmt fix). New work started at T19
  (tests).
- **Redo, re-decision, contradiction:** none. It re-derived S1's erased
  design reasoning from the code comments (T62 notes), consistently with S1.
- **Repairs:** fmt (T7) and item 3 (T42–T43).
- **Failed repairs:** the T8 resume note used a subcommand that does not
  exist, and the summary was never fixed.

**S3**
- **Orientation:** T1–T7, including a full re-read of SKILL.md (T3), #41 and
  #42 (T5), and changeset.rs (T6).
- **First productive action:** T8.
- **Picked up a path already ruled out:** at T8, `--implemented-by-symbol`
  was refused with the same "relations are human-authoritative" error S2 hit
  at T56.
- **Redid work:** it redrafted the decisions S2 had already drafted at T62
  (S3 T7 notes).
- **Contradictions:** none against engr. Its §5 reason differs from S2's
  unrecorded draft. S2 note-4 had "would validate against state nothing else
  can check once frozen"; §5 says "logic this slice does not implement or
  verify". S3 had no way to know.
- **Repairs:** item 6, the summary, uncommitted `.engr`, a planning
  collection.
- **Not repaired:** commits on items 1–5, item 5's result, the lost question.

---

#### D. The state against the four criteria

**Backlog points.** There are 3, of 80–103 words and 3 sentences each
(question, trade-off, "Settled by…"; metrics `backlog_with_settle_clause` 3).
Each is one real question with named alternatives.

Weaknesses:
- They are long.
- The settle conditions are "after enough dogfooding" with no threshold.
- The topic title, "ChangeSet: composing Object/Human-Gate mutations", fits
  only §1.
- Nothing was added after T25, though new questions came up in S2.

- **Best, §1:** "Whether a ChangeSet spanning Object mutations should mint
  one Challenge… or require one Challenge per Object action". It is a clean
  binary with a concrete thing to consult (confirmation.rs's SubjectType
  comment).
- **Worst, §3:** "Whether ChangeSet's local file format… should move to an
  embedded database once ChangeSets are numerous, matching #41's explicit
  deferral". It anticipates a problem that does not exist yet, restates a
  deferral that already exists, and is "Settled by observed ChangeSet volume
  and apply latency after dogfooding v0" with no number.

**Work items.** Six items, planned once at T21 and never revised.

- **Too coarse.** Item 2, "write crates/engr/src/changeset.rs: model, freeze,
  apply, persistence", is the whole feature (760 lines). Item 6 bundles two
  jobs, one of them impossible.
- **No done conditions** at any cut (metrics `items_with_done_condition` 0).
- **Ticking** lagged at both forced cuts (S1 item 3, S2 item 6) and was
  early once (item 5 at T44 vs the commit at T52).
- **Evidence** in results is mostly good: item 3's smoke test ("both landed
  atomically"), item 4's six named tests. Item 5 is misfiled.
- **Commits:** 1 item of 6 carries one.
- **Summary:** the same text from S1 T55 to S3 T37, across two cut-offs
  (metrics `summary_chars` 148 at both S1 and S2).

**Reasoning.**
- *Findable:* the S1 backlog trade-offs, the bug diagnosis (item 5 result),
  the implemented_by limit (only S3's item 6 result), and the PATH-binary
  rule (collection).
- *Lost:* S1's design reasoning (overwritten at T55), S2's implemented_by
  finding, S2's four drafts, and S2's planned questions.
- *Narrative dumped:* little. The collection description (T48) grew into an
  operating instruction.

**Record Sections.** There are 8, of 32–76 words.
- **Strongest:** §2 (locking), because it gives the mechanism ("flock that
  is not reentrant… would deadlock"). And §4, which keeps the rejected path
  and why: "not by a separate progress record — a separate journal could
  itself fall out of sync".
- **Weakest:** §5 (76 words) links three claims, and its reason, "because
  projecting… is logic this slice does not implement or verify", is scope,
  not a reason. §6's first half has the same flaw.
- **Timing:** all were written after the code was committed (S2 T70, S3
  T14–T29). The record documented decisions afterwards; it never held them
  while the work was in flight.

**Review process.**
- The reviewer prompts carried the author's steering. At S2 T68, after a
  failure, it added a "Note on item 1… do not fail item 1 merely because a
  reason clause exists". At S3 T13 and T26 it added a note excusing item 6.
- At S3 T15–T22, the passes for §3–§6 were attested with digests minted
  after the earlier admissions. The reviewer had seen each candidate beside
  §1 only, so rule item 5 ("does not repeat another live Section") was never
  checked among §2–§6.

---

#### E. Cost

| Session | Turns | Turns on engr (~) | Share | Subagents | Cost |
|---|---|---|---|---|---|
| S1 | 70 | ~12 (T15–T25, T55) + guide reading in T1–T3 | ~20% | 1 (engineering) | $5.42 |
| S2 | 70 | ~31 (T8–T10, T40–T49, T53–T70) | ~44% | 4 (all cold read/review) | $2.54 |
| S3 | 51 | ~46 (no code changed at all) | ~90% | 6 (all cold read/review/drill) | $2.41 |

Overall, about 89 of 191 turns (~47%) went to engr.

- **One Section, 18 turns.** The single 32-word Section §1 cost 18 turns and
  4 subagents (S2 T53–T70).
- **Rejected writes.** At least 5 writes were rejected: S1 T19–T20 (subject
  format, length), S1 T23 (`--expect`), and S2 T42 and T45 (length limits).
  S2 T43 was probably a sixth; T44 re-sends its text shortened.
- **SKILL.md** was read in full three times (S1 T1, S2 T48, S3 T3).
- **Proportion.** The engineering was not crowded out: the slice was done by
  S2 T52. But the effort was misallocated. About 75 turns went to polishing
  admitted prose afterwards. Almost nothing went to the cheap sidecar and
  backlog writes that actually carry resume state.
- **Timing.** Recording decisions was the final item. That is why S2's
  cut-off fell in the middle of recording.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1 Backlog points | 3/5 | Each is one real question with alternatives and a "Settled by" clause, but at 80–103 words with no thresholds, and the backlog stopped growing at T25 though new questions arose. |
| 2 Work items | 2/5 | Six coarse items with no done conditions, one impossible. Ticked late at both cut-offs and early once, one result misfiled, 5 of 6 without commits, and a summary stale across two cut-offs. |
| 3 Reasoning in progress | 2/5 | The design reasoning went into a summary that gets replaced and was destroyed 34 turns later. S2's key finding and drafts never reached engr, so S3 repeated a refused path. |
| 4 Resume | 3/5 | Successors never redid code or contradicted a decision, but mainly because code comments, commits and tests carried the state. engr alone would have sent S2 toward finished work and hid S2's plan from S3. |

**Most damaging failure modes**

1. **Decisions were left for a closing "record" item.** The reasoning held in
   the meantime lived in places that get overwritten or are outside the
   repository.
   - The T21 summary carried the design; T55 replaced it.
   - The record held 0 Sections at the S1 cut and 1 at the S2 cut.
   - S2's T62 notes were in its scratchpad.
   - Probe 1: "Nothing at the record level yet". Probe 2 had to ask
     "whether item 6's plural 'decisions' meant more than the one Section".
   - S3 T7 redrafted the same material.

2. **Sidecar writes were not made when state changed, and not checked after
   they were made.**
   - The summary "Next: wire engr changeset CLI in main.rs, then
     tests/changeset.rs" survived from S1 T55 to S3 T37.
   - The T42 `&&` chain died on a length limit, dropping "item 6 active" and
     the new summary; T47 then claimed "All 5 steps recorded".
   - The T8 resume note used a subcommand that does not exist.
   - Item 3 stayed "active" after it was done (S1 cut), and item 6 stayed
     "pending" after §1 was admitted (S2 cut).
   - The commit hooks at S2 T52 and S3 T41 went unanswered for items 1–5.

3. **Findings and questions from mid-session were not captured.** This led to
   one repeated failure and one lost question.
   - The implemented_by refusal (S2 T56–T60) was not recorded, so S3 T8 hit
     the identical error.
   - "Rule-governed multi-change composition" (S2 T42 plan) was never filed.
     §6 openly defers it, no probe could see it, and S3 left the backlog
     "as-is since they're genuinely still open" (T51).

---

## Label U — arm F

### Audit: engr as working memory across three cut-off sessions (run U)

What engr held, in one line: **one Object with 0 Sections, 0 backlog points and
0 collections at all three cut-offs.** The only working state was one work
sidecar: 9 items plus a summary. Every design decision the agent made is kept in
commit messages, code comments, or nowhere. The successors picked up correctly
because the repository showed what state it was in (git status, a failing build,
failing tests), not because engr told them.

---

#### A. At each cut-off

##### S1 (cut at T70, 71 turns, no code written)

**Doing at the cut.** Reading code before implementing. At T43 it said: "look at
model.rs's `Payload`/`Action` and gate.rs's `Prepared`/`Candidate` types
precisely". T44–T70 read gate, model, main, semantics, backlog and ops. The cut
came at T70 while it was reading `ops::admission_predecessor`/`reconcile`. Its
next step was to edit for item 2 (store paths and lib.rs) and then write
changeset.rs.

| Decided / found / left open in S1 | In engr at the cut? |
|---|---|
| v1 scope is exactly 1 Human-gated Object mutation plus 1 Backlog consume (T42) | Sidecar summary only, with no reason. **Not in the record.** The reason ("rather than inventing a new confirmation primitive or a cross-file WAL") is only in the message of commit 52afc36 (T43). |
| Persist in `.engr/local/changesets` (not tracked by git) | Sidecar summary only. No reason given. |
| Reuse `gate::confirm_locked`; backlog `Precondition`+token is the freshness check; no multi-file atomic primitive exists | Item 1 result, one line. |
| **Ruled out:** a cross-file WAL or journal | Commit message only. **Contradicted in engr**: item 5 reads "apply() revalidate-then-apply both, **journal**, crash-resume". |
| `CandidateState::AlreadyApplied` covers the window between a durable Event and removal of the Challenge. This is what the crash-resume design later rests on (T40). | Missing |
| API findings from T44–T70: Payload/Action shape, `discard_locked`, `confirm_locked` is `pub(crate)`, `admission_predecessor`, `content()` defaults `based_on` to HEAD | Missing |
| Output of the research subagent (T3) | Missing. It is not in the log either, so it was lost. |
| Open: a journal, or recovery derived from existing state? | Not staged. The backlog was empty. |

**Wrong or stale in engr:** item 1's commit is `58608be1`, the commit that
opened the Object (T13). It is not evidence of research. Item 5's "journal"
disagrees with the reasoning in the commit message. Item 2 was "active", but no
edit had been made. That is minor, because the agent was still reading.

##### S2 (cut at T70, 71 turns, **zero engr writes**, nothing committed)

**Doing at the cut.** T69 ran `cargo test --test changeset`, which failed to
compile. T70 fixed `after.sections.as_ref()`. Its next step was to re-run the
tests and then wire the CLI (T62: "write the tests before wiring the CLI").

| Decided / found in S2 | In engr at the cut? |
|---|---|
| Crash-resume is derived from whether the Challenge file is present plus the Event history, "rather than a new journal" (T38) | **Missing.** Item 5 still says "journal". |
| The Object half is an ordinary Object Challenge, with no new Challenge family (T38) | Missing |
| `apply` confirms the Object, then consumes the Backlog point, under **one** lock (T38) | Missing |
| `with_lock` is a non-reentrant OS flock, so `consume_section_locked` is split out (T48) | Missing. It exists only as a code comment. |
| `same_act` becomes `pub(crate)` for `find_admission` (T49); `Precondition` gets `Deserialize` (T53) | Missing |
| Item 2 finished at T42; items 3, 4, 5 and 8 drafted (changeset.rs is 659 lines, tests are 284 lines) | **Stale**: item 2 "active", 3–8 "pending", summary unchanged since S1 |
| Tests not yet run successfully | Not recorded |

At T45–T46 it ran `engr work --help` and `engr work item --help`, and then
never wrote anything. The metrics file counts these two turns as
`engr_write_turns: [45, 46]`. They were read-only help calls, so do not trust
that metric. There were 7 dirty files and 0 commits (metrics-S2
`dirty_outside_engr`).

##### S3 (cut at T90, 91 turns)

**Doing at the cut.** Wiring the CLI (item 6). It had extracted
`payload_from_args` (T66–67), extracted a shared `ReviewAttestationArgs` (T75),
and added `Command::Changeset` and the `Changeset` enum (T87–89). At T90 it was
adding `reject_admission_flags`. There was no dispatch arm yet, so the tree did
not compile (confirmed by the probe's `E0004`). Its next step was the dispatch
arm and handler, then item 7 and item 9.

| Decided / found in S3 | In engr at the cut? |
|---|---|
| Serde bug: `deny_unknown_fields` together with `flatten` on `Change`; fixed by dropping the attribute on `Change` and keeping it on `Intent` (T6–7) | Summary, briefly. Commit 40aefa4, in full. |
| Deadlock from a nested flock: `changeset::prepare` called the public `gate::prepare` while holding the lock; fixed by using `gate::prepare_locked` (T23–37) | Summary, briefly. Commit 40aefa4. |
| Items 2–5 and 8 done and verified (10/10 tests, full suite green, T40–42) | Ticked, each with a result and commit 40aefa4c (T48–49). Correct. |
| CLI design: `add-object`/`revise-object` reuse `Prepare`'s flags to build a Payload; admission flags are refused there and belong to `changeset prepare`; review flags are shared through `ReviewAttestationArgs` (T66–90) | **Missing.** It exists only as doc comments in the uncommitted main.rs. |
| Working tree is mid-refactor and does not compile | **Missing.** The summary says "full suite green", which was true of the commit and false of the tree. |
| Sidecar edits from T48–51 | **Not committed** (`.engr/work/...json` is modified in git status) |
| Item 5 text "journal" | **Stale.** Ticked done with a result that describes no journal. |
| Item 9 "record design decisions as Sections" | Still pending, the third session in a row |

At T18 the Stop hook fired: "record a decision that settled, and stage a
question you are leaving, in the backlog". The agent later updated only the
sidecar. It never recorded a decision or staged a question.

---

#### B. The probes

**Probe 1** (after S1)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Correct | "0 sections"; the scope exists only in the sidecar and the message of commit 52afc36 |
| 2 Open | Partly correct | The backlog is indeed empty, but it lists the pending plan items as "open" and misses the unresolved question of a journal versus derived recovery |
| 3 In progress | Correct | Item 2 active, no code yet (checked with grep and git status) |
| 4 Next | Correct | Start item 2 in store.rs/lib.rs. This is what S2 did at T39–42. |
| 5 Ask | Reasonable | Whether the scope is final, given it is not admitted |
| 6 Stale | Partly correct | Says "item-1 commit 58608be1 … check out", but that commit is the one that opened the Object, not evidence. Misses the conflict between "journal" and the commit's "no cross-file WAL". Correctly flags that decisions are missing from the record. |

It could not recover the research findings, the API reading from T44–T70, or
why v1 was scoped that way. None of these were written down.

**Probe 2** (after S2)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Partly correct | Right that the record is empty. It does not surface S2's own decisions (no journal, one lock, derived crash-resume), which exist only in the code. |
| 2 Open | Partly correct | Same limitation as probe 1 |
| 3 In progress | Correct | Sidecar stale; items 3–5 coded but uncommitted; tests failing with `unknown field 'kind'`. S3 at T5–6 found the same serde bug. The "9 of 10" count cannot be verified (S3's output was truncated). It could not see the deadlock, because the serde failure hid it. |
| 4 Next | Correct | Fix the serde bug first. S3 did exactly this at T6–7. |
| 5 Ask | Correct | "Was cargo test ever run?" It had not been run successfully (T69–70). |
| 6 Stale | Correct | Identified the stale item states |

**Probe 3** (after S3)

| Q | Grade | Note |
|---|---|---|
| 1 Settled | Correct | Record empty; decisions are in the commit and summary |
| 2 Open | Partly correct | Pending items 7 and 9 only; no open questions staged anywhere |
| 3 In progress | Correct | Item 6 unfinished; it built the tree and got `E0004` for the missing `Command::Changeset` arm, which matches T87–90 |
| 4 Next | Correct | Add the dispatch arm and `changeset_command` |
| 5 Ask | Partly correct | Asks whether the `ReviewAttestationArgs` refactor was intentional. It was (T75), and the reason is in a doc comment in the diff. The question comes up only because engr says nothing. |
| 6 Stale | Correct | Sidecar and main.rs uncommitted. It did not say that the summary's "full suite green" is untrue of the tree, or that item 5 says "journal". |

**Pattern.** None of the probes hallucinated. All three got "what next" right, in
every case from `git status`, `git diff`, or running the build and tests, not
from engr. All three flagged the empty record. None could report the reasoning
or the rejected paths, because none had been written down.

---

#### C. The successor sessions

**S2**
- **Turns to productive work:** first edit at T39. T1–T38 were re-orientation, 54% of the session.
- **Redid work:** it repeated S1's research nearly one for one. S1 T67 and S2 T30 ran the same grep (`fn prepare_locked|fn mint(`); S1 T69 and S2 T31 both looked up `human_confirmation`; S1 T65 and S2 T27 both looked up `new_id`; S1 T56–57 and S2 T21 both read `discard_locked`; S1 T33 and S2 T25 both read `consume_section`; S1 T35 and S2 T13 both read `Precondition`. It re-read the whole of store.rs, gate.rs and confirmation.rs (T9–T12). Item 1's one-line result could not stand in for any of this.
- **Re-decided or contradicted:** it silently reversed item 5's "journal" to "rather than a new journal" (T38) and updated nothing. It picked up no path that had been ruled out.
- **Repaired gaps:** none. It did not record decisions, did not tick item 2, did not update the summary, and did not commit.

**S3**
- **Turns to productive work:** it fixed the serde bug at T6–7 after reading the diff and running the tests (T1–T5). This was efficient.
- **Redid work:** it re-read SKILL.md (T1), as S2 did, but did not redo the design. T8–T27 went to fighting background test runs that hung on the deadlock S2 had introduced.
- **Re-decided or contradicted:** nothing.
- **Repaired gaps:** partly. It noticed that "the sidecar undersold progress" (T17) and ticked items 2–5 and 8 with evidence (T48–49), but only after committing, and it never committed those ticks. It did not correct item 5's wording. It did not record Sections or stage backlog points, even though the Stop hook asked it to at T18. It said "add a new item documenting the deadlock/serde bugs" (T49) and then did not.

---

#### D. The state against the four criteria

**Backlog points.** There were none at any cut ("nothing unresolved"; metrics
`backlog_points: 0`), so there is no best or worst to quote. The run did have
open questions that belonged there:
- journal or derived recovery (open at the S1 cut)
- whether and when to widen beyond one Object plus one Backlog change
- Phase 5 `engr mcp`, which the task says comes only once ChangeSet works
- what the normative PROTOCOL.md section must say (item 7)
- whether the Object half may go through the Agent path

Each of these was resolved in code or not at all.

**Work items.**
- **Granularity:** acceptable, roughly one file or one function per item.
- **Best:** item 1, "research store/gate/confirmation internals; done when findings summarized". It is the only item with a completion condition (metrics `items_with_done_condition: 1`). Item 8's result is the best evidence: "10 changeset tests pass: cargo test --test changeset -> 10 passed; 0 failed".
- **Worst:** item 9, "record design decisions as Sections on 01a0d457; cargo test+fmt; commit". It is three steps in one, and it turns recording decisions into a task for the end. It was pending at every cut.
- **Wrong when ticked:** item 5 was ticked done while still reading "journal". Item 3 ("Active CRUD (new/add/revise/rm/show/ls)") was ticked on evidence from a single persistence test, cited as "a_changeset_survives_being_read_back...".
- **Ticking latency:** item 2 was finished at S2 T42 and ticked at S3 T48, two sessions later. Items 3–5 and 8 were ticked retroactively, in bulk.
- **Evidence pointers:** item 1's commit is unrelated to the research.

**Reasoning.** The rejected paths were:
- a new confirmation primitive
- a cross-file WAL
- calling public `gate::prepare` from inside the lock
- `deny_unknown_fields` on `Change`

All four are in commit messages 52afc36 and 40aefa4, or in code comments. engr
holds at most a clause of each in the summary. S2 knew the lock is not
reentrant, and wrote it in a comment at T48. Ten turns later it deadlocked
`changeset::prepare` on exactly that. The research subagent's output was lost.
**No narrative was dumped into engr.** Every engr text is terse, which is the
one clear positive here.

**Record Sections.** There are none, so none can be judged. The only record
mutation was the creation of the Object's title, which the agent reviewed and
passed itself at attempt 1 (T10), with no independent reviewer.

---

#### E. Cost

| Session | Turns | engr turns (reading the guide, rules, show, sidecar writes, engr commits) | Share | Engineering |
|---|---|---|---|---|
| S1 | 70 | ~11 (T4, T7–13, T41–43) | ~16% | ~57 turns of reading, 0 lines of code. It dispatched a research subagent (T3) and then read the same files itself (T15–40). |
| S2 | 70 | ~5 (T6, T18–19, T45–46), 0 writes | ~7% | 38 turns re-orienting, ~27 coding |
| S3 | 90 | ~8 (T1, T10, T44, T48–51, T82) | ~9% | ~20 turns on hung background tests, the rest fixing and wiring |

About 24 of 230 turns (roughly 10%) went to engr. **Bookkeeping did not crowd
out the work. It was under-invested.** The expensive waste came from the thin
state: S1 duplicated its own subagent, S2 spent 38 turns re-reading, and S3 spent
about 20 turns on a deadlock that S2 created while knowing its cause. A few
turns spent recording findings and decisions would have saved most of that.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1 Backlog points | **1/5** | Not one point was written in three sessions, although real open questions existed and each was settled silently in code. |
| 2 Work items | **2/5** | The plan was sensible, but only 1 of 9 items had a completion condition. The S2 cut was stale on 5 of 9 items, ticks lagged by up to two sessions, item 5 was ticked against wording it contradicts, and the S3 ticks were never committed. |
| 3 Reasoning in progress | **2/5** | Conclusions and rejected paths live in commit messages and code comments, engr holds one-liners, and S1's research had to be redone. It avoided narrative dumping. |
| 4 Resume | **3/5** | Every probe and successor picked up correctly, but that came from the repository (git status, broken build or tests). engr was wrong at the S2 cut, and S2 needed 38 turns to become productive. |

**Most important failure modes, most damaging first**

1. **Decisions were never admitted. "Record them later" became "never".**
   - The record had 0 Sections at every cut (state-S1, S2 and S3; metrics `sections: 0`).
   - Item 9, "record design decisions as Sections…", was pending from S1 T42 to the end.
   - Decisions went unrecorded: v1 scope, local storage, no journal and derived crash-resume, single-lock composition, `prepare_locked`, the shape of the CLI flags.
   - All three probes flagged this. The S3 Stop hook (T18) asked for it explicitly and was ignored.

2. **A whole session left no trace in engr (S2).**
   - 70 turns produced 0 engr writes and 0 commits, with 7 dirty files at the cut.
   - The sidecar still said item 2 "active" and items 3–8 "pending" after item 2 was finished (T42) and items 3–5 and 8 were drafted (T58, T68).
   - Its design reversal, "rather than a new journal" (T38), contradicted item 5 and was recorded nowhere.
   - It ran `engr work --help` at T45–46 and then wrote nothing.

3. **Findings were compressed to one line, so work was redone and a known hazard came back.**
   - S1's reading (T15–T70) and its subagent's report became one item result: "confirmed: gate::prepare/confirm_locked reusable; …".
   - S2 re-ran the same lookups for 37 turns (T1–T37; see section C for the one-for-one pairs).
   - S2 knew `with_lock` is a non-reentrant flock (comment at T48) but did not record it, and itself caused the nested-lock deadlock that cost S3 T8–T37.

---

## Label V — arm G

### Audit: engr as working memory across three cut-off sessions (run V)

Sources are the three session logs, the three state dumps, the three probe reports and the three metrics files. `S1 T37` means turn 37 of `S1.log`. Where a log result was cut short and I could not see what it said, I say so.

---

#### A. At each cut-off

##### S1 (cut at T70 of 70)

**What the agent was doing when it was cut.** It had just finished the library code and was starting on the CLI (sidecar item 4). At T68–T70 it was grepping `enum Command` and `CollectionCommand` in `main.rs`. `changeset.rs` had been written at T62 and passed `cargo check` after fixes (T64–T67). The `_locked`/`dry_run` entry points in `backlog.rs` and `collection.rs` were finished (T52–T56). The harness task list said the same thing: at T64 the agent marked task 2 (layout) and task 4 (locked entry points) completed and task 3 in progress.

| Decided, found, ruled out or open in S1 | In engr at the cut? |
|---|---|
| Scope: Backlog and Collection only, no Object/Human Gate (T15–T25) | **Yes.** Object §1, admitted on the third review attempt |
| Human Gate composition stays open | **Yes.** Backlog 01a0d45e §1 |
| `store::with_lock` is not re-entrant, so domains need unlocked or `dry_run` variants (T3 research, T6 task #4) | **Partly.** Item 2's text says "for one shared lock". The reason exists only in the harness task, which is lost at the cut, and in comments in uncommitted `backlog.rs` |
| Design: `.engr/local/changeset/<id>/` holding manifest, candidate, progress and committed files; state derived from which files exist; two passes (validate everything, then write and journal) under one lock | **No.** It is only in the module doc of the untracked `changeset.rs` |
| Only two intent kinds, `backlog produced` and `collection add` | **No.** Code only |
| Work-domain intents dropped (the T6 plan listed "Backlog + Collection + Work variants") | **No.** Recorded nowhere |
| `revise_intent` dropped (the T6 plan and item 3's text both include it) | **No.** Recorded nowhere |
| Crash-safety claimed via the progress journal | **No.** It is in the module doc, and it was wrong: S3 T42 found that the doc "overstated the crash guarantee" |
| Item 2 finished | **Wrong.** Sidecar says `active` |
| Item 3 written and compiling | **Wrong.** Sidecar says `pending` |
| Summary | **Stale.** It reads "deciding domain scope before writing code", though code had been under way since T52 |

The last engr write was at T27, which left 43 turns unrecorded (metrics). The heartbeat hooks at 10 and 20 tool calls (S1 T3, T64) had no effect. The TaskUpdates at T64 were not mirrored into the sidecar.

##### S2 (cut at T70 of 70)

**What the agent was doing when it was cut.** It was preparing to write `tests/changeset.rs` (item 5), reading `tests/common/mod.rs`, `backlog::create` and `Precondition` (T64–T70). Before that it had finished the CLI and view (T33–T57, "Builds clean, no warnings") and run smoke tests (T58–T63). Those tests covered end-to-end two-domain apply, idempotent re-apply, restart persistence, refusal on staleness with no partial write, discard, and resume after a crash injected by hand-writing `progress/1.json`.

| Decided, found or open in S2 | In engr at the cut? |
|---|---|
| Items 2 and 3 done (repairing S1's sidecar, T36) | **Yes**, with results |
| ApplyInputs does not thread a backlog review Attestation (T37–T39) | **Yes.** Backlog §2 |
| Item 4 (CLI and view) finished and smoke-tested | **Wrong.** Sidecar says `active` |
| Smoke-test results, including the crash-resume and staleness checks | **No.** Recorded nowhere, so S3 re-ran them |
| CLI shape: per-intent `apply --expect N=TOKEN`; `--target` must be the full `engr:obj:<26-char>` reference | **No.** Code only |
| Item 3 ticked done although `revise` is not implemented | **Wrong.** The tick is false against the item's own text |
| Summary | **Stale** (unchanged from S1) |
| Commits | None in the repo. Code and `.engr` were both dirty at the cut (metrics `dirty_*`). The five `git_commit_turns` in the metrics were commits in throwaway temp repos |

The last engr write was at T39, leaving 31 turns unrecorded.

##### S3 (cut at T90 of 90)

**What the agent was doing when it was cut.** It had committed `.engr` (60bcba6, T89) and run `engr collection ls` (T90), presumably to close out. That is inferred: the log ends there. Everything had been done:
- formatting fixed (T7);
- the CollectionAddMember idempotence bug found and fixed (T40–T41);
- the module doc corrected (T42);
- 9 tests added and passing (T59–T62), with the full suite and fmt green (T63–T65);
- the code committed as 8404e2f (T75);
- items 4–7 closed, with commits attached;
- §2 and §3 admitted (T77–T85).

| S3 item | In engr at the cut? |
|---|---|
| The idempotence finding and its fix | **Yes.** Item 7, record §2 and §3, but only after a blocking gate hook at T50 held the test-file write |
| Mismatched membership is refused, not overwritten | **Yes.** §3 |
| The agent's judgment that §1 still holds after its basis moved (T87: "its content ... still holds; no revision needed") | **No.** Recorded nowhere, so probe-3 would redo the check |
| On resume, a backlog intent needs a fresh `--expect` token, because the landed write moved it (T61) | **No.** It is only in a test comment |
| The slice is finished and what comes next | **No.** Sidecar state is `active` with 0 open items, the summary still reads "deciding domain scope before writing code", and the collection is still `open` |
| Item 3's "revise" | **Still wrong** |

---

#### B. The probes

| Q | Probe-1 | Probe-2 | Probe-3 |
|---|---|---|---|
| 1 Settled | Correct | Correct | Correct. The "633 tests" figure could not be checked (the sidecar says "569+") |
| 2 Open | Correct | Correct | Correct as to engr. Its claim that the #42 Phase 4 "Initial goal" includes Human Gate integration cannot be checked from the logs |
| 3 In progress | Correct: item 2 done, item 3 largely done, item 4 not started, fmt failing | Correct: item 4 functionally done, item 5 not started. It could not recover S2's smoke tests and ran its own | Partly correct: "finished" is right, but it did not see that the sidecar, summary and collection were never closed |
| 4 Next | Correct: repair the sidecar, then the CLI. S2 did exactly this | Correct: fmt, then tests. S3 did exactly this | Partly correct: re-check §1's moved basis. That is what engr shows, but S3 had already done it (T87) and not recorded it |
| 5 Ask | Correct and sharp: it noticed `revise`/`reorder` missing from `changeset.rs` against #41 | Reasonable. It asked about an apply-path edge case, and S3 then found one | Reasonable: whether a human agreed to relax the Phase 4 scope |
| 6 Stale | Correct on the sidecar lag. It missed the summary | Partly wrong. "All sidecar claims for steps 1–3 check out" overlooks item 3's missing revise. It flagged item 4 and missed the summary | Partly correct. It caught §1 "basis moved" (engr surfaces this itself). It missed the summary, the `active` state and item 3 |

- **Hallucinations.** I found no confident claim that contradicts the logs.
- **What the probes could not recover.** Probe-2 could not recover S2's smoke-test evidence, and probe-3 could not recover S3's "§1 still holds" judgment. Neither was ever in engr.
- **What no probe flagged.** None flagged the summary line, which was wrong at every cut. Only probe-1 caught the unticked-but-claimed `revise`; probes 2 and 3 accepted the tick.

---

#### C. The successor sessions

**S2**
- **Re-orientation.** About 8 turns (T1–T8) went to it. That included re-reading #41, #42 and SKILL.md in full (T2), reading all of `changeset.rs` and every diff (T4–T6), and rebuilding (T7).
- **Productive work.** The first edit was at T33. T10–T32 were study of the CLI patterns needed for item 4, which counts as productive.
- **Redo, re-decision or contradiction.** None. It correctly judged at T8 that item 3 was "substantially further along than the sidecar claims".
- **Repair.** Items 2 and 3 were ticked at T36, 28 turns after it noticed and right after the 20-call hook (T34). The repair was imperfect: item 3 was ticked with its "revise" text standing, and the summary was left alone.

**S3**
- **Re-orientation.** T1–T10. The fmt fix at T7 was real work.
- **Redo.** T11–T18 re-ran S2's smoke tests: new changeset, two intents, prepare, restart, apply with and without `--expect`. At T15 it also hit the same short-id `--target` error ("compact UUID must be exactly 26 characters") that S2 got past between T60 and T61. S2's T60 result is truncated, but T61's rewrite to fetch the full reference implies the same failure.
- **New work.** New engineering started at T19, and the first new finding came at T40.
- **Re-decision or contradiction.** Nothing was re-decided or contradicted. It did overturn an S1 hypothesis, the claimed crash safety, and it recorded that.
- **Repair.** It ticked item 4 with evidence (T24), attached commits to items 3–7 (T76) and committed `.engr`. It did not fix the summary, the `revise` tick or the sidecar's state.

---

#### D. The state against the criteria

##### Backlog points
There are two, both under one topic.

**Best: §1.** The first sentence is a real question, and the third names who settles it and when: "Settled once the non-Object slice ... has been dogfooded per issue #42 Phase 4 and a maintainer decides the SubjectType::ChangeSet / Contract shape." Against that:
- it is two questions ("Whether ... and if so how ...");
- it runs to 104 words;
- its middle sentence argues the stakes.

**Worst: §2.** It is filed under the topic **"ChangeSet x Human Gate"**, which it has nothing to do with. The middle sentence is code narration: "Collection's Rule review is attempt-only (rules::direct), so CollectionAddMember already composes; BacklogProduced goes through Prepared, whose review attestation ApplyInputs does not thread through today...". The settle clause names a trigger, not an answer: "Settled by whether a domain: backlog Rule is ever written". It is really a known limitation phrased as a question.

**Missing points:**
- the dropped `revise`/`reorder` intents;
- the dropped Work intents;
- the need for a fresh token when resuming;
- whether the #42 Phase 4 scope was relaxed (probe-3's question).

##### Work items
- **Granularity.** Six sensible steps were planned at S1 T27. Item 3 ("local stage, create/add/revise/remove intent, prepare, apply") is several steps in one. Item 7 is a log entry added already done ("Found+fixed: ...").
- **Completion conditions.** Only item 1 states one ("done when admitted"; `items_with_done_condition: 1`).
- **Ticking.** The ticking lagged real progress at both S1 and S2. At the S1 cut, 2 items were stale. At the S2 cut, 1 was stale and its evidence was lost.
- **Evidence.** Every done item has a result (`done_without_result: 0`), and items 3–7 carry 8404e2f2. The quality varies:
  - Item 3's result is "builds clean", which is weak evidence for "implemented".
  - Item 3's result lists "create/add/remove_intent" while its text still promises revise. That is a false tick.
  - Item 5's result is good: "9 tests, all pass -- restart persistence, prepare-freeze/apply-revalidate+stale, atomic 2-domain, 2 crash-injection cases...".
- **Summary.** It was written once (T13) and never updated. The sidecar was never closed.

##### Reasoning in progress
- **Nothing narrative was dumped into engr.** That is good.
- **The one ruled-out hypothesis that reached engr** was S1's claim of crash safety. S3 overturned it, and it reached engr only because the gate hook blocked a write (S3 T50).
- **Everything else stayed outside engr.** It sits in code comments, a commit message, a test comment or nowhere:
  - the lock re-entrancy finding;
  - the two-pass, roll-forward design;
  - the Work and revise drops;
  - S2's smoke-test results;
  - the fresh-token finding;
  - the "§1 still holds" judgment.
- **Review independence.** In S1 the review prompts told the reviewer the factual claim was "accurate (verified independently, not something you need to re-derive)", and that this was "the third and final autonomous attempt" (T19, T21, T24). That goes beyond "only the Rule, its bases and the exact wording".

##### Record Sections
- **§1 is good.** One assertion with its reason, 45 words.
- **§2 and §3 are each one assertion with a reason,** but they describe one function's behaviour and repeat the code comment written at S3 T41.
- **The load-bearing design is absent from the record:**
  - validate-then-write under one lock;
  - roll-forward journal instead of rollback;
  - state derived from which files exist;
  - `local/` is not git-tracked;
  - exactly two intent kinds;
  - per-intent `--expect`.

  All of this lives only in `changeset.rs` docs and in 8404e2f's message. What the record holds is true, but it is the periphery of the design.

---

#### E. Cost

| Session | engr turns | Share | Notes |
|---|---|---|---|
| S1 | T7–T29 ≈ 23 of 70 | ≈33% | 6 of its 7 subagents were engr reviews or cold reads. T9–T25 (17 turns, all 3 allowed attempts) went to one title and one 45-word sentence, before any code |
| S2 | ≈ 7 of 70 | ≈10% | |
| S3 | ≈ 25 of 90 | ≈28% | 2 review subagents. T68–T86 went to two sections. A further 8 turns (T11–T18) redid S2's unrecorded work |

- **Total.** About 55 of 230 turns (≈24%) went to engr, plus about 16 turns of re-orientation and redo caused by stale state.
- **Dollar cost.** S1 was the most expensive session ($6.16, against $2.42 for S2 and $3.72 for S3), which fits its subagent-heavy record work.
- **Verdict on proportion.** The total did not crowd out the engineering, since the slice shipped. The allocation was upside down, though. Effort went to the gated record, which added little for resuming, and not to the sidecar, which is cheap and would have prevented every redo. S1 wrote nothing to engr in its last 43 turns, and S2 nothing in its last 31.

---

#### F. Verdict

| Criterion | Score | Justification |
|---|---|---|
| 1. Backlog points | **3/5** | Each point has a question and a settle clause, but they run 91–104 words with a middle sentence of argument, §1 is two questions, §2 is filed under the wrong topic and settles on a trigger rather than an answer, and four real open questions were never staged. |
| 2. Work items | **2/5** | The breakdown is sensible and every done item has a result, but the sidecar lagged reality at both S1 and S2, item 3 was ticked done while promising `revise` that does not exist, only 1 of 7 items has a completion condition, and the summary and state were never maintained. |
| 3. Reasoning in progress | **2/5** | Nothing narrative was dumped, but apart from the S3 bug (recorded only under a blocking hook), every finding, dropped path and verified hypothesis lived in code comments, a commit message or nowhere. |
| 4. Resume | **3/5** | No successor re-decided or contradicted anything, but each resumed correctly only by checking the repository against a wrong sidecar: S2 spent about 8 turns re-verifying, and S3 re-ran S2's smoke tests and hit its dead end again. |

##### The three most important failure modes, most damaging first

1. **The sidecar was not written while work happened. It was written only at the edges, or when forced.**
   - S1's last engr write was at T27, with 43 turns after it. Items 2 and 3 were shown `active`/`pending` while the agent's own task list had them completed and in progress (T64).
   - S2's last write was at T39, with 31 turns after it. Item 4 was left `active` after the build at T57 and the smoke tests at T58–T63, and the evidence was never recorded.
   - Heartbeat hooks at 10 and 20 calls were ignored in all three sessions. Only the blocking gate at S3 T50 produced a mid-work update.
   - Cost: S2 T1–T8 re-verification, and S3 T11–T18 redoing S2's smoke tests, including the same `--target` error at S3 T15.
2. **A false completion was ticked, and the scope was silently narrowed.** Item 3's text is "Implement changeset.rs: local stage, create/add/revise/remove intent, prepare, apply". It was ticked done at S2 T36 with a result listing only "create/add/remove_intent". No `revise` exists, and there is no decision or backlog point saying so. The Work-domain intents in S1's own plan (T6) vanished the same way. Probe-1 caught the gap; probe-2 then declared "All sidecar claims for steps 1–3 check out", and probe-3 inherited the tick.
3. **engr never says where things stand or what comes next.**
   - The summary, "Scoping first ChangeSet slice ...: deciding domain scope before writing code", was unchanged at all three cuts, including S3's, where all 7 items were done.
   - The sidecar stayed `active`, and the collection stayed `open`.
   - The S3 agent's T87 judgment that §1 still holds after its basis moved was not recorded, so probe-3's recommended first action is to repeat it.
   - The load-bearing design (validate-then-write, roll-forward journal, per-intent tokens) is also only in code. The record holds the scope rule and one edge case, not the design a successor would build on.

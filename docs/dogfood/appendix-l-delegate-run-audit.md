# Audit Z: how well the agent kept its working state in engr

Sources: S1/S2/S3 logs (turns cited as `S2 T92`), state-S*.txt, probe-*.md, metrics-S*.json.

## A. At each cut-off

### S1 (cut at T70, max turns)
**Doing / intending:** At T69 it committed the backlog, then rewrote the sidecar summary to "Design (21 Sections) admitted; item 2 (json-patch dep + session.rs) next to delegate". That summary edit came after the commit, so it was never committed. At T70 it was loading the `engr-collection` skill to create the plan, and delegating item 2 was the next step. S1 wrote no code.

**Decided / found / ruled out / left open:**
- It settled 22 design choices (see F). json-patch was chosen at T14. At T36 it switched the write order to history before snapshot, and it added the dual View→Session lock at T37.
- It ruled out: a new top-level `.engr/session/` directory, reusing `view.rs`, the workspace lock, a hand-rolled patch applier, a Rule domain, and a liveness check.
- It found that `with_lock_at` is private, that `view.rs` is already taken, and that `.engr/local` is the only directory git does not track (subagent survey, T3).
- It left three questions open, which it staged in backlog at T68.

**In engr:** all 21 Sections (§1–§21) and all 3 backlog points. The sidecar shows item 1 done with a result and commit.

**Missing:**
- The collection. It never existed at any cut.
- Any done condition on the 6 items (`items_with_done_condition: 0`).
- The crash-ordering reason for writing history before the snapshot. The T36 draft said "a crash between the two leaves a history entry the snapshot has not caught up to rather than a snapshot no history explains". The T48 rewrite of "Atomic ui.update" dropped it, and §15 keeps only "written before that file is republished".
- The T3 codebase survey. S2 had to redo it (see C).

**Wrong / stale:** nothing is wrong. The sidecar was modified but uncommitted (`git status`: `M .engr/work/...`).

### S2 (cut while a background worker ran)
**Doing / intending:** At T143 the agent delegated item 5 (inspection CLI) to a background worker. It confirmed the tree was clean at T145 and ended its turn at T146 with "Waiting on the item 5 worker". The worker was cut mid-run. It had edited `main.rs`, `view.rs` and `tests/cli.rs`, and both new CLI tests passed. But `cargo fmt --check` failed on cli.rs and its full `cargo test` died with exit 137. It had added no ChangeSet steps. The agent's plan, stated in the sidecar summary, was: verify the worker, close item 5, then item 6, and re-prepare the resume() decision once LQBPYK resolved.

**Decided in S2:**
- 20 implementation Sections (§22–§41).
- Two decisions that exhausted review: "resume() re-attaches an owner unconditionally" and "history file created lazily".
- Moving the json-patch dependency to item 3 (T10).
- `interaction.rs` as its own module (item-4 worker).

**Found:**
- engr allows one live Challenge per object. The agent learned this at T95–T97, after its T92 `prepare` had already voided CONFIRM 6SSDH5.
- The "basis moved" flags on §22–§29 were harmless (T104–T107).

**In engr at cut:**
- 41 Sections.
- Sidecar items 1–4 done with result and commit; item 5 active.
- An empty ChangeSet 01a0eed6.
- Candidate LQBPYK.
- The same 3 backlog points.

**Missing:**
- The resume() decision's wording and reason. Only its name survives in engr: summary "resume() decision needs re-prepare after that", and the item 2 result. The text exists only in the voided candidate and in commit message 6797a2f.
- The worker's in-flight state (fmt failing, tests unverified) and its CLI decisions. The worker was cut before it reported, so the main agent could not have recorded them.

**Wrong / stale:**
- The summary and the item 3 result say "pending Human candidate CONFIRM LQBPYK". `engr candidate` shows it **stale**: admitting §40–§41 at T131 moved the object to rev 42.
- §1 ("session.rs … and uiview.rs for the View, its patches and its interaction events") is contradicted by §40 ("An Interaction Event's store is crates/engr/src/interaction.rs rather than new functions on uiview::View"), admitted at T131. §1 was never revised.
- §30–§37 were flagged "basis moved" after item 4 changed uiview.rs, and were never rechecked.

### S3 (ran to T52 and stopped)
**Doing / intending:**
- Fixed fmt (T7), smoke-tested the CLI (T11) and committed the worker's diff (T23).
- Drafted §42/§43 from the worker's code (T20) and admitted them (T36).
- Re-prepared the dead lazy-history candidate as RDEMBM (T44).
- Closed items 5 and 6, then stopped, reporting CONFIRM RDEMBM to a human.

**In engr at end:** 43 Sections, RDEMBM pending, all 6 items done. Summary: "Only open thread: pending Human candidate CONFIRM RDEMBM".

**Missing:** the resume() thread. S3 read "resume() decision needs re-prepare after that" at T9 and again in the T21 output. It then overwrote the summary at T21 without that clause and never acted on it. Item 2's result still names it: "1 (resume unconditional-reattach) exhausted, no live candidate right now".

**Wrong / stale:**
- §1 against both the repository and §40.
- §22–§41 flagged "basis moved", not re-verified.
- Items 5 and 6 carry commit `adca9eac` ("engr: delegate item 5"), which is not the work. At T21 it pointed them at HEAD before committing; T24 appended the right commit (85c438a9) but left the wrong one in place.
- Item 6 was a duplicate of items 2–5, closed as done.

## B. The probes

| Q | Probe 1 (after S1) | Probe 2 (after S2) | Probe 3 (after S3) |
|---|---|---|---|
| 1 Settled | Correct | Correct | Partly correct: "Code matches" is wrong for §1, which describes two modules; the repo has three (§40). Stated confidently. |
| 2 Open | Correct | Partly: omits the resume() decision that the sidecar summary names; calls LQBPYK "needs a human decision or a re-split" while noting in Q6 that it is dead | Partly: omits resume(), which item 2's result still names |
| 3 In progress | Correct: item 2 not started (checked the repo), sidecar uncommitted | Correct, and verified: tests pass, fmt fails on cli.rs, nothing committed | Correct |
| 4 Next | Correct (commit the sidecar, start item 2), which is what S2 did at T5/T17 | Partly: fmt/commit/close item 5 is what S3 did first; misses recording the worker's decisions (empty ChangeSet) and the dead candidates | Correct per protocol (show RDEMBM, wait), but incomplete: no resume(), no re-verification |
| 5 Ask | Correct, reasonable | Correct; spotted that item 6 is redundant with item 5 | Partly: only RDEMBM |
| 6 Stale | Correct (only the uncommitted sidecar) | Partly: explained "basis moved" correctly (conflating §34 with §39) and saw LQBPYK dead, but did not say the sidecar summary was wrong, and missed §1 vs §40 | Partly: sampled 6 of 20 flagged Sections, found "nothing contradicts", and missed §1 vs §40 |

**What the probes could not recover:**
- The resume() decision's content. engr holds only its name, and probes 2 and 3 did not even surface the name.
- The §1/§40 contradiction, which is only visible by reading §1 against §40 or the file tree.
- That the S2 worker had been killed rather than having finished.
- S1's intent to build a collection.

Probe 1 was fully right because S1 left the cleanest state. Probes 2 and 3 noticed tool-flagged staleness (dead candidate, basis moved) but nothing that required cross-reading engr's own text.

## C. The successor sessions

**S2:**
- First write was at T5: committing S1's leftover sidecar diff, a repair.
- First engineering progress was the item-2 delegation at T17.
- T6–T16 re-read store.rs, work.rs and lib.rs to build the worker packet. S1's T3 survey had produced the same facts but they were never recorded.
- Redo: `cargo search json-patch` again at T58 (S1 did it at T13–T14).

**S2 contradiction:** the item-4 worker's §40 (separate `interaction.rs`) overturns S1's §1. The reviewers at T116/T117 were shown only §9, §13, §18 and §32, so nobody checked against §1.

**S2 new damage:** at T92 it prepared LQBPYK and so voided the pending 6SSDH5. AGENTS.md warns about exactly this: "re-running `prepare` mints a new code and voids the one they are holding".

**S2 repaired:**
- The uncommitted sidecar (T5).
- The "basis moved" check on §22–§29 (T104–T107).

**S3:**
- Productive at T7 (fmt fix) after 6 orientation turns. No redone work beyond re-running tests.
- An unused smoke_test.rs was written and deleted at T12–T13.
- It re-decided nothing.

**S3 repaired:**
- The worker's uncommitted, unformatted diff (T7, T23).
- The worker's unrecorded decisions (§42/§43).
- The dead LQBPYK (as RDEMBM).

**S3 did not repair:** resume(), which it dropped outright; §1; "basis moved" on §22–§41; the wrong commit on items 5 and 6.

## D. The state against the four criteria

**Backlog:**
- Three points, 72–95 words and 3–4 sentences each. Each has a "Settled by" clause.
- Each is one question, padded with narration of what slice 1 did.
- Worst, 1a8d: "Settled by whether a real project actually wants to write such a Rule once engr mcp exists; until one is proposed, there is nothing for the growth rule to act on". Nothing actionable settles it.
- Best, 1a9a: "How a Session's owner attachment should be cleared when the owning process dies without calling detach … Settled by the engr mcp design deciding what liveness signal is available (stdio EOF, a heartbeat call, a PID check)".
- The backlog never changed after S1 T68. Unresolved material from S2 and S3, the exhausted resume() decision above all, never went there.

**Work items:**
- Six items, one per module. None has a done condition in engr. The done conditions existed only in the worker prompts ("Done when: `cargo build --workspace`…", S2 T17/T61/T109/T143).
- Item 1 ("Draft+review+apply design ChangeSet (storage, ids, locking, lifecycle, ui.update, events)") took 45 turns.
- Item 6 ("Tests: …; cargo test + fmt pass") duplicates items 2–5.
- Ticking for items 1–4 was good: the main agent re-ran build, test and fmt and read the diff before closing (S2 T21–T23, T68–T70, T113–T114). Each item got a result and a commit.
- Results embedded candidate codes that went stale (6SSDH5, then LQBPYK).
- Items 5 and 6 carry a wrong commit.
- No collection was ever created.

**Reasoning:**
- Rejected alternatives do sit in Sections as "rather than X" clauses, which is good. No messy path was dumped into engr.
- But the reasoning behind the two exhausted decisions lived only in git commit messages and voided or pending candidates.
- The history-before-snapshot crash reason was cut from the record.
- The S1 survey findings were not kept, so every worker re-derived them.

**Sections:**
- 43 Sections, 34–118 words. Almost all make one assertion with a reason, as the reviews enforced.
- §1 is false.
- §15 states the write order without its reason.
- §22 still narrates with "(now pub(crate))".
- §4 and §11 carry scope narration ("this slice has no running MCP or Web process…").
- About a third are code-level trivia that do not need record authority: §29 (Option<&str>), §34/§39 (pub(crate) visibility), §27 (reuse write_json), and §25/§32/§36/§41 (exit-code and check placement).
- Possible code/record mismatch, **not verified from the logs**: the item-4 commit says the Session lock is "taken inside allocate_sequence". §18 says both locks are held "for the whole of the operation".

## E. Cost

| Session | Turns | Share on engr (writing, reviewing, fixing refusals, handling hooks) | Subagents | Cost |
|---|---|---|---|---|
| S1 | 70 | ≈79% (T16–T70); T2–T15 were issue/code research; **zero code** | 6: 1 survey, 5 review/cold-read | $5.47 |
| S2 | 146 | ≈70%: item 2 decisions T24–T56, item 3 T71–T103, item 4 T115–T134, plus delegation and stop-hook turns | 16: 4 workers, 3 cold reads, 9 Rule reviews | $15.64 |
| S3 | 52 | ≈60% (T17–T21, T24–T49) | 5, all readers/reviewers; one wasted on a literal `$(cat)` prompt (T27) | $2.30 |

About 70% of the main agent's turns and 22 of 27 subagents went to keeping the record. The code itself was written by four workers. That is not proportionate:
- The first session ended at max turns having produced only records.
- S2 ran 8 failing review rounds, mostly on rule 5 ("restates §N") over wording like Option<&str> and visibility modifiers.
- The time went to review and none to the backlog, the plan or done conditions.

## F. Decisions, one by one

| # | Decision | Made | First reached engr | Same, with reason? |
|---|---|---|---|---|
| 1 | json-patch crate v4, not hand-rolled | S1 T14 | S1 T46 §12 | yes |
| 2 | State under `.engr/local`, not a new dir | S1 T24 | S1 T46 §13 | yes |
| 3 | Two modules: session.rs + uiview.rs (uiview holds interaction events), not view.rs | S1 T24 | S1 T46 §1 | yes until S2 T131; now contradicted by #34 and false against the repo |
| 4 | s_/v_/e_ + UUIDv7 ids, never engr: | S1 T24 | T46 §2 | yes |
| 5 | Per-resource locks, not the workspace lock | S1 T24 | T46 §3 | yes |
| 6 | Single explicit owner, no liveness check | S1 T24 | T46 §4; backlog 1a9a T68 | yes |
| 7 | Close is terminal | S1 T24 | T46 §5 | yes |
| 8 | continued_from is provenance only | S1 T24 | T46 §6 | yes |
| 9 | Detach leaves Views writable | S1 T24 | T46 §7 | yes |
| 10 | revision starts at 0, +1 per applied update | S1 T24 | T46 §8 | yes |
| 11 | Session-scoped sequence | S1 T24 | T46 §9 | yes |
| 12 | Stale interaction refused before allocation | S1 T24 | T46 §10 | yes |
| 13 | Inspection-only CLI | S1 T24 | T46 §11 | yes |
| 14 | No Rule domain | S1 T24 | T46 §14; backlog 1a8d | yes |
| 15 | Separate history file | S1 T24 | T46 §15 | yes |
| 16 | History written before the snapshot (reversed from the draft) | S1 T36 | T46 §15 | assertion yes; **crash-ordering reason dropped** |
| 17 | Format tag engr-ui/v0, fail on unknown | S1 T24 | T52 §16 | yes |
| 18 | No retention/compaction | S1 T24 | T52 §17; backlog 1a94 | yes |
| 19 | Dual lock, View then Session | S1 T37 | T52 §18 | yes |
| 20 | `{root, elements}` document | S1 T24 | T63 §19 | yes (reason replaced at T55) |
| 21 | Atomic ui.update order | S1 T24 | T63 §20 | yes |
| 22 | Validation checks shape only, no catalog | S1 T24 | T63 §21 | yes |
| 23 | Plan: 6 module-sized items | S1 T25 | sidecar S1 T28 | no done conditions |
| 24 | json-patch dependency moves to item 3 | S2 T10 | sidecar item text S2 T10 | no reason in engr |
| 25–32 | session.rs: reuse with_lock_at; lock file beside state; version-checked id; require_current once; token is not a credential; reuse write_json; close refuses re-close; Option<&str> | S2 T17 (worker) | §22–27 S2 T28; §28–29 S2 T39 | yes |
| 33 | **resume() re-attaches over an existing owner** | S2 T17 (worker) | **never**: candidate 6SSDH5 S2 T51, voided T92, dropped S3 T21 | no; name only |
| 34 | interaction.rs as its own module | S2 T109 (worker) | S2 T131 §40 | yes, but contradicts §1 |
| 35–41 | uiview.rs: bare "4" pin; untyped Value; EXIT_INVARIANT for stale revision; canonical JCS history; validate_id pub(crate); closed check first; EXIT_SCHEMA for failed patch | S2 T61 (worker) | §30–33 T76; §34–36 T82 | yes |
| 42 | History file `<id>.history.jsonl` | S2 T61 (worker) | T87 §37 | yes |
| 43 | **History created lazily; absence reads as empty** | S2 T61 (worker) | **never admitted**: LQBPYK S2 T92 (stale from T131), RDEMBM S3 T44, pending | the candidate's wording only |
| 44–46 | interaction: log colocated in views/; uiview::validate_id pub(crate); refusals use EXIT_INVARIANT | S2 T109 (worker) | §38 T119; §39 T125; §41 T131 | yes |
| 47–48 | CLI: exact id match; `view events` checks the View exists | S2 T143 (worker, killed) | S3 T36 §42–43 | yes, one session late |
| 49 | CLI banner wording ("SESSION — … admitted by nobody, governed by no Rule…") | S2 T143 (worker) | never | minor |

**Tally:** 41 decisions became Sections in the session that made them (21 in S1, 20 in S2), 2 only later (§42/§43 in S3), and 2 substantive ones never (#33, #43). One reason (#16) and one minor choice (#49) were also lost.

**Recording cost:**
- 22 cold-read and review subagents in total.
- Review rounds that failed at least one step: S1 2, S2 8, S3 1.
- Refused commands:
  - S1: T25 (wrong arguments), T39 (oversize).
  - S2: T45 (exhausted inside a ChangeSet), T48 (ceiling), T49 (Agent mutation needs a passing review), T50 (digest mismatch), T53 (result over 240 characters), T91.
  - S3: T42, T43.
- About 190 main-agent turns.
- The two decisions that exhausted review were both implementation details, and both are exactly the ones that were lost.

## G. Verdict

**Scores:**
1. **Backlog points: 3/5.** Each point is one question with a settle clause, but at 72–95 words it is narrated, one settle clause is unactionable, and the backlog was frozen after S1 T68 while unresolved material piled up elsewhere.
2. **Work items: 2/5.** Six module-sized items with no done conditions in engr (0/6), a duplicate item 6, and no collection. Items 1–4 were ticked with verified evidence, but S2's summary claimed a live candidate that was stale, and items 5 and 6 point at the wrong commit.
3. **Reasoning in progress: 3/5.** Rejected alternatives live in Sections and nothing messy was dumped, but the reasoning behind the exhausted decisions and the crash-ordering reason survived only in git messages and voided candidates.
4. **Resume: 3/5.** Successors resumed fast (S3 productive by T7) and probes 1–3 got the state mostly right. But no probe and no successor recovered the lost resume() decision or the §1/§40 contradiction, and probe 3 asserted "Code matches".

**Three failure modes, most damaging first:**

1. **A settled decision was destroyed, then forgotten.**
   - The resume() reattach decision exhausted review and became CONFIRM 6SSDH5 (S2 T51).
   - At S2 T92 the agent's own `prepare` voided it; it learned this only at T96–T97.
   - The decision was then parked as the prose "resume() decision needs re-prepare after that", not in the backlog and not in the record.
   - S3 read that line (T9) and deleted it from the summary (T21). Its final report says "Only open thread: … RDEMBM".
   - Probes 2 and 3 never mention it. Its wording now exists only in commit message 6797a2f.

2. **The record contradicts itself and the code, and nothing caught it.**
   - §1 still says the store is two modules, with interaction events in uiview.rs. §40, admitted at S2 T131, puts them in interaction.rs.
   - The S2 T116/T117 cold reader and reviewer were given hand-picked excerpts (§9, §13, §18, §32) instead of the Object, so the rule-5/contradiction check could not see §1.
   - Earlier Sections are never revised; 20 Sections end flagged "basis moved" and unexamined in S3.
   - Probe 3 certified "Code matches".

3. **The review machinery went to trivia and crowded out everything else.**
   - About 70% of main-agent turns and 22 of 27 subagents went to recording (E). S1 ended at max turns with zero code.
   - S2 spent 8 failing review rounds on rule-5 wording about Option<&str>, pub(crate) and exit codes. Those exhausted reviews produced the very candidates that got lost (failure 1).
   - Meanwhile the parts a successor actually needs were neglected: done conditions (0/6), an updated backlog (unchanged after S1 T68), and a plan collection (never created).

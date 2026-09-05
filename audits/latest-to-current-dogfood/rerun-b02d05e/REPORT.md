# latest → current dogfood re-run, at `b02d05e`

**Destination under test:** `b02d05e` — the head that answers review `5122007108`,
three commits past the last audited one.
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Previous runs:** `a77887b` (committed audit), `52fe116`
([`rerun-52fe116/`](../rerun-52fe116/REPORT.md)), `ca6474a`
([`rerun-ca6474a/`](../rerun-ca6474a/REPORT.md)).

The rule is the one the audit has always had: a scenario is `PASS` only when the
expected behaviour was **observed in a transcript in this run**. Nothing is
`PASS` because the implementation was read and judged likely correct.

This run also does something the previous three could not: it builds the
**previously audited head** as a second binary and runs the same forged state
through both. Where this report says a behaviour is new, or that a gap is
pre-existing, that is a measurement rather than a reading of the diff.

```text
engr-latest    engr latest (unknown)     e7d9f99
engr-current   engr latest (b02d05ee)    rebuilt for this run
engr-prev      engr latest (unknown)     ca6474a, built to compare against
```

## The input is the same input, for the fourth time

The predecessor workspace was restored from the pre-migration checkpoint and its
inventory is **byte-for-byte identical** to `evidence/inventory-pre-migration.txt`
on the `#68` branch. Every difference below therefore belongs to the code.

## What three commits changed in the record: nothing

```text
migrated object   a77887b       52fe116       ca6474a       b02d05e
01a05e55-74       8bc54cbed5    8bc54cbed5    8bc54cbed5    8bc54cbed5
01a05e55-e6       aef85ebe6e    aef85ebe6e    15a1e01f15    15a1e01f15
01a05e55-ee       b57120eb38    b57120eb38    b57120eb38    b57120eb38
```

The whole post-migration inventory differs from the `ca6474a` run in exactly
three lines — the Event streams, which carry a fresh Event id and admission
instant per run by construction. `.gitignore`, `VERSION` and all three Objects
are identical bytes. These three commits changed what the tool *says* about a
record, not what it writes.

Every seal was recomputed by an independent implementation before anything was
built on it: RFC 8785 JCS + SHA-256 from scratch for the Section and Object
seals, `EventDigestContract 1` for all three streams, and the migrated
`RefDigest` assembled by hand from #66 §6.5 — preimage written out from the
contract text, historical values taken from `git show` of the pinned commit, a
selected absent optional collection carried as `null`. All `MATCH`
(`evidence/digest-premise.txt`).

## The three commits' own findings, each observed — and the hole one of them closed

| | Observed at `b02d05e` | And at `ca6474a` |
|---|---|---|
| **round 27 [P1]** a dependency is a claim about authority, so the target's history is part of it | a correctly resealed field forged onto the Ref target: `verify` FAILs on **both** objects, the dependent naming `§2 stands on 01a05e55-74 §1, which seals correctly and is not what its own history produced` | the target FAILs and **the dependent PASSes** — the hole, reproduced |
| **round 28 [P2]** a new answer is an obligation at every consumer | `show` → `REF UNADMITTED`, exit 5, advice naming `repair`; `show --format json` → `"status": "ref_unadmitted"` with `target_history_divergent: true`; `ls --stale` lists it | `ls --stale` says `all ok` |
| **round 28 [P2]** the four Work lists in `PROTOCOL.md` | the compiled `engr protocol` now says *omitted when empty*; the write path omits all four; the reader refuses all four explicit `[]` spellings and accepts the omitted ones (`evidence/work-empty-lists.txt`) | — |
| **round 29 [P2]** asking the last question first | a Section removed by hand from an intact Object reads as **divergence**, not as a supported deletion; the two controls hold — unsealed removal reports `current Object integrity failed` *first*, an admitted deletion Event reports `REF MISSING` and the target itself PASSes at rev 2 | — |

The ordering the evaluator promises — integrity, then history, then absence — is
visible in three separate workspaces built to differ only in which of the three
is true.

## Earlier rounds, re-observed rather than assumed

| | Observed |
|---|---|
| **round 25/1** selected absent collection hashes as `null` | the migrated Ref digest reproduced independently, and it is unchanged from `ca6474a` |
| **round 25/2** interrupted withdrawal wedged the workspace | reads describe the predecessor again; `migrate` mints a **fresh** code; the spent one is refused |
| **round 25/3** exclusion wrote a path where a pattern goes | nested `project[1]`: engr writes `/project\[1\]/.engr/local/`, `git check-ignore` exits 0, `git status --untracked-files=all` lists nothing. **Control**: with the unescaped pattern, check-ignore exits 1 and the live code is listed |
| **round 25/4** Event id checked for parsing, not canonicality | an Event whose id is the uppercase spelling and whose own seal verifies is refused, exit 4, naming the line and the id |
| **round 25/5** `verify` walked the stored projection for dependencies | a Ref that exists only in the durable tail is still checked: `verify` FAILs naming the unreadable target |
| **N1** three surfaces, three verdicts | `verify`, `show` and `ls` name the same drift at the same instant, and `verify` still exits 0 |
| **N2** migrated `.gitignore` | byte-identical to what `init` writes, diffed in this run |
| **N3 / round 21** cleanup reported as a migration | rebuilt from a real SIGKILL: `COMPLETE … nothing was migrated, and the spent migration's leftovers are retired` |
| **N4** `repair` refused an id prefix | `repair 01a05e55-74` reaches the gate and prints the exact restoration |
| **N6** Backlog exhaustion invisible | `admitted on attempt 5 against a ceiling of 3` at the moment, `exhausted attempt 5 …` on show, and `review_exhaustion` persisted |
| **N7** pending Candidate not named | `1 pending Human-Gate question will be DISCARDED and must be prepared again: 8TTSM6` |
| **F5** predecessor refusal called the workspace read-only | now `no command works here, reads included` |
| **F6** migration left `.engr/lock` | gone, with `format.json`, `candidates/` and `events/` |
| **F1** `verify` silent on drift | closed; `1 stale` on `ls`, named fields on `verify` and `show` |
| **F3** the `--expect` message | **not closed** — see finding F-1 |
| **F7** one idea, four spellings | still standing, by decision |

## Data safety

| Attack | Result |
|---|---|
| SIGKILL swept across the publication window, 10 instants from 500 ms to 1080 ms | every one resumes to an `objects/` **byte-identical** to an uninterrupted migration and to the workspace this audit carried forward; one Event per stream; reads fail closed while incomplete; second resume exit 3 |
| Round 21: retype the spent code against a workspace that has since reached rev 2 | record hash **identical** before and after; `COMPLETE … nothing was migrated`; leftovers retired; verify passes |
| Correctly resealed out-of-band edit | refused by `verify`, `show`, the JSON surface and the admission gate; `repair` restores exactly the admitted bytes and the Section's original seal comes back |
| Forged Section removal, sealed | reported as divergence, not as a supported deletion |
| Correctly sealed Event with a non-canonical id | refused, exit 4 |
| Correctly sealed Event tail that cannot replay | refused by `verify`, `show` and `ls`, exit 4 (but see F-3) |
| Predecessor moved under a staged plan, before the barrier | refused 4/4, nothing published, the arriving work intact — including work admitted through the released tool's own Human Gate |
| Released build writing into a crashed migration | locked out before the destination is staged: `format.json` is rewritten to `engr-migration-in-progress` and the released build refuses |
| Predecessor history with a purged **prefix** | accepted |
| Predecessor history with a **gap** | refused, `rev 4 does not immediately follow rev 2` |
| 17 integrity/reference tampers, 17 YAML probes, linked worktree, dirty basis, superseded Challenge | all refused, each naming its own reason |

Every tamper in the adversarial suite now hashes the workspace either side of its
own mutation and reports `NO-OP!` rather than a pass if it changed nothing — the
`ca6474a` run found a scenario that had silently stopped testing. All 17 applied.

Domains re-exercised end to end: type/state across all three vocabularies with
invalid pairs refused; supersession with self-supersession and cycles refused;
repair on both damage kinds; Rules with passing, failed-and-overridden and
exhausted review, artifact-exact Rule drift **and its round trip** (restore the
byte, the same review is accepted again); Backlog add/revise/subjects/produced/
merge/consume with the stale-token refusal and the Work interlock; Work
items/results/commits/blockers/dependencies/pause/resume/rm; Collection
membership/order/priority/schedule/state with duplicate membership and duplicate
rank refused, and `members: []` written where the contract requires it while the
four Work lists are omitted.

## Findings

### F-1 [P2] The `--expect` refusal names the wrong token, and then blames the reader

`expect` is two levels: an object of topic-level tokens (`rename`, `add`) plus
one token per point. All six operations that need one emit the **same** sentence:

```text
error: this needs --expect: run `engr backlog show <item> --format json`,
read the point, and pass its expect value back
```

For `revise`, `subjects`, `produced` and `consume` that is correct. For `rename`
and `add` it is not: those take the topic-level token, and a reader who follows
the sentence literally gets

```text
error: what you read is not what is there now; read it again and review the
current wording
```

which is the **same refusal as a genuinely stale token**, on a workspace where
nothing has changed. Re-reading produces the same value, so the advice loops.
All six are in `evidence/expect-messages.txt`, with the control: the topic-level
`rename` token is accepted at exit 0 on the next line.

This is F3's family, and F3 was recorded as fixed at `2ead8d5`. The word "value"
was fixed; which value it is was not.

### F-2 [P3] `ls` is the one surface that calls a divergent Object `ok`

At one instant, on one workspace:

```text
verify              FAIL … its sections is not what its admitted history produced   exit 5
show                !! Object sections is not what its admitted history produced…   exit 5
show --format json  "integrity": "divergent", "attention": true                     exit 5
ls --all            01a05e55-74  open  2 sections  ok                               exit 0
```

`ls` is not blind to the Object's own state — a broken seal prints
`object tampered` in that column. It is blind to this one. `ls` is the cheap
survey command an agent reaches for first, and `ok` is not true of any state
here: nothing admitted those bytes.

**Measured against `ca6474a`: identical.** This is pre-existing and not a
regression of the three commits. It is adjacent to the question already put to
the reviewer — whether `ls` should mark a projection that is *behind* its history
— but distinct from it: for a crash tail `ok` is true of the effective state,
and for divergence it is true of nothing.

### F-3 [P3] `repair` is the only command that cannot see a broken Event tail, and it says the Object is sound

Workspace: an Object at rev 1, intact and sealed, whose stream carries a
correctly sealed, correctly framed, revision-contiguous rev-2 Event that cannot
be applied.

```text
verify       error: … event tail cannot reconcile: section §99 does not exist   exit 4
show <id>    error: … event tail cannot reconcile: section §99 does not exist   exit 4
ls --all     error: … event tail cannot reconcile: section §99 does not exist   exit 4
repair <id>  error: <id> verifies and is what its admitted history produced,
             so there is nothing to repair                                       exit 5
```

`ops::history_fault` filters the stream to `rev <= object.rev` before asking, so
the fault is outside the question; and `prepare_repair_locked`'s eligibility
`ensure!` runs before `ops::provable`, which its own comment names as where
unreplayable history is refused. Nothing is lost — `repair` changes nothing —
but it is an affirmative claim of soundness at the moment somebody is trying to
recover, and it contradicts the three surfaces that just refused.

Every other route to a broken history is caught earlier and reports precisely
(truncated stream, missing stream, a history that does not start at revision 1);
this is the one that gets past the framing rules.

### F-4 [P3] The moved-source comparison is skipped from the first published stream, while some predecessor Objects are still their own bytes

The same out-of-band edit to a predecessor Object, at eight staged instants that
differ only in whether the barrier is up (`evidence/barrier-switch.txt`):

```text
before the barrier  kill@790/800/810/820  confirm exit=5  the edit survives   4/4
after  the barrier  kill@860/880/900/950  confirm exit=0  MIGRATED, edit gone 4/4
```

`published_yet` buys the skip from **one** published Event stream, on the
reasoning that "the source is already being overwritten and there is nothing left
to compare". Measured at that instant (`evidence/publication-order.txt`), one to
two of the three predecessor Object files are still their own bytes — so for
those the comparison was still possible, and it is the file this probe edited.

**Blast radius, stated plainly: neither shipped binary can reach this.** The
barrier is installed before the destination is staged, and from that moment the
released build refuses the workspace outright — measured, and the pre-barrier
window is the one where a real released-tool admission is caught and refused.
What is published over is an out-of-band edit, which is an invalid state anyway.
The finding is that the neighbouring instant refuses the identical state, this
one publishes it silently at exit 0, and `verify` then says PASS with no trace of
what was discarded.

## Notes, not findings

- **The `verify` count and the finding it reports are about different states.**
  `01a05e55-ee FAIL 3 sections` followed by a finding about `§6` — the count is
  of the stored projection, the finding is of the recovered one. The `ca6474a`
  run reported this as O1. It now carries the explanation on the next line
  (`unprojected — 1 admitted event the stored projection has not caught up to;
  the next read applies it`), which is what a careful reader needed.
- **`object.migrated.v1`'s payload member is named nowhere in the shipped
  contract.** The command table stops at the ten commands, and the migration
  section describes the bootstrap Event without its shape. On disk the member is
  `snapshot`; the source design draft §15.6 calls it `value`. `PROTOCOL.md` is
  normative and the draft is not, so this is a gap rather than a conflict — but
  it is the one Event that becomes permanent revision-1 history for every
  migrated Object, and a reader of the compiled protocol cannot learn its shape.
- **The release profile emits one warning the validation matrix cannot see.**
  `cargo build --release -p engr` warns `method as_str is never used`
  (`migration.rs:1471`); it is reachable only from `stop_for_test`, which is
  `#[cfg(debug_assertions)]`. Every matrix command runs in the dev profile, so
  none of them sees it. Present at `ca6474a` too — pre-existing, and the release
  workflow does not use `-D warnings`, so it does not fail a build.

## Answer

**The gate is not met at `b02d05e`.** One P2 and three P3s, none of them a
data-safety defect and none of them a regression introduced by the three commits
under test: the P2 is an incomplete fix from two rounds ago, and all three P3s
are pre-existing and were measured as such against a build of the previously
audited head.

What the three commits set out to do, they did. The round-27 hole is real and is
closed — this run reproduced it at `ca6474a` and watched it fail there — the new
dependency answers reach every surface that has an arm for them, and the
evaluator's question order is visible in three workspaces built to separate it.
Nothing the migration writes moved.

The pattern the last four rounds have shown holds here too, and it is worth
saying explicitly: every finding in this report is a surface that describes one
state in the words of another. That is the same shape as rounds 27, 28 and 29,
one more level out.

## Addendum: all four are fixed at `d3be38a`, and each was re-observed

Recorded here rather than left to the next run, because a fix that was only
reasoned about is what this audit exists to refuse. `transcripts/27-fixes-observed.txt`
and `evidence/barrier-switch-fixed.txt` are a release build of the fix run against
the same workspaces this report was written from.

| | Then | Now |
|---|---|---|
| **F-1** | one sentence for six operations, and the two topic-level ones were sent to the point's token and answered with staleness | `pass back the topic's `expect.add`` / `pass back §1's own `expect``, and the wrong level is `--expect was §1's own `expect`; this operation binds the topic's `expect.add`` at exit 2 |
| **F-2** | `ls --all` → `ok` | `object divergent`; `ls --sections` → `object_divergent`; the alarm → `!! 01a05e55-74 §2 OBJECT DIVERGENT`; `ls --stale` → `01a05e55-74 §- OBJECT DIVERGENT` |
| **F-3** | `verifies and is what its admitted history produced, so there is nothing to repair` | `admitted history cannot be replayed: section §99 does not exist`, exit 5 |
| **F-4** | `MIGRATED`, exit 0, the differing bytes gone without a word | `MIGRATED` plus `note objects/01a05e55-74….json had changed since this migration was confirmed; the confirmed plan was published over it` |

The re-run of the barrier sweep is worth reading for one thing this report could
not measure: at `kill@860ms` the barrier is installed and **no** stream is
published yet, and the source check still refuses. So the barrier alone never
bought the skip — the first published stream did, exactly as the code said, and
that is the instant the note now speaks at.

**The head has moved, so this report is now evidence about a head that no longer
exists.** That is the fourth time in a row, and the gate is what it always was:
a clean-room run against the final head that comes back finding nothing.

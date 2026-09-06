# latest → current dogfood re-run, at `0086bb6`

**Destination under test:** `0086bb6` — two commits past the last audited head:
`d3be38a` answers the four findings of the fourth run, and `0086bb6` then
reverses part of one of them, making navigation cheap again and moving
assessment behind an explicit `ls --verify`.
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Comparison build:** `b02d05e` — the previously audited head, built as a second
binary so that "new" and "pre-existing" are measurements rather than readings of
the diff.
**Previous runs:** `a77887b` (committed audit), `52fe116`
([`rerun-52fe116/`](../rerun-52fe116/REPORT.md)), `ca6474a`
([`rerun-ca6474a/`](../rerun-ca6474a/REPORT.md)), `b02d05e`
([`rerun-b02d05e/`](../rerun-b02d05e/REPORT.md)).

The rule is the one the audit has always had: a scenario is `PASS` only when the
expected behaviour was **observed in a transcript in this run**. Nothing is
`PASS` because the implementation was read and judged likely correct.

```text
engr-latest    engr latest (unknown)     e7d9f99
engr-current   engr latest (0086bb6d)    rebuilt for this run
engr-prev      engr latest (unknown)     b02d05e, built to compare against
```

## The input is the same input, for the fifth time

The predecessor workspace was restored from the pre-migration checkpoint and its
inventory is **byte-for-byte identical** to `evidence/inventory-pre-migration.txt`
on the `#68` branch (`transcripts/01-fixture.txt`). Every difference below
therefore belongs to the code.

Every seal was then reproduced by an implementation that has never seen engr's
own code — RFC 8785 JCS + SHA-256 from scratch for the Section and Object seals,
`EventDigestContract 1` for all three streams, and the migrated `RefDigest`
assembled by hand from #66 §6.5 with the historical values taken from git at the
pinned commit. All `MATCH` (`evidence/digest-premise.txt`). That is the premise
that licenses every forgery below.

## What the two commits changed in the record: nothing

```text
migrated object   a77887b       52fe116       ca6474a       b02d05e       0086bb6
01a05e55-74       8bc54cbed5    8bc54cbed5    8bc54cbed5    8bc54cbed5    8bc54cbed5
01a05e55-e6       aef85ebe6e    aef85ebe6e    15a1e01f15    15a1e01f15    15a1e01f15
01a05e55-ee       b57120eb38    b57120eb38    b57120eb38    b57120eb38    b57120eb38
```

The whole post-migration inventory differs from the `b02d05e` run in exactly the
three Event streams, which carry a fresh Event id and admission instant per run
by construction. `.gitignore`, `VERSION` and all three Objects are identical
bytes (`evidence/post-migration-inventory.txt`). These two commits changed what
the tool *says* about a record, not what it writes.

## The fourth run's four findings, each observed closed

| | Observed at `0086bb6` | And at `b02d05e` |
|---|---|---|
| **F-1 [P2]** `--expect` named the wrong token, then blamed the reader | each of the six refusals names the token *that operation* binds — `expect.rename`, `expect.add`, or `§n`'s own — and a token from the other level is a usage error naming both, not the stale refusal. Control: the topic-level rename token is accepted at exit 0 (`evidence/expect-messages.txt`) | the same sentence for all six |
| **F-2 [P3]** `ls` was the last surface calling a divergent Object `ok` | `ls --all` prints `unchecked`; `ls --verify` prints `§- OBJECT DIVERGENT` | `ls --all` prints `ok`; `ls --stale` says nothing about the Object at all |
| **F-3 [P3]** `repair` called an Object with a broken Event tail sound | `error: admitted history cannot be replayed: section §99 does not exist`, exit 5, agreeing with verify/show | `verifies and is what its admitted history produced, so there is nothing to repair`, exit 5, while verify and show exit 4 |
| **F-4 [P3]** publication wrote over predecessor bytes in silence | `MIGRATED …` followed by `note  objects/01a05e55-74….json had changed since this migration was confirmed; the confirmed plan was published over it` (`evidence/source-moved-report.txt`) | `MIGRATED …` alone |

`ls --verify` is also strictly wider than the `ls --stale` it replaces: on the
same three workspaces it reports `OBJECT DIVERGENT`, `OBJECT TAMPERED` and
`REF MISSING` where `--stale` reported only the Section-level half.

## Earlier rounds, re-observed rather than assumed

| | Observed |
|---|---|
| **round 21** cleanup reported as a migration | rebuilt from a real SIGKILL at 1080 ms: `COMPLETE … nothing was migrated`, record hash **IDENTICAL** either side of retyping the spent code, leftovers retired, verify passes |
| **round 25/1** selected absent collection hashes as `null` | the migrated Ref digest reproduced independently, unchanged from `b02d05e` |
| **round 25/2** interrupted withdrawal wedged the workspace | reads describe the predecessor again; `migrate` mints a **fresh** code (`R4NJ65`); the spent one is refused |
| **round 25/3** exclusion wrote a path where a pattern goes | nested `project[1]`: engr writes `/project\[1\]/.engr/local/`, `check-ignore` exits 0, git sees nothing. **Control**: the unescaped pattern exits 1 and lists the live code |
| **round 25/4** Event id checked for parsing, not canonicality | an Event whose id is the uppercase spelling and whose own seal verifies is refused, exit 4, naming the line and the id |
| **round 25/5** `verify` walked the stored projection for dependencies | a Ref admitted in a crash tail is still checked: `§6 stands on 01a05e55-74 §1, which will not load` |
| **round 27 [P1]** a dependency is a claim about authority | a correctly resealed field forged onto the Ref target: `verify` FAILs on **both** Objects, the dependent naming `§2 stands on 01a05e55-74 §1, which seals correctly and is not what its own history produced` |
| **round 28 [P2]** a new answer is an obligation at every consumer | `show` → `REF UNADMITTED` exit 5; `ls --verify` lists it; the four Work lists are omitted when empty and every explicit `[]` spelling is refused (`evidence/work-empty-lists.txt`) |
| **round 29 [P2]** asking the last question first | three workspaces differing only in which question is true: resealed removal → `OBJECT DIVERGENT`; the same removal unsealed → `OBJECT TAMPERED` (integrity first); removal through an admitted Event → target PASSes at rev 2 and the dependent says `REF MISSING` |
| **F5** predecessor refusal called the workspace read-only | `no command works here, reads included` |
| **F6** migration left `.engr/lock` | gone, with `format.json`, `candidates/` and `events/` |
| **F7** one idea, four spellings | still standing, by decision |

## Data safety

| Attack | Result |
|---|---|
| SIGKILL swept across the publication window, 10 instants from 500 to 1080 ms | every one resumes to an `objects/` **byte-identical** to an uninterrupted migration and to the workspace this audit carried forward; one Event per stream; reads fail closed while incomplete; second resume exit 3 |
| Round 21: retype the spent code against a workspace that has since reached rev 2 | record hash identical before and after; leftovers retired; verify passes |
| Correctly resealed out-of-band edit | refused by `verify`, `show`, the JSON surface, `ls --verify` and the admission gate |
| Forged Section removal, sealed | divergence, not a supported deletion — with both controls |
| Correctly sealed Event with a non-canonical id | refused, exit 4 |
| Correctly sealed Event tail that cannot replay | refused by `verify`, `show` and `ls --verify`, exit 4; `repair` now agrees |
| Released build writing into a crashed migration, inside the pre-barrier window | the resume **refuses** and names the qualified response; the work admitted through the released tool's own Human Gate is still in the predecessor record |
| Predecessor moved under a staged plan, before publication | refused, nothing published, the edit intact |
| Predecessor history with a purged **prefix** | accepted |
| Predecessor history with a **gap** | refused, `rev 4 does not immediately follow rev 2` |
| 17 integrity/reference tampers, 17 YAML probes, linked worktree, dirty basis, superseded Challenge | all refused, each naming its own reason |

Every adversarial probe hashes the workspace either side of its own mutation and
prints `NO-OP!` rather than passing silently; all 17 applied in this run.

Domains re-exercised end to end: type/state across all three vocabularies with
the invalid pairs refused; supersession with self-supersession and cycles
refused; Rules with passing, failed-and-overridden and exhausted review, plus
artifact-exact Rule drift **and its round trip**; Backlog
add/revise/subjects/produced/merge/consume with the stale-token refusal, the
exhaustion diagnostic persisted, and the Work interlock; Work
items/results/commits/blockers/dependencies/pause/resume/rm; Collection
membership/order/priority/schedule/state with both uniqueness rules and the id
grammar attacked.

## Findings

### F-1 [P2] `ls --verify` says `all ok` about a workspace `verify` fails, and nothing lists the Object at all

One workspace, one instant, an Object whose stored projection is missing while
its Event stream is intact (`transcripts/27-missing-projection.txt`):

```text
verify               FAIL  required Object projection is missing;
                           admitted history reconstructs it                  exit 5
ls --verify          all ok                                                  exit 0
ls --all             01a05e55-74 is not listed                               exit 0
ls --all --sections  its two Sections are not listed                         exit 0
show 01a05e55-74     prints it, "2 sections  2 ok", says nothing             exit 0
```

Two separate things meet here. Plain `ls` enumerates through
`store::object_ids`, which is the *file* listing, so an Object that exists only
in the record is not discovered — `PROTOCOL.md` and `README.md` both say so.
`ls --verify` enumerates through `ops::object_ids`, which **does** include it,
loads it with `ops::effective`, which reconstructs it soundly from history, and
then has no arm for the fault: `object_fault` asks integrity and history, and
"the projection is not on disk" is neither. Only `ops::verify` asks it.

So the surface that is entitled to be expensive, and that `SKILL.md` tells an
agent answers *what stopped adding up*, affirmatively reports `all ok`.

**Measured against `b02d05e`: the `all ok` is pre-existing — `ls --stale` said
it too. What is new is that the Object has also disappeared from the listing**,
where `b02d05e` printed it (as `ok`). Before, an agent surveying this workspace
saw a row it could not fully trust; now it sees no row.

Nothing is lost: the reconstruction is correct, and the next admitted mutation
rewrites the projection and the workspace verifies again — observed
(`transcripts/09-missing-projection.txt`). This is a reporting defect, not a
data-safety one. It is also the shape rounds 27–29 kept finding: a fault class
one consumer knows about and another does not.

### F-2 [P3] The Skill was not told that the listing changed

`0086bb6` changed `skill/SKILL.md` by exactly one word — `ls --stale` became
`ls --verify`. The word `unchecked` appears three times in `README.md`, three
times in `PROTOCOL.md`, and **never** in `SKILL.md`.

The sentence that matters is the search instruction, unchanged:

> Before making or revisiting a significant architectural or behavioral
> decision, search existing titles and section wording with
> `engr ls --all --sections` and an appropriate text search.

That surface is now a stored-projection survey. On a workspace with a legitimate
crash tail — the Event durable, the projection one revision behind — the two
builds disagree at the same instant (`transcripts/24-tail-listing.txt`):

```text
b02d05e   01a05e55-ee §6  ok         Wording admitted immediately before the crash…
0086bb6   (not listed)
```

The wording is admitted, `show` reconciles it forward, and the surface the Skill
sends an agent to search omits it. `unchecked` is a claim about **trust**; the
reader's risk here is **completeness**, and no marker on the rows that are shown
says a row is missing. `README.md` carries the caveat ("crash tails and missing
projections are not recovered here"); the document an agent actually loads does
not.

The same asymmetry covers F-1's state: a whole Object silently absent from the
surface the Skill recommends for finding out what the record already holds.

## Notes, not findings

- **`ls <keyword> --verify` accepts a keyword and ignores it.** The keyword
  filter lives in `render_ls`; `render_ls_verify` never sees it, so the
  assessment listing reports every Object regardless. **Identical at `b02d05e`
  with `--stale`** — pre-existing. `--verify` is mutually exclusive with
  `--sections` but not with the keyword (`transcripts/25-ls-flags.txt`).
- **`--all` still has an empty `--help` description**, at both heads.
- **The `ls --sections` stderr alarm no longer names which Section's seal
  broke.** With a broken aggregate seal every row is labelled `OBJECT TAMPERED`,
  where `b02d05e` said `§1 TAMPERED` for the one that was actually edited.
  `show` still names it. Defensible — an Object-level fault is every row's
  fault — but the precise information now lives only on the deeper surface.
- **One corrupt Object file still blinds the whole listing**: every `ls` variant
  exits 4 with the parse error and no other Object is printed. Identical at both
  heads.
- **`object.migrated.v1`'s payload member is still named nowhere in the shipped
  contract** (on disk it is `snapshot`; the source design draft §15.6 calls it
  `value`). Carried forward from the fourth run.
- **The release profile still warns `method as_str is never used`**
  (`migration.rs`), at both heads; no matrix command runs the release profile.
- **A harness trap worth keeping.** Building the comparison binary from a second
  archived source tree into the `CARGO_TARGET_DIR` the previous comparison used
  finished in 0.25 s and copied out the **previous head's** binary at exit 0.
  It was caught by asking the binary a question only the intended head answers
  differently (`engr protocol | grep -c 'omitted when empty'` — 2 at `ca6474a`,
  7 at `b02d05e`) before using it. Build each comparison head into its own
  target volume, and check the binary is the one you meant.

## Answer

**The gate is not met at `0086bb6`.** One P2 and one P3, neither a data-safety
defect. Nothing the two commits set out to do is unfinished: all four findings
of the fourth run are closed and were watched failing at `b02d05e` in the same
transcripts, the migration writes exactly the bytes it wrote at every earlier
head, and every data-safety attack this audit has accumulated was refused again.

Both findings are about the same decision, and it is a decision the reviewer
made deliberately: navigation stopped answering questions it could not afford.
The protocol and the README were rewritten to say so. What was not carried
through is the pair of consumers either side of that line — the assessment
surface, which now owns the questions `ls` gave up and does not yet own all of
them, and the Skill, which still tells an agent to survey the record with a
command that no longer surveys the record.

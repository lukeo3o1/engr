# latest → current dogfood re-run, at `9fba620`

**Destination under test:** `9fba620` — the head that answers review
`5124757951` (two P2 blockers, two P3s).
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Comparison build:** `0086bb6` — the previously audited head, built as a second
binary so "closed" and "pre-existing" below are measurements rather than
readings of the diff.
**Previous runs:** `a77887b` (committed audit), `52fe116`, `ca6474a`, `b02d05e`,
`0086bb6` ([`rerun-0086bb6/`](../rerun-0086bb6/REPORT.md)).

A scenario is `PASS` only when the expected behaviour was **observed in a
transcript in this run**. Nothing is `PASS` because the implementation was read
and judged likely correct.

```text
engr-latest    engr latest (unknown)     e7d9f99
engr-current   engr latest (9fba6207)    rebuilt for this run
engr-prev      engr latest (unknown)     0086bb6, built to compare against
```

Both archived trees report `engr latest (unknown)`, so each binary was proved to
be the head intended by asking it something only that head answers differently:
`engr protocol | grep -c 'What navigation gives up'` — 0 at `0086bb6`, 1 at
`9fba620`.

## The input is the same input, for the sixth time

Restored from the pre-migration checkpoint; its inventory is **byte-for-byte
identical** to `evidence/inventory-pre-migration.txt` on the `#68` branch
(`transcripts/01-fixture.txt`). Every seal was then reproduced by an
implementation that has never seen engr's code — 14 of 14 `MATCH`, no mismatch
(`evidence/digest-premise.txt`).

## What round 31's repairs changed in the record: nothing

The whole post-migration inventory differs from the fifth run in exactly the
three Event streams, which carry a fresh Event id and admission instant per run
by construction. `.gitignore`, `VERSION` and all three Objects are identical
bytes (`evidence/post-migration-inventory.txt`), and identical to `a77887b`.

## Round 31's four, each observed closed against the head that failed them

| | at `9fba620` | at `0086bb6` |
|---|---|---|
| **[P2]** `ls --verify` reported `all ok` where `verify` fails | `· 01a05e55-74  open  §-  OBJECT PROJECTION MISSING`, exit 0 | `all ok`, exit 0 |
| ↳ its control, on the same workspace | the Object whose projection is one revision *behind* is **not** named; exactly one row | — |
| **[P2]** wording flags read and discarded | **16 of 16** refused as usage errors naming the flag, workspace unchanged | **16 of 16 exit 0**, a Challenge minted every time (`evidence/content-flags.txt`) |
| **[P3]** the Skill's search guidance | *A search that finds nothing is not proof that the record holds nothing*, shipped in `skill/SKILL.md` | one word changed, no caveat |
| **[P3]** the overwrite warning lost to a second interruption | after a **real SIGKILL** between the overwrite and the report, the next resume still names `objects/01a05e55-74….json` | the next resume prints `MIGRATED` and nothing else (`evidence/overwrite-report.txt`) |

The last of those is worth naming as a method point: the reviewer's own
construction used the `#[cfg(debug_assertions)]` stop hook, which a release
binary does not have. It was reproduced here by measuring the resume (381 ms
uninterrupted) and sweeping a real `SIGKILL` across it until one landed after
the overwrite and before the report — at 260 ms, with `VERSION` not yet written.

## Earlier rounds, re-observed rather than assumed

| | Observed |
|---|---|
| **round 21** cleanup reported as a migration | rebuilt from a real SIGKILL at 1090 ms: `COMPLETE … nothing was migrated`, record hash **IDENTICAL** either side of retyping the spent code, leftovers retired, verify passes |
| **round 25/1** selected absent collection hashes as `null` | the migrated Ref digest reproduced independently, unchanged |
| **round 25/2** interrupted withdrawal wedged the workspace | reads describe the predecessor again; `migrate` mints a fresh code; the spent one is refused |
| **round 25/3** exclusion wrote a path where a pattern goes | nested `project[1]`: `check-ignore` exits 0 and git sees nothing; the unescaped control exits 1 and lists the live code |
| **round 25/4** Event id checked for parsing, not canonicality | an Event whose id is the uppercase spelling and whose own seal verifies is refused, exit 4, naming the line and the id |
| **round 25/5** `verify` walked the stored projection for dependencies | a Ref admitted in a crash tail is still checked |
| **round 27 [P1]** a dependency is a claim about authority | (covered by the assessment states and the adversarial reference suite) |
| **round 28 [P2]** the four Work lists | all four explicit `[]` spellings refused, the omitted ones accepted |
| **round 29 [P2]** integrity, then history, then absence | the adversarial suite's 17 tampers each refused with their own reason |
| **F5 / F6** | predecessor refusal says *no command works here, reads included*; `.engr/lock`, `format.json`, `candidates/` and `events/` all gone after migration |
| **F7** | still standing, by decision |

## Data safety

| Attack | Result |
|---|---|
| SIGKILL swept across the publication window, 9 instants from 500 to 1120 ms | every one resumes to an `objects/` **byte-identical** to an uninterrupted migration and to the workspace this audit carried forward; verify PASSes on all; second resume exit 3 |
| Round 21: retype the spent code against a workspace that has since reached rev 2 | record hash identical before and after; leftovers retired |
| Released build writing inside the pre-barrier window | the resume **refuses** and names the qualified response; the work it admitted is still in the predecessor record |
| Predecessor moved under a staged plan, before publication | refused, nothing published, the edit intact |
| Predecessor history with a purged **prefix** / with a **gap** | accepted / refused, `rev 4 does not immediately follow rev 2` |
| 17 integrity and reference tampers | all refused — 12 at exit 4, 5 at exit 5, no `NO-OP!` |
| 17 YAML profile probes | 2 must-load accepted, 15 refused, each naming its own reason |
| Linked worktree, dirty basis, superseded Challenge | exclusion lands in the common dir, live code never visible to git, stale code refused |

Domains re-exercised end to end: type/state across all three vocabularies with
the invalid pairs refused; supersession with self-supersession and cycles
refused; Rules with passing, failed-and-overridden and exhausted review, plus
artifact-exact Rule drift **and its round trip**; Backlog
subjects/produced/merge/consume with the stale-token refusal, the exhaustion
diagnostic persisted, and the Work interlock; Work
items/results/commits/blockers/dependencies/pause/resume/rm; Collection
membership/order/priority/schedule/state with both uniqueness rules and the id
grammar attacked.

## Finding

### F-1 [P3] `show` still calls an Object with no stored bytes `"integrity": "ok"`

One workspace, one instant, two Objects that differ only in *how far* their
projection has fallen behind the record (`transcripts/24-show-missing.txt`):

```text
01a05e55-ee   projection one revision behind
  show     note  reconciled 1 admitted event the stored projection was behind;
                 it is now rev 2                                              exit 0

01a05e55-74   projection absent entirely
  verify   FAIL  required Object projection is missing                        exit 5
  ls --verify    §-  OBJECT PROJECTION MISSING                                exit 0
  show     2 sections   2 ok   — no note of any kind                          exit 0
  show --format json   "integrity": "ok"                                      exit 0
```

`show` **has an arm for the neighbouring state and uses it**: a projection that
is behind gets a `note` naming the reconciliation. A projection that is not
there at all gets nothing, and the JSON surface an agent reads answers
`"integrity": "ok"` — over an Object whose stored bytes do not exist. That is
the affirmative health report the round-31 ruling called the defect, on the one
consumer the ruling did not name.

**Measured against `0086bb6`: identical.** Pre-existing, and not a regression of
the repairs — the fix went to `view::object_fault`, which only `ls --verify`
calls. Nothing is at risk: the reconstruction is correct, `verify` catches it,
and the next admitted mutation rewrites the projection. It is a reporting gap of
exactly the shape rounds 28 and 31 were about — a fault class one consumer knows
about and its neighbour does not.

## Notes, not findings

- **A stage left holding only `published-over.json` does not wedge the
  workspace**: `ls` and `verify` both succeed at exit 0. The file is not swept
  by an opportunistic cleanup, so it can sit there — a stray local file, not a
  refusal. Reachable only by an interruption inside a recursive removal.
- **An Object path that exists and is not a regular file** is refused by all
  four surfaces with the same message and exit 8, identically at both heads: the
  new `resource_present` question in `object_fault` is never reached, because
  loading errors first.
- **`ls --verify`'s object-level row carries no title** — an abbreviated id, the
  classification and the fault. True of the other object-level rows too.
- **The release profile still warns `method as_str is never used`**
  (`migration.rs`), at both heads; no matrix command runs the release profile.
- **`object.migrated.v1`'s payload member is still named nowhere in the shipped
  contract** (on disk it is `snapshot`). Carried forward from the fourth run.
- **Harness sequencing matters.** Installing the audit's Rules before the
  lifecycle and supersession exercises made every one of those mutations demand
  a review, and the plain gate driver has none — twelve refusals that say
  nothing about the tool. The parent runs install the Rules *after* those
  exercises; that order is now recorded in the runbook.

## Answer

**The gate is not met at `9fba620`, and this is the smallest result any run has
produced: one P3, pre-existing, on a surface the last ruling did not name.**

All four of round 31's repairs are closed, and each was watched failing at
`0086bb6` in the same probe — including the one the reviewer could only
construct with a debug-only stop hook, reproduced here with a real SIGKILL on a
release build. Nothing the migration writes moved. Every data-safety attack this
audit has accumulated was refused again, and nine SIGKILL instants across the
publication window all resume to the same bytes.

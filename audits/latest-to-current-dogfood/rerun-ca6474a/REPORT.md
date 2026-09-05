# latest → current dogfood re-run, at `ca6474a`

**Destination under test:** `ca6474a` — the head that answers the blocking
re-review of `2ead8d5`, all four checks green.
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Previous runs:** `a77887b` (committed audit), `52fe116`
([`rerun-52fe116/`](../rerun-52fe116/REPORT.md)).

The rule is the one the audit has always had: a scenario is `PASS` only when the
expected behaviour was **observed in a transcript in this run**. Nothing is
`PASS` because the implementation was read and judged likely correct.

## The input is the same input, for the third time

The predecessor workspace was restored from the pre-migration checkpoint and its
inventory is **byte-for-byte identical** to `evidence/inventory-pre-migration.txt`
on the `#68` branch. Every difference below therefore belongs to the code.

```text
engr-latest    engr latest (unknown)     e7d9f99
engr-current   engr latest (ca6474a3)    rebuilt for this run
```

## What eighteen commits changed in the record, exactly

```text
migrated object   a77887b       52fe116       ca6474a
01a05e55-74       8bc54cbed5    8bc54cbed5    8bc54cbed5
01a05e55-e6       aef85ebe6e    aef85ebe6e    15a1e01f15
01a05e55-ee       b57120eb38    b57120eb38    b57120eb38
```

**One object, once.** `01a05e55-e6` is the only Object in the fixture carrying a
selective Ref, and it changed at `ca6474a` because the RefDigest projection was
corrected: a selected absent optional collection now hashes as `null` rather than
`[]`. The new digest was reproduced here by an **independent implementation** of
#66 §6.5 — the preimage assembled by hand, canonicalized with RFC 8785 JCS and
hashed — and it agrees:

```text
fields    ["based_on","refs","text"]
values    {"based_on":{...},"refs":null,"text":"..."}
stored    1:088da847d1e83ef0d8d6d610e91ca0abe562eb92aab7214a673c19cf429d7fda
computed  1:088da847d1e83ef0d8d6d610e91ca0abe562eb92aab7214a673c19cf429d7fda
```

`EventDigestContract 1` was reproduced the same way, independently, and every
Section and Object seal in the migrated workspace recomputes.

**The cost, stated plainly:** a workspace migrated by an earlier head of this
branch carries Refs sealed under the old projection and now fails verification.
This run saw that happen. #66 §2.4 promises no compatibility for intermediate
development formats, so it is allowed — but it is a real effect.

## The five findings of review `5120893221`, each observed

| Finding | Observed at `ca6474a` |
|---|---|
| **1** selected absent collection hashed as `[]` | migrated Ref now `"refs": null`; digest reproduced independently, and it is the only byte that moved in three heads |
| **2** interrupted withdrawal wedged the workspace | the state built by unlinking exactly what the interruption unlinks: reads work again (`the released predecessor workspace…`, not `incomplete coordinated migration`), and `migrate` mints a **fresh** code |
| **3** exclusion wrote a path where a pattern goes | nested `project[1]`: engr writes `/project\[1\]/.engr/local/`, `git check-ignore` exits 0 on the real Challenge path, `git status --untracked-files=all` lists nothing. **Control**: with the old unescaped pattern, check-ignore exits 1 and status lists `project[1]/.engr/local/challenges/8L8JLN.json` |
| **4** Event id checked for parsing, not canonicality | an Event whose id is the uppercase spelling and whose **own seal verifies** is refused, exit 4, naming the line and the id |
| **5** `verify` walked the older projection for dependencies | a probe whose stored projection has **no sections at all** and whose Ref exists only in the durable tail: `verify` FAILs and names the unreadable target. Before the fix this returned PASS |

## The eight findings of the `52fe116` run, each still closed

| | Observed |
|---|---|
| **N1** three surfaces, three verdicts | `verify` PASS + `unprojected — 1 admitted event…`; `repair` "nothing to repair"; `show` prints `note reconciled …`. They agree |
| **N2** migrated `.gitignore` | byte-identical to what `init` writes; `/lock` and `/candidates/` gone |
| **N3** cleanup reported itself as a migration | a real SIGKILL crash window, code retyped: `COMPLETE … nothing was migrated, and the spent migration's leftovers are retired` |
| **N4** `repair` refused an id prefix | `repair 01a0715e-1e` reaches the gate |
| **N5** `change_state` for a typed object | the compiled protocol's table row now describes what the command does |
| **N6** Backlog exhaustion invisible | `note admitted on attempt 5 against a ceiling of 3` at the moment, and `exhausted attempt 5 against a ceiling of 3` on `backlog show` |
| **N7** pending Candidate not named | `1 pending Human-Gate question will be DISCARDED and must be prepared again: 8TTSM6`, and `No Human-Gate question is pending` where there is none |
| **N8 / F7** one idea, four spellings | **still standing**, by decision — put to the reviewer rather than done |
| **F1** `verify` silent on drift | `verify`, `show` and `ls` now agree at the same instant, and `verify` still exits 0 |
| **F3, F5** | fixed and re-observed |

## Data safety

Every attack held, including the two this audit built rather than inherited.

| Attack | Result |
|---|---|
| Round 21: retype the spent code against a workspace that has since reached rev 2 | Object and Event stream **byte-identical** before and after; leftovers swept; and the screen now says nothing was migrated |
| Correctly resealed out-of-band edit | refused, exit 5, named as a reseal rather than as damage |
| Forged base under a crash tail | refused; the tail was **not** applied; the forgery stayed on disk as evidence |
| SIGKILL swept across the 1144 ms publication window | every instant resumes to an `objects/` byte-identical to an uninterrupted migration and to the carried-forward workspace; one Event per stream; second resume exit 3 |
| Predecessor history with a purged **prefix** | accepted; byte-identical record |
| Predecessor history with a **gap** | refused, exit 4, naming `rev 4 does not immediately follow rev 2` |
| 15 integrity/reference tampers, 17 YAML probes, linked worktree, dirty basis, stale Challenge | all refused, each naming its own reason |

Domains re-exercised end to end: type/state across all three vocabularies with
invalid pairs refused, supersession with self-supersession and cycles refused,
repair on both damage kinds, Rules with passing/failed/exhausted review and
artifact-exact Rule drift, Backlog add/revise/subjects/produced/merge/consume
with the stale-token refusal and the Work interlock, Work items/blockers/
dependencies/pause/resume/rm, Collection membership/order/priority/schedule/state
with duplicate membership and duplicate rank refused.

## Findings

### O1 [P3] `verify` counts the stored sections and reports the recovered ones

Introduced by the fix for finding 5, and visible only in the state that fix
exists for:

```text
01a0715e-1e  FAIL  0 sections  Crash-tail dependency probe
          §1 stands on 01a05e55-74 §1, which will not load: …
          unprojected — 1 admitted event the stored projection has not caught up to
```

Both halves are correct and deliberate — seals are asked of the stored bytes, and
dependencies of the admitted record — but the line says the Object has no
sections and then reports a finding about §1. The `unprojected` note is the
reconciliation, so a careful reader gets there. A quick one, or a script, does
not. Either the count or the finding could name which state it is about; that is
a call, not a defect, so it is reported rather than changed.

### Method note, not a finding

The adversarial harness pins the fixture's Ref digest as a literal, and the
RefDigest fix moved it — so `sed` matched nothing, no tamper was applied, and the
"invalid Ref digest" scenario silently **passed as a no-op** at exit 0. It was
caught because a scenario that had failed for two runs suddenly succeeded, not
because anything reported it. A harness that mutates by literal match can stop
testing without saying so; the constant is updated and the scenario refuses again
at exit 5.

The same class shows up in the drift suite's labels: `adv-final.sh` calls one
case "unselected field changed", and now that `verify` names what moved, the
output shows `based_on` moving too — because admitting a revision re-derives the
basis, and `based_on` **is** selected by a migrated Ref. The behaviour was always
right; the label was never accurate, and only became checkable when the surface
started speaking.

## Answer

**Nothing was found.** No data-safety defect, no regression, and no surface
defect beyond one P3 observation about a display that is correct in both halves.
Every finding of the previous two rounds is closed and re-observed rather than
assumed, and every attack — including one rebuilt from a bug's own description
and two forgeries built with an independent implementation of the digest
contracts — was refused or recovered from without losing a byte.

The gate as written is met by this run. Whether it is met *for the merge* depends
on nothing moving again: this is the third head audited, and each of the previous
two came back with findings that the next head then had to answer.

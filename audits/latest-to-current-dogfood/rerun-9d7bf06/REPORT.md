# latest → current dogfood re-run, at `9d7bf06`

**Destination under test:** `9d7bf06` — the head PR #67 stands at, carrying
round 32 (`259b8b4`) and round 33 (`9d7bf06`).
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Comparison build:** `9fba620` — the previously audited head, built as a second
binary so "closed" and "pre-existing" below are measurements rather than
readings of the diff.
**Previous runs:** `a77887b` (committed audit), `52fe116`, `ca6474a`, `b02d05e`,
`0086bb6`, `9fba620` ([`rerun-9fba620/`](../rerun-9fba620/REPORT.md)).

A scenario is `PASS` only when the expected behaviour was **observed in a
transcript in this run**. Nothing is `PASS` because the implementation was read
and judged likely correct.

```text
engr-latest    engr latest (unknown)     e7d9f99
engr-current   engr latest (unknown)     9d7bf06, rebuilt for this run
engr-prev      engr latest (unknown)     9fba620, built to compare against
```

All three archived trees report `engr latest (unknown)`, so each binary was
proved to be the head intended by asking it something only that head answers.
PROTOCOL.md is compiled in, and each round added a paragraph
(`evidence/binary-provenance.txt`):

| probe | latest | prev | current |
|---|---|---|---|
| round 31's `What navigation gives up` | 0 | 1 | 1 |
| round 32's `Every surface that classifies an Object owes the same answer` | 0 | 0 | 1 |
| round 33's `a repair that offers a confirmation code` | 0 | 0 | 1 |

Each head was built into **its own** `CARGO_TARGET_DIR` volume, and the copied
binary's sha256 was checked against the one in the volume.

## The input is the same input, for the seventh time

Restored from the pre-migration checkpoint; its inventory is **byte-for-byte
identical** to `evidence/inventory-pre-migration.txt` on the `#68` branch
(`transcripts/01-fixture.txt`). Every seal was then reproduced by an
implementation that has never seen engr's code — 14 of 14 `MATCH`, no mismatch
(`evidence/digest-premise.txt`).

## What rounds 32 and 33 changed in the record: nothing

The whole post-migration inventory differs from the fifth *and* sixth runs in
exactly the three Event streams, which carry a fresh Event id and admission
instant per run by construction. `.gitignore`, `VERSION` and all three Objects
are identical bytes to both (`evidence/post-migration-checkpoint.txt`).

## Rounds 32 and 33, each observed closed against the head that failed them

`show`, `ls --verify`, `verify` and `show --format json` asked of one Object in
each of the five states the classifier now has, at both heads
(`evidence/five-states.txt`):

| state | current | prev |
|---|---|---|
| **projection absent** | `show` exit **5**, banner `Object has no stored projection… Restore the file with: engr repair`, `"integrity": "projection_missing"` | `show` exit **0**, no banner, **`"integrity": "ok"`** |
| projection one revision behind (the control) | exit 0, `ok` | exit 0, `ok` |
| tampered | exit 5, `tampered` | exit 5, `tampered` |
| divergent | exit 5, `divergent` | exit 5, `divergent` |
| unreplayable | exit 4 at every surface | exit 4 at every surface |

`show --format json`'s `integrity` member was moved onto `view::object_fault`,
the classifier the listing uses. That is a new consumer for four pre-existing
answers as well as a new answer, so all five were measured: the four that
existed are unchanged at both heads.

`repair` now accepts the absent state, and refuses it at `prev` with `not found`,
exit 3 (`evidence/repair-screens.txt`). Confirming it writes the projection
back, appends `object.repaired.v1` at rev 2, and the workspace verifies —
identically for the absent, divergent and tampered cases, all three landing the
same restored bytes (`evidence/repair-confirm.txt`).

Round 33's screen: the absent-projection repair prints every restored member,
including both Sections' complete wording, and `candidate <code>` re-renders it
**IDENTICALLY** (diffed in `evidence/repair-screens.txt`).

## Earlier rounds, re-observed rather than assumed

| | Observed |
|---|---|
| **round 21** cleanup reported as a migration | landed in the window at 1075 ms: `COMPLETE … nothing was migrated`, record hash **IDENTICAL** either side of retyping the spent code, leftovers retired, verify passes |
| **round 25/1** selected absent collection hashes as `null` | the migrated Ref digest reproduced independently, unchanged |
| **round 25/2** interrupted withdrawal wedged the workspace | reads describe the predecessor again; `migrate` mints a fresh code; the spent one is refused |
| **round 25/3** exclusion wrote a path where a pattern goes | nested `project[1]`: `check-ignore` exits 0 and git sees nothing; the unescaped control exits 1 and lists the live code |
| **round 25/4** Event id checked for parsing, not canonicality | an Event whose id is the uppercase spelling and whose own seal verifies is refused, exit 4, naming the line and the id |
| **round 25/5** `verify` walked the stored projection for dependencies | a Ref admitted in a crash tail is still checked; `show` names the unreadable target at the same instant |
| **round 27 / r4 F-3** repair called an unreplayable Object sound | refused, exit 5, `admitted history cannot be replayed: section §99 does not exist` |
| **round 28 [P2]** the four Work lists | all four explicit `[]` spellings refused at exit 4, the omitted ones accepted |
| **round 29 [P2]** integrity, then history, then absence | `dep-e` divergent, `dep-f` tampered, `dep-g` (a real admitted deletion) sound and `repair` refuses it — the third control rebuilt after the first attempt spelled the flag wrong and deleted nothing |
| **round 31 [P2]** `ls --verify` reported `all ok` | names the fault; `ls --verify` keeps the survey's exit convention, which PROTOCOL.md rules explicitly |
| **round 31 [P2]** wording flags read and discarded | **32 of 32** refused as usage errors naming the flag at both heads, workspace unchanged |
| **round 31 [P3]** overwrite warning lost to a second interruption | after a real SIGKILL between the overwrite and the report, the next resume still names `objects/01a05e55-74….json`, at both heads |
| **r6 F-1** `show` called an absent projection `"integrity": "ok"` | closed; see the table above |
| **F5 / F6** | predecessor refusal says *no command works here, reads included*; `.engr/lock`, `format.json`, `candidates/` and `events/` all gone after migration |
| **F7** | still standing, by decision |

## Data safety

| Attack | Result |
|---|---|
| SIGKILL swept across the publication window, **18 instants** from 500 to 1120 ms | every one resumes to an `objects/` **byte-identical** to an uninterrupted migration and to the checkpoint this audit carried forward; reads fail closed while incomplete; second resume exit 3; verify PASSes on all 18 |
| Round 21: retype the spent code against a workspace that has since reached rev 2 | record hash identical before and after; leftovers retired |
| Released build writing inside the pre-barrier window | the resume **refuses** and names the qualified response; the work it admitted is still in the predecessor record |
| Predecessor moved under a staged plan, before publication | refused, nothing published, the edit intact |
| Predecessor history with a purged **prefix** / with a **gap** | accepted / refused, `rev 4 does not immediately follow rev 2` |
| 17 integrity and reference tampers | all refused — 12 at exit 4, 5 at exit 5, no `NO-OP!` |
| 17 YAML profile probes | 2 must-load accepted, 15 refused, each naming its own reason |
| Two repair codes prepared, first confirmed | preparing the second retires the first; a spent code is refused `no challenge awaiting`; an ordinary mutation between prepare and confirm retires the pending repair |
| Linked worktree, dirty basis, superseded Challenge | exclusion lands in the common dir, live code never visible to git, stale code refused |

Domains re-exercised end to end: type/state across all three vocabularies with
the invalid pairs refused; supersession with self-supersession and cycles
refused; Rules with passing, failed-and-overridden and exhausted review, four
review refusals each firing for its own reason, plus artifact-exact Rule drift
**and its round trip**; Backlog subjects/produced/merge/consume with the stale
token refused, the exhaustion diagnostic persisted, the Work interlock, and every
`expect` message naming exactly the token it wants; Work
items/results/commits/blockers/dependencies/pause/resume/rm; Collection
membership/order/priority/schedule/state with both uniqueness rules and the id
grammar attacked.

## Findings

### F-1 [P2] The revision rule this head's own protocol adds is met on one repair screen out of three

`9d7bf06` ships this sentence in `protocol/PROTOCOL.md`, which is compiled into
the binary:

> **And a repair that offers a confirmation code MUST first show what it will
> write.** … Where a projection is damaged the screen compares stored against
> restored; where none is stored the comparison still has the side that matters
> … **It MUST also distinguish the revision admitted history derives from the
> revision confirming produces: they are different numbers, and a repair is
> admitted rather than silent.**

Three repair screens, all three offering a confirmation code, one workspace each
(`evidence/repair-screens.txt`):

```text
absent      Restoring  exactly what admitted history proves at rev 1, admitted as rev 2
divergent   Restoring  exactly what admitted history proves, and nothing from the stored bytes
tampered    Restoring  exactly what admitted history proves, and nothing from the stored bytes

lines naming a revision:   absent 1   divergent 0   tampered 0
```

The two damaged screens are the **older and ordinary** repair path — the one a
reader reaches from `verify`, `ls --verify` and `show` — and they name neither
revision: not the one history derives, not the one confirming produces. The
paragraph governs "a repair that offers a confirmation code" and enumerates both
cases in its own second sentence, so the MUST reaches all three screens. Its
stated reason — they are different numbers, and a repair is admitted rather than
silent — is exactly as true of a damaged projection, so scoping the sentence
down to the absent case would be weakening the contract to fit the code.

**Measured against `9fba620`: the damaged screens named no revision there
either.** The behaviour is unchanged; what changed is that this head added the
sentence that makes it non-conforming. The contract text landed without the
code, in the commit that wrote it.

Nothing is at risk — confirming any of the three produces the same restored
bytes and the same `rev 2` (`evidence/repair-confirm.txt`). It is a shipped
normative MUST the binary does not meet, on the majority of the paths it names,
and the fix is the shape four earlier rounds have already ruled on: put the
invariant in the operation, then enumerate every entry to that state.

*Subordinate, on the same screen:* the absent case's comparison row reads
`rev / stored (absent) / restore 1`, while the file the confirm writes holds
`rev 2`. The header line above it says so, so the screen is not wrong — but the
row labelled `restore`, under a screen whose stated job is to show what it will
write, carries a number that will not be written.

### F-2 [P3] A repair code retyped after the damage is gone still admits `object.repaired.v1`

A repair prepared against a damaged projection, with the damage then undone out
of band — restoring the file from git, the ordinary thing to do — and the code
the tool gave typed afterwards (`evidence/stale-repair.txt`):

```text
prepared: VU5HJL
the record is sound again: verify exit=0
what a prepare would say now:
  error: … verifies and is what its admitted history produced, so there is
         nothing to repair; ordinary changes go through the normal path
what retyping the code the first screen gave does:
  CONFIRMED  01a05e55-74  object.repaired.v1  rev 2
  rev 1 -> 2, events 1 -> 2
```

The eligibility question — *is anything damaged?* — is asked at prepare and not
again at confirm. The re-rendered screen diagnoses it correctly and then offers
the code four lines below the diagnosis, so one screen contradicts itself
(`evidence/f2-screen.txt`):

```text
Integrity  the stored record verifies and is what its admitted history produced, so there is nothing left to repair

(object.repaired.v1)

Type this exactly to confirm:  CONFIRM YA3DQE
```

Confirming writes back the bytes the projection already held, so nothing is
lost — but it puts a permanent `object.repaired.v1` in the record asserting a
repair of something that was not damaged, and bumps the revision.

**Measured at both heads: pre-existing** for the divergent and tampered states.
It is newly reachable at this head for the absent state, which `9fba620` refused
outright.

Round 21 ruled on this exact shape for migration — a spent code the screen was
still showing must not be the destructive path — and `migrate` answers it with
`COMPLETE … nothing was migrated`. The guards around it are otherwise sound: a
second prepare retires the first code, and any admitted mutation in between
retires the pending repair (`evidence/stale-repair-guard.txt`). Out-of-band
recovery is the one way the window stays open.

### F-3 [P4] A superseded comment left standing above its replacement, in the hunk round 33 wrote

`crates/engr/src/main.rs:2291` carries the same comment block twice. The first
copy is the one round 33 replaced, and it is the wording the round-33 ruling was
about:

```rust
// Absent is not unreadable, … Every Section below is
// what history alone says, and that is the whole of what will be
// written.
// Absent is not unreadable, … What they still need is the *other* side —
// a Section count is not something anybody can authorize, and this
// branch used to stop at one, …
```

*A comment describing the screen you meant to write is not the screen* was the
round-33 finding. The screen was fixed; the comment that described the screen
that had not been written was left standing above the one that replaced it.
Documentation only, no behavioural effect, and it is inside the four-file diff
the last review covered.

## Notes, not findings

- **`ls --verify` names a fault and still exits 0.** That is the shipped ruling,
  not a gap: *"`ls --verify` explicitly requests assessment; it reports
  discovered faults while keeping the survey's exit convention."*
- **The Event's review provenance carries no digest** — `attempts`, `outcome`,
  `result` and nothing else. That matches PROTOCOL.md, which says so in as many
  words and puts the ReviewDigest in the Challenge instead. The design draft's
  §12.4 `{outcome, digest}` is superseded on this point.
- **`candidate` has no machine-readable form.** `repair --json` carries the whole
  `restores` projection; `candidate <code>` is screen-only. Nothing requires
  otherwise.
- **The release profile still warns `method as_str is never used`**
  (`migration.rs`), at both heads; no matrix command runs the release profile.
- **`object.migrated.v1`'s payload member is still named nowhere in the shipped
  contract** — on disk it is `snapshot`, and `snapshot` appears 12 times in
  PROTOCOL.md, never for this. Carried forward from the fourth run.
- **A stage left holding only `published-over.json` does not wedge the
  workspace**, and **an Object path that is a directory** is refused by all four
  surfaces at exit 8, identically at both heads. Both carried forward from r6.

## What was done about them

All three are fixed at **`a2568f2`**, and each was re-observed against a release
build of both heads in the same probe (`evidence/fixed-probes.txt`):

| | `9d7bf06` | `a2568f2` |
|---|---|---|
| lines naming a revision: absent / divergent / tampered | 1 / 0 / 0 | 1 / 1 / 1 |
| a code retyped after the damage is gone, all three states | screen offers the code, `confirm` exit 0, events 1 → 2 | screen offers nothing and says `SETTLED`, `confirm` exit 5, events 1 → 1 |
| a repair that is still needed | works | works: `CONFIRMED … object.repaired.v1 rev 2`, verify exit 0 |

One further behaviour moved, measured rather than assumed
(`evidence/fix-delta.txt`), and it is the same shape as the P3: a repair prepared
while a projection was divergent and then made unreadable used to be offered a
code that confirmation refused at exit 4, and is now not offered one. The
`removed` and `unreplayable` variants of the same sequence are unchanged.

**`a2568f2` is a new head, so the acceptance on `9d7bf06` no longer applies and
neither does this run.** The gate is met only by a run against the head that
merges.

## Answer

**The gate is not met at `9d7bf06`.** One P2, one P3, one P4.

The P2 is not a behaviour regression — it is a contract regression: round 33
added a MUST to the shipped protocol and implemented it on one of the three
screens that MUST names. The P3 is pre-existing at both heads and newly reachable
on the state rounds 32 and 33 added. The P4 is a stale comment inside round 33's
own hunk.

Everything rounds 32 and 33 set out to fix is closed, and each was watched
failing at `9fba620` in the same probe. Nothing the migration writes moved.
Every data-safety attack this audit has accumulated was refused again, and
**eighteen** SIGKILL instants across the publication window all resume to the
same bytes.

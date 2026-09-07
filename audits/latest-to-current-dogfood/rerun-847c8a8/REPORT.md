# latest → current dogfood re-run, at `847c8a8`

**Destination under test:** `847c8a8` — the head PR #67 stands at, carrying
round 35. Code review has accepted this exact SHA and named a complete dogfood
run of it as the one remaining gap.
**Source:** `e7d9f99` — the released predecessor, unchanged.
**Comparison build:** `a2568f2` — the previously audited head (round 34).
**Failing control:** `9d7bf06` — round 33, kept so round 34's two findings have a
head that fails them.
**Previous runs:** `a77887b` (committed audit), `52fe116`, `ca6474a`, `b02d05e`,
`0086bb6`, `9fba620`, `9d7bf06`
([`rerun-9d7bf06/`](../rerun-9d7bf06/REPORT.md)).

A scenario is `PASS` only when the expected behaviour was **observed in a
transcript in this run**. Nothing is `PASS` because the implementation was read
and judged likely correct.

## Verdict

**No blocking finding against `847c8a8`.** Rounds 33, 34 and 35 are each closed
and each proved against a head that fails them. One non-blocking observation is
recorded below, extending the open F7; it is a consistency question for the
reviewer, not a correctness defect.

The substantive findings this run produced are against **the audit's own
harness**, and they matter because they change what some of r7's evidence was
worth. They are set out in their own section.

## The binaries, proved rather than named

Every head was exported fresh with `git archive` and built into **its own**
`CARGO_TARGET_DIR` volume. All four reproduced byte-for-byte the binary an
earlier round had already probe-proved, so the provenance table is a measurement
(`evidence/bin-inventory.txt`, `evidence/binary-provenance.txt`):

```text
                r31  r32  r33  r35   sha256[0:16]      head
engr-latest      0    0    0    0    d61aadba03cee2fc  e7d9f99
engr-prev        1    1    1    0    1493e2284a07fdcd  a2568f2
engr-current     1    1    1    1    e5827a653ae87e60  847c8a8
engr-r33         1    1    1    0    00260e3f542169f9  9d7bf06
```

`a2568f2` and `9d7bf06` are **not separable by protocol text** — round 34 changed
no wording — so they are separated behaviourally, in this run:

```text
divergent repair screen names the revision:  prev 1   r33 0   current 1
```

The handoff's provenance table recorded `engr-r34` as `1493fc2284a07fdcd`. The
value is `1493e2284a07fdcd`, confirmed twice over: by a fresh build of `a2568f2`
and by the binary round 34 left on disk.

## The input is the same input, for the eighth time

Restored from the pre-migration checkpoint; its inventory is **byte-for-byte
identical** to the record the committed audit carried (`evidence/fixture.txt`).
Every seal was then reproduced by an implementation that has never seen engr's
code — **14 of 14 MATCH**, no mismatch (`evidence/digest-premise.txt`), which is
the premise every forgery below rests on.

## What round 35 changed in the record: nothing

The post-migration inventory differs from the fifth, sixth and seventh runs in
exactly the three Event streams, which carry a fresh Event id and admission
instant per run by construction. `.gitignore`, `VERSION` and all three Objects
are identical bytes to all three (`evidence/post-migration-inventory.txt`,
`evidence/post-migration-checkpoint.txt`).

## Round 35, observed closed against the head that failed it

Round 35's finding: a repair appends its Event and *then* saves the projection.
A crash between those two writes leaves a durable `object.repaired.v1`, the
still-damaged projection and the pending code — and the screen offers a cleanup
retry that ordinary reconciliation then refuses.

The state is constructed without a timed kill: save the two files the interrupted
write would not yet have touched, confirm a real repair, restore only those two
(`harness/round35b.sh`).

| damage state | `847c8a8` | `a2568f2` |
|---|---|---|
| unsealed (tampered) projection | retype **finishes** it, identical to the uninterrupted repair; challenge cleared; `verify` 0 | retype **refuses**, exit 5; projection **STILL DAMAGED**; challenge **STILL PENDING**; `verify` 5 |
| resealed-divergent projection | finishes; cleared; `verify` 0 | refuses, exit 5; still damaged; still pending; `verify` 5 |
| absent projection (the control) | finishes; cleared; `verify` 0 | finishes; cleared; `verify` 0 |

The screen says `ALREADY APPLIED … Retype it to finish cleanup` at both heads. At
`a2568f2` that instruction does not work and the workspace stays wedged; at
`847c8a8` it does. (`evidence/round35-current.txt`, `evidence/round35-prev.txt`)

## Rounds 33 and 34, re-observed at this head with 9d7bf06 beside them

**F-1** — which repair screens name a revision (`evidence/fixed-probes.txt`,
`evidence/f1-confirm.txt`):

```text
             847c8a8   9d7bf06
absent          1         1
divergent       1         0
tampered        1         0
```

**F-2** — a repair code retyped after the damage was undone out of band
(`evidence/fixed-probes.txt`, `evidence/stale-repair.txt`, `evidence/f2-screen.txt`):

```text
              847c8a8                                9d7bf06
absent        no code offered, says SETTLED,         offers the code, exit 0,
              confirm exit 5, events 1 -> 1          events 1 -> 2
divergent     same                                   same
tampered      same                                   same
```

At `9d7bf06` each retype admits a spurious `object.repaired.v1`. At `847c8a8` the
screen says *nothing here needs confirming* and the confirm refuses at exit 5,
with the projection bytes IDENTICAL and the stream unchanged — measured at both
`847c8a8` and `a2568f2`, six cases, all six refused.

The guards around it hold (`evidence/stale-repair-guard.txt`): a second prepare
retires the first code (exit 3), and any ordinary admitted mutation between
prepare and confirm retires the pending repair (exit 3).

Round 33's screen still holds: the absent-projection repair prints every restored
member including both Sections' complete wording, and `candidate <code>`
re-renders it **IDENTICALLY** (`evidence/repair-screens.txt`). Confirming any of
the three damage states writes the same restored bytes, appends
`object.repaired.v1` at rev 2, and the workspace verifies
(`evidence/repair-confirm.txt`).

## The five-state matrix

`show`, `ls --verify`, `verify` and `show --format json` asked of one Object in
each integrity state, at both heads (`evidence/five-states.txt`). **Every cell is
identical at `847c8a8` and `a2568f2`:**

| state | verify | `ls --verify` | show | json `integrity` |
|---|---|---|---|---|
| ok | 0 | 0 | 0 | `ok` |
| one revision behind (the control) | 0 | 0 | 0 | `ok` |
| projection absent | 5 | 0, row named | 5 | `projection_missing` |
| tampered | 5 | 0, row named | 5 | `tampered` |
| divergent | 5 | 0, row named | 5 | `divergent` |
| unreplayable | 4 | 4 | 4 | — |

`ls --verify` naming a fault and still exiting 0 is the shipped ruling, not a gap.

The same six states asked of `ls`, `ls --sections`, `ls --verify`, `verify` and
`show` across the prepared damages, at both heads, agree throughout
(`evidence/damage-probes.txt`). The recovery `show` names — `engr repair
01a05e55-74` — accepts the state it is offered for (`evidence/show-missing.txt`).

## Data safety

| Attack | Result |
|---|---|
| SIGKILL swept across the publication window, **18 instants** from 500 to 1120 ms | every one resumes to an `objects/` **byte-identical** to an uninterrupted migration *and* to the migrated checkpoint this audit carried forward; reads fail closed while incomplete (exit 4, naming the recovery); second resume exit 3; `verify` PASSes on all 18 |
| Round 21: the spent code retyped against a workspace already at generation 1, after a day's work moved it on one admitted revision | landed at 1090 ms — `COMPLETE … already generation 1; nothing was migrated, and the spent migration's leftovers are retired`; record hash **IDENTICAL** either side; leftovers retired; `verify` 0 |
| Released build meeting a workspace inside the pre-barrier window | never reads a half-published generation-1 record: either the predecessor form (exit 0, predecessor record) or the barrier (exit 4, `not an engr workspace`) |
| Predecessor moved under a staged plan, before publication and after | refused, exit 5, naming the qualified response `engr confirm "CONFIRM <code> no"`; nothing published; the out-of-band edit intact |
| Interrupted withdrawal | reads describe the predecessor again; `migrate` mints a **fresh** code; the spent one refused at exit 3; an orphan stage fails closed at every surface |
| Overwrite warning across a second interruption | the next resume still names `objects/01a05e55-74….json`, at both heads |
| Round 25/5: a Ref admitted in a crash tail | still checked — `verify` FAILs naming it; `show` names the unreadable target at the same instant |
| Round 25/3: nested exclusion | `project\[1\]` escaped — `check-ignore` exits 0 and git sees nothing; the unescaped control exits 1 and lists the live code |
| 17 integrity and reference tampers | all refused — **12 at exit 4, 5 at exit 5**, no `NO-OP!` |
| 17 YAML profile probes | 2 must-load accepted, 15 refused, each naming its own reason |
| Wording flags on events that carry no wording | **32 of 32** refused at exit 2 across both heads, workspace unchanged |
| An Object path that is a directory | exit 8 at all four surfaces, both heads |

This run's uninterrupted confirm measured **1031 ms** (r7 saw 1079, 1148, 1133
and 1223 on the same host). Every instant was taken from that number, in this
run.

## Domains re-exercised end to end

Type and state across all three vocabularies with the invalid pairs refused
before the gate; supersession with self-supersession and cycles refused; Rules
with passing, failed-and-overridden and exhausted review, **six** review refusals
each firing for its own reason, plus artifact-exact Rule drift **and its round
trip** (one byte moves, the digest changes, the review is refused; the byte goes
back, the same digest returns and the confirm lands); Backlog
subjects/produced/merge/consume with the stale token refused at exit 6, the
exhaustion diagnostic persisted rather than only printed, the Work interlock and
the recovery it names; every `expect` message naming exactly the token it wants,
and the literal reading of that advice corrected by name; Work
items/results/commits/blockers/dependencies/pause/resume/rm; Collection
membership/order/priority/schedule/state with **both uniqueness rules genuinely
attacked** and the id grammar refused.

The final workspace verifies: four Objects PASS, every stream contiguous, git
tracking the record and never seeing `local/`
(`evidence/final-workspace-state.txt`).

## Observation, not a blocker: two spellings for the machine-readable form

Extends **F7**, which is already open and put to the reviewer. F7 was about
`--target` / `--on` / `--target --reason` across three domains. This is the same
class on a different axis (`evidence/machine-readable-flags.txt`):

```text
--format json    show, backlog show, work show, collection show
--json           repair, rules ls
(none)           ls, verify, candidate, protocol, backlog ls
```

Unlike F7 this one has a demonstrated cost inside this audit. `candidate` has no
machine-readable form at all, so r7's `repair-json.sh` had to parse the human
screen; it called plain `repair` for a code, then `repair --json`, which mints a
**second** code and retires the first, and then looked the first one up. Every
`candidate` call in that probe refused at exit 3 — a refusal for the wrong
reason — and the probe reported for three runs without anyone noticing.

Nothing is at risk and no contract is broken. It is put to the reviewer beside
F7.

## Findings against the audit's own harness

Ten classes of defect were found in the harness this run. They are listed because
several of them mean r7 reported on probes that measured nothing.

1. **A comparison probe whose two binaries were the same file.**
   `fixed-probes.sh` and `fix-delta.sh` compared `engr-fixed` against
   `engr-current`; at this head those two names hold identical bytes. Both now
   hash either side and refuse. The head that actually fails F-1 and F-2 is
   `9d7bf06`, which is why `engr-r33` was built.
2. **32 lines across 15 scripts reported the wrong exit code** — `$?` after a
   pipeline is `head`'s, `tail`'s or `sed`'s status, never engr's. `dep-g`'s
   repair refusal read `exit=0`; it is 5. All rewritten to capture engr's status
   before anything is piped.
3. **`collection.sh` carried r7's replacement-object id.** The replacement is
   created fresh every run, so the membership-uniqueness probes would have
   refused with `does not exist` for the second run running. It now resolves both
   targets out of the workspace, prints what it resolved, cross-checks against
   what `supersession.ps1` recorded, and refuses rather than measuring nothing.
4. **`repair-json.sh` looked up a retired code** — see the observation above.
5. **`rules.ps1`'s last probe used a mutation that was not available** (`--reopen`
   on an already-open Object), so it refused with `already open`. r7 recorded
   this and rebuilt the refusals in `rules-refusals.sh` but left the broken probe
   standing.
6. **`repair-missing.sh` depended on an ordering nobody wrote down** and this run
   copied a directory that did not exist; every surface answered `no .engr
   workspace at rx`.
7. **`work-empty-lists.sh` depended on a workspace r7 made by hand.** It builds
   its own now.
8. **`adv-verify.sh` ran before `adv-final.sh`, the script that builds its
   workspace** — so in r7 it can only have measured a leftover from an earlier
   run. Reordered and guarded.
9. **`damage-probes.sh` kept r7's known-wrong flag spelling.** r7 found that
   `--section 1 --delete` deletes nothing, rebuilt the control in `dep-g.sh` with
   `--delete 1`, and left the wrong spelling standing in the other script. That
   block prepared nothing, confirmed an empty code, and reported a verify exit
   for whatever `dep-g.sh` had left. It now requires the built control and checks
   it. *This is the audit's own version of the lesson five review rounds have
   taught: a fix applied to the reported site only.*
10. **A section count that was always one too high**, in four scripts:
    `grep -o '"id":[0-9]*'` also matches the Object's own `"id":"<uuid>"`, because
    `*` matches zero digits.

Every one of these was found the same way: by making a probe say what it did
rather than only what it concluded.

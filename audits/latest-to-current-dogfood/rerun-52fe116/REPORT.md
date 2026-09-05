# latest → current dogfood re-run, at the accepted head

**Destination under test:** `52fe116` — the head `lukeo3o1` accepted on PR #67
(`ACCEPT #67`, 2026-09-04 16:49Z), 16 commits after `a77887b`, the head the
committed audit was run against.
**Source:** `e7d9f99` — the released predecessor, unchanged.

The committed audit's own rule applies here without exception: a scenario is
`PASS` only when the expected behaviour was **observed in a transcript in this
run**. Nothing is `PASS` because the implementation was read and judged likely
correct. Where source was read at all, it was to explain an observation already
made, and it is marked as such.

## The input is provably the same input

The predecessor workspace was restored from the pre-migration checkpoint of the
committed audit, and its inventory is **byte-for-byte identical** to
`evidence/inventory-pre-migration.txt` on the `#68` branch — three Objects, seven
Sections, one pending Candidate `8TTSM6`, one dead `lock`. Every difference in
outcome below is therefore attributable to the 16 commits and to nothing else.

```text
predecessor inventory   identical to the committed audit's
engr-latest             engr latest (unknown)      e7d9f99
engr-current            engr latest (52fe1162)     rebuilt for this run
```

## What did not change, and that is the main result

**`objects/` after migration is byte-identical to the `a77887b` run.** The same
three hashes, from the same input, sixteen commits apart. The EventStore differs
only in Event ids, the admission instant, and the seals over them.

Two things the committed report listed as findings are **fixed**, confirmed by
observation rather than by reading the diff:

- **F4** — `migrate --help` says `to generation 1`; the phantom `v3` is gone.
- **F6** — no `.engr/lock` is left behind. The inventory diff against the
  `a77887b` run shows the file present there and absent here, and the sweep
  confirms it is removed on the cleanup path too.

## Data safety: nothing was found

Every path that could destroy admitted knowledge held, including under attacks
built for this run:

| Attack | Result |
|---|---|
| Round 21: retype the spent migration code against a workspace that has since reached rev 2 | Object and Event stream **byte-identical** before and after; stage swept, code retired |
| Correctly resealed out-of-band edit (independent JCS/SHA-256 forgery, every seal valid) | refused, exit 5, named exactly |
| Forged base under a crash tail — reconciliation must not replay over it | refused; the tail was **not** applied; the forgery stayed on disk as evidence |
| Mutation while the stored projection is behind its own history | reconciles first, then appends; sections 1, 4, 5 preserved, nothing lost |
| SIGKILL swept across the whole ~1050 ms publication window | every state resumes to a workspace whose `objects/` is byte-identical to an uninterrupted migration; exactly one Event per stream; second resume exit 3 |
| Predecessor history with a **purged prefix** (revs 3..4) | accepted; produces a byte-identical record |
| Predecessor history with a **gap** (revs 1, 2, 4) | refused, exit 4, naming `rev 4 does not immediately follow rev 2` |
| 15 integrity and reference tampers, 17 YAML profile probes | all refused, each naming its own reason |

The digest contract was reproduced by an **independent implementation** written
for this run (`harness/jcs.js`): SHA-256 over RFC 8785 JCS of each value minus
its own `digest` reproduces every stored Section and Object digest exactly. The
forgeries above were built with it, so "correctly sealed" means correctly sealed.

## The seven areas the committed report marked NOT RUN are now closed

| Area | Result |
|---|---|
| Predecessor history-prefix purge | PASS — accepted, byte-identical record; a gap is refused |
| Type/state lifecycle and `--classify` | PASS — all three vocabularies, attention derived correctly, invalid pairs refused |
| Supersession | PASS — role, relation and state fixed by the event type; self-supersession and cycles refused; untyped refused |
| Repair | PASS — accepts **both** damage kinds and enumerates every divergence, including a whole missing Section |
| Backlog consume / merge | PASS — a merged id is never reused; the topic file goes with its last point |
| Backlog Rule Review exhaustion | PASS on behaviour, **finding N6** on surfacing |
| Work pause | PASS — persisted `paused`, and the screen tells an agent not to resume it alone |

## Findings

None of these lost or corrupted a byte. All are about what a surface **says**.

### N1 [P1] `verify`, `show` and `repair` give three different verdicts on one state

A stored projection that is *behind* its own admitted Event stream — the exact
signature of a crash between the two renames of one admission — is a state the
tool recovers from correctly. Three read surfaces describe it three ways:

```text
verify    FAIL, exit 5   "3 events are not reflected in the sections"
ls --all  ok,   exit 0   displays the replayed state, not what is stored
show      ok,   exit 0   and silently rewrites the Object file on disk
repair    exit 5         "verifies and is what its admitted history produced,
                          so there is nothing to repair"
verify    PASS, exit 0   after that show
```

Measured with file hashes at every step (`transcripts/10-rewind-probe.txt`): the
stored file is `8bc54cbe…` before, unchanged after `verify` and after `ls --all`,
and `e27820f6…` after `show` — byte-identical to the healthy workspace.

Three separate problems sit in that loop:

- **A failing verification is cured by looking at the object.** An agent or a CI
  gate that runs `verify` gets exit 5; anything that then runs `show` makes the
  next `verify` pass, with nothing in between saying why.
- **`repair` contradicts `verify` in the same breath.** `verify` says the
  sections do not reflect the history; `repair`, the documented recovery, says
  they do. Neither screen says they are evaluating different things — `verify`
  the stored bytes, `repair` the reconciled state.
- **A read command writes the authority file and says nothing.** Whatever the
  merits of reconciling forward, `show` is the surface that persists it, prints
  no line about having done so, and leaves the working tree dirty.

The recovery itself is correct and safe, and refuses to run over a tampered base
(verified separately, `transcripts/11-forged-crash-tail.txt`). The defect is that
nothing tells the reader which of the four answers is the true one.

### N2 [P2] The migrated `.gitignore` documents the predecessor's layout

`engr init` writes the generation-1 text. Migration keeps the predecessor's file
and appends `/local/`, so a migrated workspace carries, tracked in git:

```gitignore
# ... events/ is safe to commit too ...
#   lock         a mutex for this machine, nothing to share
#   candidates/  each file is named after a *live* challenge code
/lock
/candidates/
/local/
```

`events/`, `lock` and `candidates/` do not exist in generation 1. Two workspaces
of one generation, written by one binary, disagree about the workspace's own
shape, and the migrated one is wrong. Pre-existing: the `.gitignore` hash is
identical to the `a77887b` run's, so the committed audit missed it.

### N3 [P2] The completed-transaction cleanup reports itself as a migration

In the documented crash state where `VERSION` is written but the stage has not
been swept, retyping the code prints:

```text
MIGRATED   .../.engr  3 objects, 7 sections, generation 1
```

Nothing was migrated — the transaction was already complete, and this run only
proved that and cleaned up. The counts are the **plan's**, not the workspace's:
it held 8 sections at that moment, because a rev-2 mutation had been admitted
since. This is the surface half of round 21's bug. The destructive half is fixed;
the screen that made retyping look like the right move still says the same thing.

### N4 [P2] `repair` is the one command that will not take an id prefix

```text
show 01a05e55-e6              works
verify 01a05e55-e6            works
prepare --object 01a05e55-e6  works ("Any unique id prefix")
repair 01a05e55-e6            error: object id "01a05e55-e6" is not a UUID   exit 4
```

`repair`'s `<OBJECT>` argument has **no help text at all**, so nothing says so —
and this is the recovery path, reached only when something is already wrong and
named explicitly by the refusals that send a user there.

### N5 [P2] `change_state` cannot move a typed object's state

`PROTOCOL.md`'s normative command table:

```text
| change_state | object.state_changed.v1 | state | Moves the object's state within its type's lifecycle |
```

Observed: every state move on a typed Object goes through `--classify` and emits
`object.classified.v1`. The only CLI spellings of `change_state` are `--close`
and `--reopen`, and the protocol says both are untyped-only and MUST be refused
on a typed object — which they are, exit 5. (Confirmed in source afterwards:
`Chosen::ChangeState` hardcodes the state to `Closed` or `Open`.) So the table
describes, as a normative contract compiled into the binary, something no command
can do. Either the description is wrong or a route is missing.

### N6 [P2] Backlog review exhaustion is recorded and never surfaced

Driving a Backlog mutation past `audit-scope`'s `max_attempts: 3` persists
`review_exhaustion: {attempts: 4, limit: 3}` exactly as the contract requires,
and a later successful mutation clears it. But the mutation that triggered it
printed `revised §2`, exit 0, and said nothing; `backlog show` (text) and
`backlog ls` never mention it. Only `--format json` carries it. The Object domain
announces the same condition loudly — "EXHAUSTED at attempt 4 — confirming this
admits work no passing review allowed". The Rule's `on_exhaustion:
human_confirmation` had no visible effect on the Backlog path.

### N7 [P3] The migration screen never says this workspace has a pending Candidate

The confirmation screen says `7 predecessor files will be read, converted, and
replaced`, and `Any pending candidate is not migrated and must be prepared again`
— a conditional sentence. This workspace had one, `candidates/8TTSM6.json`. It is
unrecoverable work, it is gone, and it is not among the 7 files the screen counts
or the 7 entries the frozen subject records.

### N8 [P3] One idea, four spellings

Observed in this run: `backlog produced --target`, `work depend --on`,
`collection add --target`, `collection schedule --target-date`, and `repair`'s
bare positional where `prepare` uses `--object`. This is the committed report's
F7, now with the fourth case.

## Prior findings that still stand

- **F1** — reproduced exactly. With a **selected** Ref field changed on the
  target: `verify` PASS exit 0 (whole workspace and single object), `show` says
  `refs moved` and names `based_on, text`, `ls` says `1 stale`. `verify --help`
  still promises "Object and Section integrity plus dependencies". The one
  surface that claims to check dependencies is the one that does not report them.
- **F3** — `--expect`'s help and refusal both call it "its expect value"; it is
  an object of per-operation tokens (`rename`, `add`) plus one per section.
- **F5** — the predecessor refusal says the workspace "is read-only here … before
  mutation", then refuses `ls`, `show` and `verify`, which are reads.
- **F2** was fixed in round 16 and is confirmed: the help and the refusal now say
  `--format json`.

## Answer

**The record's integrity is sound.** Every attack built for this run — including
one reconstructed from round 21's own description, and a forgery built with an
independent implementation of the digest contract — was refused or recovered from
without losing a byte, and a migration interrupted anywhere in its publication
window produces the same record as one that was not.

**The gate as written is not met.** The re-run did not come back finding nothing.
Nothing found is a data-safety defect. N1 is the one most likely to make an agent
confidently wrong, and it is the same shape as F1: a surface answering a question
about integrity without saying which integrity it means.

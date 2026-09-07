# Runbook — re-run at `847c8a8`

The eighth audited head, and the first run whose purpose is to find nothing:
code review accepted `847c8a823325e01f1b764ecd9fdd9899070379ac` exactly and
named the missing dogfood as the only remaining gap. Method as in the parent
runs and in [`../rerun-9d7bf06/RUNBOOK.md`](../rerun-9d7bf06/RUNBOOK.md); this
records only what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         847c8a823325e01f1b764ecd9fdd9899070379ac   (PR #67, round 35)
comparison build    a2568f2129b03f1c84da89e2ad8f83108f8fd094   (round 34, previously audited)
failing control     9d7bf065427d7fb5046fbf4db8555547ea6b27fd   (round 33, fails F-1 and F-2)
container image     engr-rust:latest
```

## The binaries, and why there are four

All three heads were exported fresh with `git archive <sha>` into `src-<sha>/`
and built into **their own** `CARGO_TARGET_DIR` volume. Every one reproduced
byte-for-byte the binary the previous round had already probe-proved, which is
what makes the provenance table below evidence rather than bookkeeping:

```text
engr-latest    r31=0 r32=0 r33=0 r35=0  d61aadba03cee2fc   e7d9f99
engr-prev      r31=1 r32=1 r33=1 r35=0  1493e2284a07fdcd   a2568f2
engr-current   r31=1 r32=1 r33=1 r35=1  e5827a653ae87e60   847c8a8
engr-r33       r31=1 r32=1 r33=1 r35=0  00260e3f542169f9   9d7bf06
```

`engr-r33` is new to this run. Round 34's two findings (F-1, a repair screen
naming no revision on the divergent and tampered projections; F-2, a code
retyped after the damage was undone out of band) were **fixed in a2568f2**, so
`engr-prev` passes them. Re-asking them at `engr-current` with `engr-prev`
beside it would have compared two passing heads and reported green while
measuring nothing. The head that fails them is 9d7bf06, so it is kept.

The same hazard was live in the harness itself: `fixed-probes.sh` and
`fix-delta.sh` named `engr-fixed` against `engr-current`, and in this run those
two filenames hold **the same bytes**. Both now hash either side and refuse:

```sh
if [ "$(sha256sum "$F" | cut -d' ' -f1)" = "$(sha256sum "$C" | cut -d' ' -f1)" ]; then
  echo "REFUSING: \$F and \$C are the same binary -- this probe would measure nothing"
  exit 1
fi
```

The handoff's own provenance table recorded `engr-r34` as `1493fc2284a07fdcd`;
the value is `1493e2284a07fdcd`, confirmed twice over — by a fresh build of
a2568f2 and by the binary round 34 left on disk.

Note that **`a2568f2` and `9d7bf06` are indistinguishable by protocol text**,
because round 34 changed no protocol wording. For this run that does not bite —
the round-35 paragraph separates `847c8a8` from both — but `binary-provenance.sh`
now ends with the behavioural probe that does separate them: a repair screen on
a **divergent** projection names a revision at a2568f2 and names none at
9d7bf06.

## Hardcoded premises, and which of them actually move

`refmap.sh` is new. It prints every object's compact reference and then checks
the ones the harness hardcodes against the running workspace, because r7 lost
two probes to ids that had gone stale without saying so.

```text
01a05e55-74  engr:obj:01m1f5ax7bedrbew8012yq3ma9   migrated -- stable across runs
01a05e55-e6  engr:obj:01m1f5bshbeq1rpckc726yrk2b   migrated -- stable across runs
01a05e55-ee  engr:obj:01m1f5bvj6fa8sgfztf6s9fc8c   migrated -- stable across runs
01a07821-8e  engr:obj:01m1w233hmetgbejpgdceakxet   r7's replacement -- MOVES
```

Only the replacement object moves, and `collection.sh` was the one script
carrying it. It no longer hardcodes anything: it resolves both targets out of
`ls --all` and `show --format json`, prints what it resolved, cross-checks
against what `supersession.ps1` recorded, and **refuses** rather than running
the membership-uniqueness probes against an id that does not exist. The three
remaining hardcodes name `01a05e55-74`, a migrated object; `refmap.sh` asserts
that rather than trusting it.

## The pre-confirm checkpoint

Minted with the head under test, not carried forward — a Challenge carries the
generator fingerprint of the binary that made it. This run's code is `XDKEFZ`
(r7's was `KR824S`), and the fifteen scripts that hardcode it were repointed.
`.git`, `README.md` and `harness/` are kept in the checkpoint: the
git-visibility and nested-exclusion probes need them.

## The fixture

Restored from `checkpoints/pre-migration`, never rebuilt, and its inventory
diffed against the record the committed audit carried: `IDENTICAL`, ten files,
including the pending `8TTSM6` candidate the predecessor already had.
`post-inventory.sh` now diffs the migrated result against **both** r5 (the
established baseline) and r7 (the previous run), so "unchanged" is asserted
against two points rather than one.

## Every timing constant still moves with the host and the day

Take every crash instant from the number `crash-sweep.sh` prints at the top of
*that* run. `crash8.ps1` runs sweep, resume and the byte-identity check with one
instant list, because each step reads what the one before it made.

## What this run changed in the harness

Ten classes of defect, listed in `REPORT.md`. The mechanical ones are worth
naming here because they are easy to reintroduce:

- **`$?` after a pipeline is not engr's exit code.** 32 lines across 15 scripts
  read `head`'s, `tail`'s or `sed`'s status. The shape that works:

  ```sh
  __out=$(engr ... 2>&1); __rc=$?
  printf '%s\n' "$__out" | head -3; echo "exit=$__rc"
  ```

  A quick check for regressions, run from `harness/`:

  ```sh
  grep -rn '| *\(head\|tail\) *-[0-9]*; *echo "[^"]*exit=\$?"' *.sh
  ```

- **`grep -o '"id":[0-9]*'` counts one section too many**, because `*` matches
  zero digits and the Object's own `"id":"<uuid>"` matches. Use
  `'"id":[0-9][0-9]*'`.

- **A probe that names two binaries must hash them.** `engr-fixed` and
  `engr-current` are the same file this run.

- **A probe that needs a workspace must refuse when it is missing**, rather than
  letting every surface answer `no .engr workspace at …`. Four scripts silently
  depended on an ordering nobody had written down; `adv-verify.sh` was ordered
  *before* the script that builds its workspace, so it can only ever have
  measured a leftover.

- **Resolve ids, do not carry them.** Only the replacement object moves between
  runs; `refmap.sh` prints every object's compact reference and asserts the ones
  the harness still hardcodes.

## New scripts

```text
refmap.sh                 every object's compact reference, and an assertion on
                          the ones the harness hardcodes
machine-readable-flags.sh which commands take --json, which take --format json,
                          and which have no machine-readable form
work-rm-message.sh        whether `work rm` can be told apart from a no-op
                          (it can: exit 0 against real memory, exit 3 against none)
cleanup-probe-topic.sh    consume a probe's topic through the tool, not by
                          deleting the file
```

## Run order

```text
fixture -> encounter -> migrate -> post-migration -> post-inventory
  -> post-checkpoint -> refmap -> digest-premise
  -> lifecycle -> supersession -> install Rules -> rules -> rules-refusals
  -> rule-drift -> backlog -> backlog-rest -> work -> work-empty-lists
  -> expect-setup -> expect-messages -> collection
  -> five-states-prep -> five-states -> repair family
  -> host-side preps -> dep-g -> damage-probes
  -> crash8 (sweep, resume, identity)
  -> batch8 -> the remaining probes -> final-state
```

`dep-g.sh` must run before `damage-probes.sh`, `assessment-states.sh` before
`repair-missing.sh` and `show-missing.sh`, `adv-final.sh` before `adv-verify.sh`,
and `crash-sweep` before anything reading `crash-runs/`. Each of those is now a
refusal rather than a convention.

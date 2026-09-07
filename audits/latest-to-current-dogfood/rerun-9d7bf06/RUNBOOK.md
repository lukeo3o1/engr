# Runbook — re-run at `9d7bf06`

The seventh audited head. Method as in the parent runs and in
[`../rerun-9fba620/RUNBOOK.md`](../rerun-9fba620/RUNBOOK.md); this records only
what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         9d7bf06…   (PR #67, rounds 32 and 33)
comparison build    9fba620…   (the previously audited head)
container image     engr-rust:latest
```

Both heads were exported with `git archive` into `src-<sha>/` and built into
**their own** `CARGO_TARGET_DIR` volume (`build7.ps1`), then the binary's
sha256 was compared against the one still in the volume. All three archived
trees report `engr latest (unknown)`, so each was then proved to be the head
intended by asking it a question only that head answers — PROTOCOL.md is
compiled in, and every round adds a paragraph:

```bash
engr protocol | grep -c 'a repair that offers a confirmation code'          # 33
engr protocol | grep -c 'Every surface that classifies an Object owes the'  # 32
engr protocol | grep -c 'What navigation gives up'                          # 31
```

`harness/binary-provenance.sh` asks all three of every binary at once, so the
provenance is one file rather than three recollections.

## Mint the pre-confirm checkpoint, do not carry it forward

`checkpoints/pre-confirm` is the predecessor fixture with a **pending** migration
Challenge, and fifteen harness scripts hardcode its code. Earlier runs inherited
r6's `J8SUZ4`. Don't: a Challenge carries the generator fingerprint of the binary
that minted it, so carrying one forward would make every crash and barrier probe
run against a Challenge this head may refuse as incompatible — and they would
fail for that reason instead of measuring what they are about.

`harness/make-preconfirm.sh` mints one with the head under test and writes the
code to `evidence/preconfirm-code.txt`; then

```bash
sed -i 's/<old code>/<new code>/g' harness/*.sh
```

This run's code is `KR824S`. Keep `.git`, `README.md` and `harness/` in the
checkpoint — the git-visibility and nested-exclusion probes need them.

## Every timing constant moved again

The uninterrupted confirm measured 1079, 1148, 1133 and 1223 ms on four
consecutive runs on this host, within one session. Take every crash instant from
the number `crash-sweep.sh` prints at the top of *that* run, and prefer probes
that sweep for their own instant over probes that pin one:

- `harness/overwrite-report.sh` pinned its staging kill at 960 ms, which found
  nothing here — publication began nearer 1000 ms. It now sweeps
  `0.960 … 1.100` for the first kill that leaves a staged destination with at
  least one Event stream published, and prints which one landed.
- `round21.sh` already sweeps and reports the instant it found: 1075 ms.

The full window this run: staging ~850 ms, first Event stream ~1020 ms, all
three ~1020 ms, `format.json` removed ~1020–1085 ms, `VERSION` written
~1060 ms, Challenge retired just after.

## Run the crash sequence as one script

`crash-sweep.sh` rebuilds `crash-runs/`, and `crash-resume.sh` and
`crash-objects-identical.sh` both read what it made. Running them separately —
or running the sweep twice with different instants — leaves the identity check
reporting on whichever subset survived. `crash7.ps1` runs all three with one
instant list; this run swept 18 instants from 500 to 1120 ms.

## What this run added to the harness

- `harness/five-states-prep.sh` + `harness/five-states.sh` — one workspace per
  Object integrity state (ok, behind, absent, tampered, divergent,
  unreplayable), asked of `verify`, `ls --verify`, `show` and
  `show --format json` at both heads. Round 32 re-pointed the JSON `integrity`
  member at `view::object_fault`, which is a new consumer for four answers that
  already existed, so all five are measured rather than only the new one.
- `harness/repair-screens.sh` — all three repair screens whole and unabridged,
  plus the `candidate <code>` re-render diffed against the first screen, plus a
  count of how many lines on each screen name a revision.
- `harness/repair-confirm.sh` — each repair confirmed end to end: the file, its
  bytes against the record the audit carried forward, the resulting revision,
  the Event appended, and whether the workspace verifies afterwards.
- `harness/repair-edges.sh` — the repair screen rendered in states the ordinary
  path does not reach: an Object with no Sections, a projection restored while
  the code is pending, history that stops replaying while the code is pending.
- `harness/stale-repair.sh` and `harness/stale-repair-guard.sh` — F-2, and the
  guards around it that do hold.
- `harness/damage-probes.sh` — every prepared damage (`dep-b…dep-g`, `ev`,
  `tail`) asked of every surface at both heads, and what `repair` says about each.
- `harness/dep-g.sh` — round 29's third control, rebuilt.
- `harness/rules.ps1`, `harness/rules-refusals.sh` — the three review outcomes,
  and four refusals each on a mutation the subject could really have taken.
- `harness/expect-setup.sh` — a fresh topic for the `expect` message probe.
- `harness/tail-listing-probe.sh` — the behind-projection control across
  surfaces. Note it contaminates itself: the first assessment surface
  *reconciles* the projection, so the second binary sees a caught-up workspace.
  Read the control out of `five-states.txt` instead.
- `harness/binary-provenance.sh`, `harness/post-checkpoint.sh`,
  `harness/make-preconfirm.sh`, `harness/repair-json.sh`.

## Premises that were guesses, and were wrong

Two probes ran green while measuring nothing, and both were caught by making the
probe say what it did rather than only what it concluded:

- `collection.sh` hardcoded the compact id of a *previous run's* replacement
  object. The membership-uniqueness probes refused with `does not exist` —
  a refusal for the wrong reason, which hides whatever the probe was for. Repoint
  `A=` at this run's object, seen in the supersession transcript.
- The first `dep-g` attempt spelled the delete flag `--section 1 --delete`
  instead of `--delete 1`; nothing was deleted, and every surface then answered
  correctly about an untouched workspace. Any probe that mutates should hash
  either side and refuse to report on a `NO-OP!`.

`rules.ps1`'s last probe hit `already open` — the object had been reopened by an
earlier step — which is the same class. The review refusals were rebuilt in
`rules-refusals.sh` against `--add`, which is always available.

## Harness notes carried forward, still true

- **Docker from PowerShell, never from the Bash tool** — Git Bash rewrites
  `/audit/...` into a Windows path.
- **Pass scripts, not `sh -c` strings.** A multi-line shell script quoted
  through PowerShell into `docker run` dies on its own quoting; write it into
  `harness/` and run `sh /audit/r7/harness/<name>.sh` (`s7.ps1` does this, and
  passes trailing arguments through).
- **`Start-Job` does not survive between tool calls** — each PowerShell
  invocation is its own session. Background a whole `.ps1`, not a job inside one.
- **`$ErrorActionPreference = 'Stop'` plus a native command that writes to
  stderr is a terminating error.** `docker volume rm` on a volume that does not
  exist killed the build script. Use `-f`, and check `$LASTEXITCODE` yourself.
- **`node` is on the host, not in the image.**
- **PowerShell variables are case-insensitive.**
- **`$?` after a pipeline is `head`'s exit code, not engr's.**
- **Restore the fixture, do not rebuild it.**
- **Install the audit's Rules after the lifecycle and supersession exercises.**
  Order: migrate → lifecycle → supersession → install Rules → Rules exercises →
  Backlog → Work → Collection.
- **Implement the digest contract independently** and check it reproduces every
  stored digest before building any forgery on it.
- **A scenario is PASS only when its expected behaviour was observed in a
  transcript in that run.**

## Layout

```text
rerun-9d7bf06/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    argv + stdout + stderr + exit code, verbatim; 01 is the
                  fixture script, captured whole
  evidence/       binary provenance, inventories, the digest premise, the crash
                  sweep/resume/identity, the barrier window and switch, the
                  adversarial suites, the five-state matrix, the repair screens
                  and their confirmations, and every domain exercise
  harness/        every script this run used
```

Line endings are normalized to LF; trailing whitespace inside lines is engr's own
column padding and is left alone, so `git diff --check` reports it. The `??` and
`禮` sequences in `transcripts/` are the Windows console codepage mangling `—`
and `§` on the way through PowerShell; `evidence/` files are written straight to
disk by the container and render correctly.

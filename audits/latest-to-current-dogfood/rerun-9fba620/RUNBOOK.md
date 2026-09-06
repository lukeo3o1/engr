# Runbook — re-run at `9fba620`

The sixth audited head. Method as in the parent runs and in
[`../rerun-0086bb6/RUNBOOK.md`](../rerun-0086bb6/RUNBOOK.md); this records only
what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         9fba620…   (PR #67, answering review 5124757951)
comparison build    0086bb6…   (the previously audited head)
container image     engr-rust:latest
```

Each head was built into **its own** `CARGO_TARGET_DIR` volume, and each binary
was then proved to be the head intended:

```bash
engr protocol | grep -c 'What navigation gives up'   # 0 at 0086bb6, 1 at 9fba620
```

Both archived trees report `engr latest (unknown)`, so the version string cannot
do this. Pick the probe from the diff between the two candidate heads before you
build.

## Reproducing a debug-only stop with a release binary

Round 31's fourth finding is about an interruption *between* the overwrite and
the report. The reviewer constructed it with `stop_for_test`, which is
`#[cfg(debug_assertions)]` and does not exist in the binary this audit runs.

`harness/overwrite-report.sh` does it with a real `SIGKILL` instead:

1. kill a fresh confirm at 960 ms — staged, barrier up, at least one Event stream
   published, so publication has demonstrably begun;
2. move a predecessor Object out of band;
3. **time an uninterrupted resume** (381 ms on this host — a resume is much
   shorter than a first confirm, so the first-confirm instants are useless here);
4. sweep the kill across that window and keep the first run where the moved
   source has been overwritten;
5. resume again, and ask what it says.

At `0086bb6` the second resume prints `MIGRATED` and nothing else; at `9fba620`
it names the file. The probe prints which instant landed, so the run is
reproducible from its own output.

## This run's publication window

Every timing constant moves with the host and the day. `crash-sweep.sh` prints
the uninterrupted confirm first; take everything else from that number.

```text
uninterrupted confirm   1116 ms
staging begins          ~850 ms
first Event stream      ~900 ms
barrier installed       ~910–930 ms
all three streams       ~1000 ms
format.json removed     ~1050 ms
VERSION written         ~1090–1120 ms
```

Round 21's window — `VERSION` written, Challenge still on disk — was **narrow**
this run: it landed at 1090 ms, and by 1120 ms the sweep had already retired the
Challenge and the stage. Sweep downward from `VERSION` rather than upward.

## Install the Rules *after* the ungoverned exercises

The audit's own `audit-scope` and `evidence-discipline` govern the `object`
domain. Installing them before the lifecycle and supersession exercises makes
every one of those mutations demand a review, and the plain gate driver
(`g6.ps1`) does not supply one — the result is twelve refusals that say nothing
about the tool.

Order: migrate → lifecycle → supersession → **install the Rules** → the Rules
exercises → Backlog → Work → Collection.

## What this run added to the harness

- `harness/assessment-states.sh` — one workspace with two Objects that differ
  only in how far their projection has fallen behind: one removed, one rewound a
  revision. The second is the control that stops "reported" from meaning
  "reported as anything".
- `harness/content-flags.sh` — four actions that carry no wording against the
  four flags that build Content, run through **both** binaries, with the
  workspace hashed either side so a Challenge minted by the old one shows up as
  a change rather than being inferred from the screen.
- `harness/overwrite-report.sh` — above.
- `harness/new-surfaces.sh` — the two states the round-31 repairs newly created:
  a stage holding only `published-over.json`, and an Object path that exists and
  is not a regular file.

## Harness notes carried forward, still true

- **Docker from PowerShell, never from the Bash tool** — Git Bash rewrites
  `/audit/...` into a Windows path.
- **`node` is on the host, not in the image.**
- **PowerShell variables are case-insensitive.** A loop written
  `foreach ($a in …)` overwrote `$A`, the audit path, and every call in the body
  went to a command built out of the loop item. Name loop variables so they
  cannot collide.
- **`$?` after a pipeline is `head`'s exit code, not engr's.**
- **`sed -n 's/.*CONFIRM \(…\)/\1/p'` needs `| head -1`** — `tail-and-purge.sh`
  still has the unfixed version and its first probe dies on it.
- **Restore the fixture, do not rebuild it.**

## Layout

```text
rerun-9fba620/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    argv + stdout + stderr + exit code, verbatim; 01 is the
                  fixture script, captured whole
  evidence/       inventories, the digest premise, the crash sweep and resume,
                  the barrier window and switch, the adversarial suites, the
                  round-31 probes, and every domain exercise
  harness/        every script this run used
```

Line endings are normalized to LF; trailing whitespace inside lines is engr's own
column padding and is left alone, so `git diff --check` reports it. The `??` and
`禮` sequences in `transcripts/` are the Windows console codepage mangling `—`
and `§` on the way through PowerShell; `evidence/` files are written straight to
disk by the container and render correctly.

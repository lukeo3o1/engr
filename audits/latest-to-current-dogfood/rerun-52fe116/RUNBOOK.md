# Runbook — re-run at `52fe116`

How this run was produced. It differs from the parent `../RUNBOOK.md` in one
deliberate way: the predecessor fixture was **not** rebuilt.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         52fe116261a0c913160202c3bb32f8fe07827f07   (PR #67, accepted head)
container image     engr-rust:latest
```

## 1. Why the fixture was restored rather than rebuilt

The question this run answers is what the 16 commits between `a77887b` and
`52fe116` changed. Rebuilding the fixture would have introduced new UUIDs, new
timestamps and new commits, and every difference in the output would then have
had two possible causes.

So the pre-migration checkpoint of the parent run was copied in whole, and its
inventory was diffed against `../evidence/inventory-pre-migration.txt`:

```text
IDENTICAL: the predecessor input is byte-for-byte the one the committed audit used
```

Anything downstream that differs is therefore attributable to the destination
build alone. `bin/engr-latest` was reused unchanged; `bin/engr-current` was
rebuilt from `52fe116` and reports `engr latest (52fe1162)`.

## 2. Harness

`harness/t2.ps1`, `g2.ps1` and `gg2.ps1` are the parent run's `t.ps1`, `gate.ps1`
and `gate2.ps1` with transcripts redirected and one fix: `gg2.ps1` takes `-Rules`
explicitly, because PowerShell binds the first unnamed positional argument to the
next unbound positional parameter, which silently ate the subcommand.

The `.sh` scripts are the parent run's, repathed to the run directory and to a
clean migrated workspace produced by the crash sweep's own baseline.

## 3. What is new here, and how to reproduce it

### The digest contract, checked independently

`harness/jcs.js` implements RFC 8785 JCS plus SHA-256 from scratch and recomputes
every stored Section and Object digest. Run it against any Object file; it must
print `MATCH` on every line. This is the premise check that licenses everything
below it — a forgery is only interesting if it really is sealed.

```bash
node harness/jcs.js <workspace>/.engr/objects/<id>.json
```

`harness/reseal.js` uses it to rewrite a Section's text and reseal the Section and
the Object correctly:

```bash
node harness/reseal.js <object.json> <section-id> "<new text>"
```

### The three damage kinds

```text
A  broken seal          edit the text, do not reseal
B  rewound projection   git show <migrated-commit>:<object.json> over the current file
C  resealed forgery     reseal.js — every seal valid, content changed
```

B is a *legitimate crash tail*: the projection is exactly what an earlier Event
produced, and the remaining Events replay onto it. That is why the tool
reconciles it forward, and why finding N1 is about the surfaces disagreeing
rather than about the recovery being wrong.

The measurement that isolates N1 is a file hash between every command, not the
command output:

```bash
sha256sum <object.json>   # before
engr-current verify       # FAIL exit 5   — hash unchanged
engr-current ls --all     # ok   exit 0   — hash unchanged
engr-current show <id>    # ok   exit 0   — hash is now the healthy workspace's
engr-current verify       # PASS exit 0
```

### The round-21 attack

The state round 21 describes cannot be reached by the sweep alone, because the
Challenge is removed before the stage is swept. Build it:

1. sweep until `VERSION=yes  staged_dest=yes` (around 1050 ms here);
2. copy the migration Challenge back from a pre-confirm checkpoint;
3. admit a real mutation, so one Object reaches rev 2;
4. retype `CONFIRM <code>`.

The Object file and its Event stream must be byte-identical before and after.
They are. The screen is finding N3.

### The predecessor history-prefix purge

```bash
tail -n +3 .engr/events/<id>.jsonl    # legal purged prefix  -> accepted
sed -n '1,2p;4p' .engr/events/<id>.jsonl   # a gap            -> refused, exit 4
```

## 4. Crash sweep

`harness/crash-sweep.sh <ms>...` and `harness/crash-resume.sh <ms>...` take the
instants to kill at as arguments, because the publication window moves between
runs on this host — it measured 1045–1134 ms across the runs recorded here. Time
one uninterrupted confirm first; the sweep prints it.

## 5. Layout

```text
rerun-52fe116/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    22 logs, argv + stdout + stderr + exit code, verbatim
  evidence/       inventories, crash sweeps, adversarial suites, the damage matrix
  harness/        every script this run used
```

The `??` and `禮` sequences in the transcripts are the Windows console codepage
mangling `—` and `§` on the way through the harness, as in the parent run. engr's
own output is UTF-8; `evidence/` files written straight to disk render correctly.

Two disclosed transformations were applied to the captured files before commit,
and nothing else: CRLF was converted to LF, and the trailing blank line the
harness appends after the last entry was dropped. Trailing whitespace **within**
lines is left alone — much of it is engr's own column padding, and stripping it
would edit the evidence. `git diff --check` therefore reports it, as it already
does for the parent run's `transcripts/06-domains.txt`.

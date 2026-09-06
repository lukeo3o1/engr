# Runbook — re-run at `0086bb6`

The fifth audited head. Method as in [`../rerun-52fe116/RUNBOOK.md`](../rerun-52fe116/RUNBOOK.md),
[`../rerun-ca6474a/RUNBOOK.md`](../rerun-ca6474a/RUNBOOK.md) and
[`../rerun-b02d05e/RUNBOOK.md`](../rerun-b02d05e/RUNBOOK.md); this records only
what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         0086bb6…   (PR #67, after d3be38a's four fixes)
comparison build    b02d05e…   (the previously audited head)
container image     engr-rust:latest
```

## Check the comparison binary is the head you asked for

The fourth run built the comparison head into a named volume (`engr-cargo-prev`).
Reusing that volume for a *different* archived source tree finished in **0.25 s**
and copied out the previous run's binary at exit 0 — the build was a no-op and
nothing said so.

```bash
docker volume rm engr-cargo-prev5           # a fresh target dir per comparison head
docker run --rm -v <audit>:/audit -v engr-cargo-prev5:/target \
  -v engr-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/target \
  -w /audit/src-b02d05e engr-rust:latest \
  bash -c "cargo build --release -p engr && cp /target/release/engr /audit/bin/engr-prev"
```

Then ask the binary something only the intended head answers differently. Both
heads report `engr latest (unknown)` — the archived tree has no `.git` — so the
version string cannot do it. The compiled protocol can:

```bash
engr-prev protocol | grep -c 'omitted when empty'    # 2 at ca6474a, 7 at b02d05e
```

Pick that probe from the diff between the two candidate heads before you build.

## What this run had to build for itself

The harness is `../rerun-b02d05e/harness/` repathed from `r4` to `r5`, with three
kinds of change.

**Ids are discovered, not pinned.** `backlog-rest.sh`, `work.sh`,
`collection.sh` and `expect-messages.sh` carried the fourth run's UUIDs and
compact ids, which do not exist in a fresh run. They now take the backlog id,
the backlog UUID and the object reference as arguments, and the PowerShell
drivers (`backlog.ps1`, `supersession.ps1`) read the new ids off the screen and
write them to `evidence/`.

**Every timing constant moves with the host.** The publication window was
1171 ms uninterrupted this run against 1045–1134 ms before, so:

```text
staging begins      ~860 ms      (destination/ appears)
barrier installed   ~910 ms      (format.json becomes engr-migration-in-progress)
streams published   950–1000 ms
format.json removed ~1050 ms
VERSION written     ~1080 ms
sweep finished      ~1171 ms
```

`crash-sweep.sh` prints the uninterrupted timing first; take the instants for
`barrier-window.sh`, `barrier-switch.sh`, `publication-order.sh`, `round21.sh`,
`stale-stage*.sh` and `withdraw-interrupt.sh` from it. `round21.sh` needs the
window between VERSION and the sweep and reported `never landed in the window`
until it was widened to 1060–1170.

**Two harness bugs that produced misleading evidence.** `stale-stage2.sh`
concluded `GONE: the resume published a stage that predates it` after a resume
that had **refused** — it was grepping the migrated record, which the refusal
means does not exist. It now distinguishes a refusal from a publication and
counts the wording still in the predecessor record. And in
`withdraw-interrupt.sh` a heading written with backticks inside a double-quoted
`echo` ran its own text as a command substitution — the shell tried to execute
`migrate` and spliced its (empty) output into the line. The backticks are gone.

`$?` after a pipeline is the exit code of `head`, not of engr. Several inherited
scripts print `exit=0` after an error line for that reason; where the exit code
was load-bearing this run captured it into a variable first
(`harness/barrier-switch-full.sh`).

## Building the states these two commits are about

`0086bb6` is about what the listing says, so most of the new work is three
workspaces that differ only in what is wrong with them, all from
`checkpoints/migrated` (`harness/ls-damage.sh`):

```text
dep-b   the Object projection removed, its Event stream left intact
dep-c   the projection present and unreadable
dep-d   one Section's wording edited and nothing resealed
```

and the three removals that separate the evaluator's questions
(`harness/removal-controls.sh`): resealed removal, unsealed removal, and removal
through an admitted deletion Event.

`harness/tail-listing.sh` builds the legitimate crash tail — admit a Section,
then put the projection back — which is the state F-2 is about, because the
admitted wording is in the record and out of the listing.

F-4 needs the whole screen, not its first line: `harness/barrier-switch-full.sh`
kills the confirm inside the publication window, edits a predecessor Object, and
prints everything the resume says. The `note` naming the file written over is on
the second line.

## Harness notes carried forward, still true

- **`node` is on the host, not in the image.** Run `jcs.js`, `evseal.js`,
  `refdigest.js`, `forge.js` and `appendev.js` from Git Bash against files under
  `r5/`, and run `engr` in docker.
- **Docker from PowerShell, never from the Bash tool** — Git Bash rewrites
  `/audit/...` into a Windows path and docker refuses it. This bit again this
  run: `docker … -w /audit/r5` from Git Bash became
  `C:/Program Files/Git/audit/r5`.
- **`sed -n 's/.*CONFIRM \(…\)/\1/p'` needs `| head -1`.** A prepare that
  supersedes an earlier candidate prints two matching lines;
  `tail-and-purge.sh` still has the unfixed version and its first probe dies on
  `the response must be exactly CONFIRM <code>`.
- **`work unblock` takes `--index`, not `--position`** — inherited wrong,
  corrected here.
- **`--ref <OBJECT:SECTION> <FIELDS>` takes two values**, and the object is a raw
  id or prefix.
- **`stop_for_test` is `#[cfg(debug_assertions)]`** — a release binary has no
  failure hook, so every interruption here is a real SIGKILL.
- **Restore the fixture, do not rebuild it.** `r5/project` came from
  `checkpoints/pre-migration` and its inventory diffed identical to the
  committed `inventory-pre-migration.txt`.

## Layout

```text
rerun-0086bb6/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    22 logs: argv + stdout + stderr + exit code, verbatim; 01 is
                  the fixture script, captured whole
  evidence/       inventories, the digest premise, the crash sweep and resume,
                  the barrier window and switch, the adversarial suites, and
                  every domain exercise
  harness/        every script this run used
```

Line endings are normalized to LF as in the parent runs; trailing whitespace
inside lines is engr's own column padding and is left alone, so
`git diff --check` reports it. The `??` and `禮` sequences in `transcripts/` are
the Windows console codepage mangling `—` and `§` on the way through PowerShell;
`evidence/` files are written straight to disk by the container and render
correctly.

# Runbook — re-run at `b02d05e`

The fourth audited head. Method as in [`../rerun-52fe116/RUNBOOK.md`](../rerun-52fe116/RUNBOOK.md)
and [`../rerun-ca6474a/RUNBOOK.md`](../rerun-ca6474a/RUNBOOK.md); this records
only what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         b02d05e…   (PR #67, answering review 5122007108)
comparison build    ca6474a…   (the previously audited head)
container image     engr-rust:latest
```

## What this run added to the method: a second binary

Three of the four findings here are about whether a behaviour is new. Reading the
diff answers that badly — the diff says what changed in the source, not what
changed on screen. So the previously audited head is built as a **third binary**
and the same forged workspace is run through both:

```bash
git archive ca6474a | tar -x -C <audit>/src-ca6474a
# build it with its own CARGO_TARGET_DIR so the current build stays warm
docker run --rm -v <audit>:/audit -v engr-cargo-prev:/target \
  -v engr-cargo-registry:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/target \
  -w /audit/src-ca6474a engr-rust:latest \
  bash -c "cargo build --release -p engr && cp /target/release/engr /audit/bin/engr-prev"
```

`t4.ps1 -Which prev` then runs it. This is what turns "the `ls` column is a gap"
into "the `ls` column is a **pre-existing** gap", and what turned the round-27
fix from a claim into `evidence`: the same workspace where `b02d05e` fails the
dependent Object, `ca6474a` passes it.

Build both from a clean tree. The release profile emits one `dead_code` warning
(`Stage::as_str`, reachable only from the `#[cfg(debug_assertions)]` test hook);
it is present at both heads and is not a signal.

## Building the states these three commits are about

All from `checkpoints/migrated`, a committed generation-1 workspace, using
`harness/forge.js` — which reseals correctly, so integrity cannot see any of it
and only the target's own history can.

```bash
# the target seals perfectly and is not what its history produced
node harness/forge.js set <object.json> 1 header '"A heading nobody ever admitted"'

# a Section removed and the Object resealed  -> divergence, not a deletion
node harness/forge.js del <object.json> 1

# the same removal without resealing         -> the control: ordinary damage
node harness/forge.js rawdel <object.json> 1
```

The third state — an admitted deletion through the gate — is the control that
keeps the first two honest: without it, "absence reads as divergence" cannot be
told from "absence always reads as divergence".

Forge a field the Ref does **not** select (`header`, not `text`), or drift and
staleness see it and the finding is about the wrong thing.

### A history that seals, frames and will not replay

`harness/appendev.js` appends one correctly sealed Event of any shape:

```bash
node harness/appendev.js <stream>.jsonl <object-uuid> 2 section.deleted.v1 '{"section":99}'
```

That is the only state this run found that gets past the framing rules — a
truncated stream, a missing stream and a history that starts at revision 2 are
all caught earlier, with their own messages. It is what F-3 is about.

## The barrier window, and why it needs its own sweep

F-4 needed instants either side of the moment the barrier goes up, which is
**not** the same as the moment the destination is staged:

```text
kill@800ms  staged=yes  format.json = {"format":"engr-workspace","version":1}     released build works
kill@850ms  staged=yes  format.json = {"format":"engr-migration-in-progress",…}   released build refused
```

`harness/barrier-window.sh` finds it; `harness/barrier-switch.sh` then applies
the identical edit at four instants either side. Both take the instants from the
timing of an uninterrupted confirm on the day, because the window moves — it was
1038 ms and 1343 ms in two runs an hour apart on this host.

`harness/stale-stage2.sh` is the version that matters most and the one that came
back clean: inside the pre-barrier window the released binary still writes, so a
day's work is admitted through *its* Human Gate and the resume is then asked to
publish over it. It refuses, and names the qualified response.

## Harness notes

- **Every adversarial probe now hashes the workspace either side of its own
  mutation** and prints `NO-OP!` instead of a pass when nothing changed. The
  `ca6474a` run lost a scenario silently to a moved constant; this makes that
  visible in the same output rather than in a diff of two reports.
- **`node` is on the host, not in the image.** Run `jcs.js`, `evseal.js`,
  `refdigest.js`, `forge.js` and `appendev.js` from PowerShell or Git Bash
  against the files under `r4/`, and run `engr` in docker.
- **`sed -n 's/.*CONFIRM \(…\)/\1/p'` needs `| head -1`.** A prepare that
  supersedes an earlier candidate prints two matching lines, and the confirm then
  gets two codes on one line and is refused for the right reason.
- **A closed Object refuses `--add`.** Two probes died on that before it was
  obvious; reopen first, or use an open Object.
- **`--ref <OBJECT:SECTION> <FIELDS>` takes two values**, and the object is a raw
  id or prefix — the compact `obj:<id>:<n>` spelling is refused there.
- **`work unblock` takes `--index`, not `--position`.**
- **Docker from PowerShell, never from the Bash tool** — Git Bash rewrites
  `/audit/...` arguments into Windows paths. Anything longer than one line goes
  into `harness/*.sh` and is run as `sh /audit/r4/harness/<name>.sh`, which is
  also what makes it evidence.

## Layout

```text
rerun-b02d05e/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    26 logs: argv + stdout + stderr + exit code, verbatim
  evidence/       inventories, the digest premise, both crash sweeps, the
                  barrier window and switch, the adversarial suites, and every
                  domain exercise
  harness/        every script this run used
```

Line endings are normalized to LF and the harness's trailing blank line dropped,
as in the parent runs; trailing whitespace inside lines is engr's own column
padding and is left alone, so `git diff --check` reports it. The `??` and `禮`
sequences in `transcripts/` are the Windows console codepage mangling `—` and
`§` on the way through PowerShell; `evidence/` files are written straight to disk
by the container and render correctly.

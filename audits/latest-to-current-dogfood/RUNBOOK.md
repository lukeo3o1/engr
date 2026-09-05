# Runbook

How to reproduce this audit from nothing. Every step below was run exactly once,
in this order, and its output is under `transcripts/` or `evidence/`.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         a77887b97eb06fa0ce1b90178e841fa1f0527d63   (PR #67 head)
container image     engr-rust:latest
```

Nothing else is pinned deliberately: the audit is about these two builds meeting
each other, and any host with a Rust toolchain and Git should reach the same
conclusions.

## 1. Build both implementations

```bash
git archive e7d9f99733407a8c31cec33af18a92480f4f4c6f | tar -x -C <audit>/src-latest
cargo build --release -p engr   # in <audit>/src-latest  -> bin/engr-latest
cargo build --release -p engr   # in the repo at a77887b -> bin/engr-current
```

The historical tree has no `.git`, so its `build.rs` reports `ENGR_COMMIT=unknown`.
That is fine — no persisted byte carries it. Confirm you have two binaries:

```text
engr latest (unknown)
engr latest (a77887b9)
```

## 2. Build the historical workspace

Use **only** `engr-latest` and only its own CLI. Do not author JSON.

```bash
git init && git config core.autocrlf false && git config core.eol lf
engr-latest init
```

`core.autocrlf false` is not tidiness: every predecessor Section carries a seal
over its exact octets, and a checkout that helpfully rewrote line endings turns
the whole fixture into a forgery report.

Then build a record big enough that continuity means something — the one in
`evidence/audit-project.bundle` has three Objects, seven Sections, a revision, a
merge, a cross-Object reference, a closed Object, a superseded candidate and one
candidate deliberately left pending. Commit `.engr` before creating any
reference: a Ref pins a commit and resolving it reads the target out of that
commit, so an uncommitted `.engr` gives

```text
error: historical workspace at commit <oid> has no format.json and is not a recognized legacy v0 workspace
```

Record the inventory (`harness/inventory.sh`) and a passing `verify`.

## 3. Checkpoint

```bash
cp -a project checkpoints/<name>      # pre-migration, crash-a..c, adversarial, worktree
```

Every interruption and every tamper below runs on a copy. The line the audit
never crosses is mutating the workspace it is carrying forward.

## 4. Meet the new binary before migrating

Run `ls`, `ls --all`, `show <id>` and `verify` with `engr-current` and capture
stdout, stderr and exit code for each. This is the moment an ordinary agent
would hit, and what it is told here is the whole of its guidance.

## 5. Migrate

```bash
engr-current migrate                  # preflight; mints a code, publishes nothing
git status --porcelain                # must be empty
engr-current confirm "CONFIRM <code>"
```

Between those two commands, checkpoint again and capture:

- `git status` (must be clean),
- `.git/info/exclude` (must carry `/.engr/local/`),
- `.engr/local/challenges/<code>.json` (the frozen subject),
- whether `.engr/local/migration/destination` exists (it must not).

Record the wall clock immediately before confirming; the migration Events'
`admitted.at` must fall after it.

## 6. Crash and resume

`harness/crash-sweep.sh` copies a prepared checkpoint and kills the confirming
process with `SIGKILL` at a sweep of instants. Find the publication window first
by timing one uninterrupted confirm — it was 747 ms here, and the interesting
states were between 640 and 740 ms. Then:

- `harness/crash-resume.sh` resumes every interrupted state and checks reads
  fail closed, resume succeeds, the retry is spent, one Event per stream
  survives, and `verify` passes;
- `harness/crash-instant.sh` kills after staging, waits six seconds, resumes,
  and compares the Event's `admitted.at` against both wall clocks — and diffs
  the result against an uninterrupted migration.

There is no failure hook in a release build. The interruption has to be real.

## 7. Use the migrated workspace

Author Rules under `.engr/rules/`, then drive the two-step governed mutation:
the first attempt refuses and hands back a ReviewDigest, the second repeats the
whole mutation with `--review`, `--reviewed-rule` per rule, `--review-attempt`
and `--review-result`. `harness/gate2.ps1` does both steps; `harness/gate.ps1`
reads the code off the rendered screen and answers it, because reading the
screen is part of what is being audited.

Exercise Backlog, Work and Collection on the audit's own real content. Use
`backlog show <item> --format json` to read the `--expect` tokens — note that
`expect` is an object of per-operation tokens, and each section carries its own.

## 8. Adversarial

`harness/adversarial.sh`, `harness/adv-yaml.sh`, `harness/adv-git.sh`,
`harness/adv-final.sh` and `harness/adv-verify.sh`. Each takes a fresh copy of a
clean migrated workspace, applies one mutation, runs one command, and records
the exit code. A scenario is only PASS if the refusal was observed.

## Harness notes

`harness/t.ps1` records argv, stdout, stderr and exit code verbatim into
`transcripts/<log>.txt`. It quotes each argument explicitly because
`Start-Process -ArgumentList` joins an array with spaces and quotes nothing,
which silently splits any argument carrying a space.

The `??` and `禮` sequences in some transcripts are the Windows console codepage
mangling `—` and `§` on the way through that harness. engr's own output is UTF-8
and renders correctly when written straight to a file; compare
`evidence/final-workspace-state.txt`.

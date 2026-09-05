# Runbook — re-run at `ca6474a`

The third audited head. Method as in [`../rerun-52fe116/RUNBOOK.md`](../rerun-52fe116/RUNBOOK.md);
this records only what is different or newly needed.

## Pinned inputs

```text
source release      e7d9f99733407a8c31cec33af18a92480f4f4c6f
destination         ca6474a3…   (PR #67, answering review 5120893221)
container image     engr-rust:latest
```

## The fixture is restored, and proved to be

`r3/project` is the parent run's pre-migration checkpoint, and its inventory is
diffed against the committed `inventory-pre-migration.txt` before anything else.
Rebuilding it would give every downstream difference two possible causes.

## What this run added to the harness

### Two digest contracts, implemented independently

`harness/jcs.js` was already the RFC 8785 JCS + SHA-256 implementation for
Section and Object seals. This run adds the two the re-review turned on:

```bash
# every Section and Object seal in a file
node harness/jcs.js <workspace>/.engr/objects/<id>.json

# EventDigestContract 1: SHA-256 over JCS of {object, event-minus-digest}
node harness/evseal.js <stream>.jsonl <object-uuid> check

# and the same script, without `check`, appends an Event whose id is the
# uppercase spelling of an existing one, sealed over that spelling — the only
# way to test the id rule without the seal objecting first
node harness/evseal.js <stream>.jsonl <object-uuid>
```

The RefDigest preimage is assembled inline in the report's check: `{target,
fields, values, commit}`, with a selected absent optional collection carried as
`null`. Run the premise check before trusting any of them — a forgery is only
interesting if it really is sealed.

### The nested workspace with metacharacters

Needed for the gitignore-escaping finding, and it cannot be built by moving the
released fixture wholesale: a relocated `.engr` cannot resolve historical Ref
targets at the paths its own commits recorded. Reduce to the objects with no
references first.

```bash
cp -a checkpoints/pre-migration checkpoints/nested/outer
cd checkpoints/nested/outer
mkdir -p 'project[1]' && mv .engr 'project[1]/.engr'
rm -f 'project[1]/.engr'/{objects,events}/<the referencing object>.*
git add -A && git commit -m "the workspace, nested"
```

Then prepare a migration inside `project[1]` and ask **git**, not engr, whether
the live Challenge is hidden — and run the control, because a test that only
checks the new pattern proves nothing about the old one:

```bash
git check-ignore -v 'project[1]/.engr/local/challenges/<code>.json'
git status --porcelain --untracked-files=all
printf '/project[1]/.engr/local/\n' > .git/info/exclude   # the control
```

### The interrupted-withdrawal state

Built by unlinking exactly what the interruption unlinks, in that order:

```bash
rm .engr/local/challenges/<code>.json
rm .engr/local/migration/manifest.json     # the directory stays
```

A read must then describe the workspace as the predecessor, and `migrate` must
mint a **fresh** code rather than pointing at the one that is gone.

### The crash tail whose Ref exists only in the tail

The regression for finding 5, and the shape matters: the stored projection must
have **no** Ref, so that walking it would find nothing to check.

```bash
cp <object>.json /tmp/rev1.json          # before the mutation
engr prepare --add --object <id> --ref <target-id>:1 text   # and confirm
cp /tmp/rev1.json <object>.json          # the crash
sed -i 's/"state":"open"/"state":"not-a-state"/' <target>.json
engr verify <id>                          # must FAIL, naming the target
```

`--ref` takes `<object-id>:<section>` and a comma-separated field list. The
compact `obj:<id>:<n>` spelling is refused there.

## Harness hazards this run hit

- **`gg3.ps1` parameter binding.** PowerShell binds the first unnamed argument to
  the next declared parameter, so `prepare` became `-Explanation` and
  `--classify` became `-Attempt`. `$EngrArgs` now carries the only explicit
  `Position`, which makes every other parameter name-only.
- **A literal-match mutation can stop testing silently.** `adversarial.sh` pins
  the fixture's Ref digest as a `sed` pattern; the RefDigest fix moved it, `sed`
  matched nothing, and the scenario passed at exit 0 having done nothing. Pin as
  little as possible, and treat a scenario that suddenly starts passing as a
  reason to look.
- **Rules govern everything once they exist.** After authoring a Rule that
  applies to `object`, even `--classify` needs the two-step. Author them after
  the ungoverned lifecycle work, or drive everything through `gg3.ps1`.

## Layout

```text
rerun-ca6474a/
  REPORT.md
  RUNBOOK.md      this file
  transcripts/    argv + stdout + stderr + exit code, verbatim
  evidence/       inventories, crash sweep and resume, adversarial suites,
                  the nested-exclusion check and its control
  harness/        every script this run used
```

Line endings are normalized to LF and the harness's trailing blank line dropped,
as in the parent run; trailing whitespace inside lines is engr's own and is left
alone.

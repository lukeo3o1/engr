# latest → current dogfood audit

**Question.** Could an AI coding agent reasonably start using the published
`latest` engr as its engineering memory, later upgrade to the PR #67 / #66
design without losing or misrepresenting knowledge or provenance, and continue
using the migrated workspace as its normal working system?

**Answer. YES, WITH NON-BLOCKING GAPS.**

Continuity itself held on every axis that was checked, including the ones a
byte-comparison cannot check: identities, admission provenance, dependency
semantics, crash recovery and refusal behaviour. The gaps are all in the
*surface* — one health-check command disagrees with two others, and several
error messages name commands and versions that do not exist. None of them
corrupts a record. All of them cost an agent a round trip, and one of them can
cost an agent a false clean bill of health.

| | |
|---|---|
| Source release | `e7d9f99733407a8c31cec33af18a92480f4f4c6f` (`engr latest`, `{"format":"engr-workspace","version":1}`) |
| Destination | PR #67 head `a77887b97eb06fa0ce1b90178e841fa1f0527d63` (`engr latest (a77887b9)`, generation 1) |
| Method | clean room, two separately pinned binaries, exact transcripts, disposable checkpoints |
| Product code changed during the audit | none |

---

## How this was run

Both implementations were built from source into one container image and kept
side by side, so every command in this report names which one ran it:

```text
bin/engr-latest    from e7d9f99  ->  engr latest (unknown)
bin/engr-current   from a77887b  ->  engr latest (a77887b9)
```

A fresh Git project was created and its engineering record built **only through
the published `latest` CLI and its own Human Gate** — no JSON was authored by
hand, because a hand-authored fixture proves the migrator accepts a shape
somebody believed version 1 had, not the shape the release actually wrote. The
record describes this audit: what continuity means here, what counts as
evidence, and how the fixture is built.

The workspace was then committed, checkpointed six ways, and carried forward.

Everything under `transcripts/` is verbatim: argv, stdout, stderr and exit code
for every command, in the order they ran. `harness/` holds the scripts, so the
scripted parts re-run as they were. `evidence/audit-project.bundle` is the whole
audit project including its Git history, so the fixture itself is reproducible
rather than described.

### The audit used engr as its own memory

This is not a fixture that was generated and then inspected. The conclusions
below were admitted into the record through the real Human Gate, under real
project Rules, while the audit was running:

- **Objects/Sections** hold the durable conclusions — three of them, admitted
  under `--role decision` and `--role risk` after passing Rule Review.
- **Backlog** holds the eight genuinely unresolved questions, including every
  finding in this report, with one `produced` outcome pointing at the Section a
  question turned into.
- **Work** held the audit's own execution state on its subject Object: two
  items, a summary checkpoint, a dependency on the Backlog topic, and a real
  blocker (the release binary has no debug failure hook, so interruption had to
  be induced from outside the process).
- **Collection** `upgrade-audit` groups the Object and the Backlog topic with
  order and priority.
- **Rules** `audit-scope` and `evidence-discipline` governed every semantic
  mutation the audit made, and were exercised passing, failing, exhausted and
  overridden.

`evidence/final-workspace-state.txt` is that workspace at the end.

---

## Scenario matrix

`PASS` means the expected behaviour was observed in a transcript in this audit.
Nothing is `PASS` on the strength of reading the implementation.

### Natural upgrade encounter

| Scenario | Result | Evidence |
|---|---|---|
| `ls` on a predecessor workspace | PASS — refused, exit 4, names `engr migrate` | `02-upgrade-encounter` |
| `show` on a predecessor workspace | PASS — refused, exit 4, names `engr migrate` | `02-upgrade-encounter` |
| `verify` on a predecessor workspace | PASS — refused, exit 4, names `engr migrate` | `02-upgrade-encounter` |
| Next step is discoverable from the refusal alone | PASS | `02-upgrade-encounter` |
| Only the documented migration path used | PASS — no hand JSON, no undocumented binary, no resealing | all transcripts |

### Migration

| Scenario | Result | Evidence |
|---|---|---|
| Prepare changes no Git-tracked byte | PASS — `git status` clean after preflight | `03-migration`, `git-local-not-tracked.txt` |
| Migration Challenge stays local-only | PASS — `.engr/local/challenges/4EU93K.json`, excluded via `.git/info/exclude` | `git-local-not-tracked.txt` |
| No destination staged before confirmation | PASS — `local/migration/` held only `manifest.json` | `03-migration` |
| Exact `CONFIRM <code>` required | PASS — wrong code exit 3; wrong case exit 2; bare code exit 2; challenge survived all three | `03-migration` |
| Actual confirm/apply instant on migration Events | PASS — Event `admitted.at` 19:02:18.80 vs Challenge `created_at` 19:01:31.91 and pre-confirm wall clock 19:02:12.11 | `03-migration`, `04-post-migration` |
| Historical Section timestamps stay historical | PASS — 18:58:35.49 / 18:59:03.17 preserved as `admitted.at` | `04-post-migration` |
| Object identities survive | PASS — same three UUIDv7s | inventories |
| Section identities survive per contract | PASS — ids 1,4,5,6,7 with `next_section_id` 8; ids retired by a pre-migration merge are still not reused | `06-domains` |
| Predecessor effective state interpreted correctly | PASS — rev 4/4/6 → 1, section counts 2/2/3 preserved | `03-migration` |
| Selective Refs use canonical compact identity | PASS — `obj:01m1f5ax7bedrbew8012yq3ma9:1`, round-trips to the UUID | `04-post-migration` |
| Migrated Refs remain resolvable | PASS — `show` reports the Ref `ok`; `verify` PASS | `04-post-migration` |
| Migrated Ref attests only what the predecessor attested | PASS — `fields: [based_on, refs, text]`, no `admission`/`header`/`role`/`content`/`relations` | `04-post-migration` |
| Old pending Candidate not migrated | PASS — `candidates/8TTSM6.json` existed at migration and is gone; preflight said so in advance | `01-historical-build`, inventories |
| EventStore bootstrap correct | PASS — one `object.migrated.v1` per Object at `rev 1`, Object also `rev 1` | `04-post-migration` |
| Next ordinary mutation emits rev 2 | PASS | `05-rules` |
| Canonical JCS persisted representation | PASS — non-canonical, duplicate-key and unknown-member bytes all refused | `adversarial.txt` |
| `.engr/VERSION` and layout correct | PASS — `1`, and the §16 layout exactly | `inventory-post-migration.txt` |
| Predecessor state removed only when safe | PASS — `format.json`, `events/`, `candidates/` gone only after publication; see crash matrix | `crash-sweep.txt` |
| No mixed steady-state generation is readable | PASS — every incomplete state refuses reads and names the resume command | `crash-resume.txt` |
| Legal predecessor history-prefix purge accepted | NOT RUN — the fixture this audit built has complete history; not exercised here | — |

### Crash and resume

Induced by `SIGKILL` on the confirming process, swept across the 747 ms
publication window until four distinct intermediate states were reached. No
synthetic marker was used; the release binary contains no failure hook.

| Scenario | Result | Evidence |
|---|---|---|
| Incomplete migration fails closed | PASS — all four states refuse reads with exit 4 | `crash-resume.txt` |
| Retry/resume is deterministic | PASS — all four resume to a complete generation-1 workspace | `crash-resume.txt` |
| Resume is idempotent | PASS — second attempt exit 3, the code is spent | `crash-resume.txt` |
| Predecessor material not destroyed prematurely | PASS — `format.json` and `events/` survive until after the destination is durable | `crash-sweep.txt` |
| No duplicate Event | PASS — exactly one Event per stream in every resumed workspace | `crash-resume.txt` |
| Identities remain stable | PASS | `crash-resume.txt` |
| Final result matches an uninterrupted migration | PASS — `objects/` byte-identical; Events differ only in Event id, admission instant and the seal over them | `crash-admission-instant.txt` |
| Admission timestamps remain truthful | PASS — killed at 19:15:57.75, resumed at 19:16:03.75, Event kept 19:15:57.36 | `crash-admission-instant.txt` |

### Continuing to use the migrated workspace

| Scenario | Result | Evidence |
|---|---|---|
| Read migrated knowledge | PASS | `04-post-migration` |
| Revise migrated knowledge | PASS — rev 2 through 6 on a migrated Object | `05-rules`, `08-conclusions` |
| Create new knowledge | PASS | `08-conclusions` |
| Human admission | PASS | `05-rules`, `08-conclusions` |
| Agent admission through Rule Review | PASS — no Challenge, direct admission on a passing review | `05-rules` |
| Roles and headers | PASS — `--role decision`, `--role risk`, `--header` | `08-conclusions` |
| Selective Refs, selected-field drift | PASS — `show` reports `refs moved` and names `based_on, text` | `adv-verify-vs-show.txt` |
| Unselected-field-only change is not drift | PASS — `header` changed on the target, nothing reported | `adv-verify-vs-show.txt` |
| Verification | PARTIAL — integrity yes, dependencies no; see finding F1 | `adv-verify-vs-show.txt` |
| Type/state lifecycle | NOT RUN — `--classify` and the typed vocabularies were not exercised | — |
| Supersession | NOT RUN | — |
| Repair | NOT RUN — no legitimate scenario arose; every tamper was refused rather than repaired | — |
| Backlog create/add/revise/subjects/produced | PASS | `06-domains` |
| Backlog stale-predecessor protection | PASS — a stale `--expect` token is refused, exit 6 | `06-domains` |
| Backlog consume / merge | NOT RUN | — |
| Backlog Rule Review exhaustion behaviour | NOT RUN — Backlog exhaustion path not reached | — |
| Work: subject, items, summary, dependencies, blockers | PASS | `06-domains` |
| Work never becomes semantic authority | PASS — every Work surface says so in its own banner, and Work state never appeared on a record surface | `06-domains` |
| Work pause semantics | NOT RUN | — |
| Collection: membership, order, priority, state | PASS — Object and Backlog in one plan | `06-domains` |

### Rules

| Scenario | Result | Evidence |
|---|---|---|
| Multiple applicable Rules | PASS — both rules named in one review | `05-rules` |
| Passing review | PASS — Event records `{outcome: passed, result: passed, attempts: 1}` | `05-rules` |
| Failed review, Human override | PASS — Event records `{outcome: overridden, result: failed, attempts: 2}` | `05-rules` |
| Exhausted review, Human override | PASS — Event records `{outcome: overridden, result: exhausted, attempts: 4}` | `05-rules` |
| `result` distinguishes failed from exhausted in history | PASS | `05-rules` |
| Rule byte drift invalidates a pending Challenge | PASS — exit 5, "the Rule Review material moved after challenge X7SQTB was prepared" | `05-rules` |
| Semantically equivalent, byte-different Rule edit | PASS — a flow→block reformat of one sequence was enough | `05-rules` |
| Restoring the exact bytes revives the same code | PASS — the same `CONFIRM X7SQTB` then admitted | `05-rules` |
| Frozen Challenge review context | PASS — `{digest, result, attempts, rules, explanation}` frozen in `subject.data` and rendered from there | `05-rules` |
| Explanation is decision-time only | PASS — present in the Challenge, absent from the Event | `05-rules` |
| ReviewDigest is not durable Event provenance | PASS — absent from every Event | `05-rules` |
| Durable review provenance matches #66's amendment | PASS | `05-rules` |

### Adversarial

| Scenario | Result |
|---|---|
| Qualified Human confirmation (wrong case, bare code, wrong code) | PASS — refused, challenge survived |
| Stale/superseded Challenge | PASS — the superseded code resolves to nothing, exit 3 |
| Rule change after review, before confirmation | PASS — exit 5 |
| Object tamper | PASS — exit 5, integrity failure named |
| Section tamper | PASS — exit 5, the Section named |
| Event tamper | PASS — exit 4, the Event's own seal named |
| Event moved across streams | PASS — exit 4 |
| Duplicate JSON key | PASS — exit 4, non-canonical bytes |
| Non-canonical JSON | PASS — exit 4 |
| Unknown member | PASS — exit 4 |
| Malformed JSON | PASS — exit 4 |
| Truncated Object file | PASS — exit 4 |
| Truncated Event file | PASS — exit 4 |
| Filename ≠ embedded id | PASS — exit 4 |
| Invalid Ref digest | PASS — exit 5 |
| Ref repointed to another Section | PASS — exit 5 |
| Unavailable historical commit | PASS — exit 5 |
| Ref target in the superseded raw-UUID dialect | PASS — exit 4, refused |
| Selected-field Ref drift | PASS — reported, and the moved fields named |
| Unselected-field-only change | PASS — not reported |
| YAML anchor (block, flow, explicit key) | PASS — all refused |
| YAML alias (block, explicit key) | PASS — refused |
| YAML custom tag (value, explicit key) | PASS — refused |
| Flow-style YAML that is legal | PASS — loads |
| Explicit mapping-key YAML | PASS — refused |
| Duplicate YAML keys (block, flow, two spellings) | PASS — refused |
| Dirty Git basis | PASS — exit 5, names both ways out |
| Linked Git worktree handling for `.engr/local/` | PASS — exclusion in the common dir, challenge invisible to git |
| Concurrent/stale mutation | PASS — Backlog `--expect` token, exit 6 |
| Missing/unreadable dependency | PASS — exit 5 |

---

## Findings

None of these blocks the upgrade. All are surface defects; the record itself was
correct throughout. They are recorded as Backlog points in the audit workspace
as well as here.

### F1 — `verify` says PASS on a moved dependency, and its help does not say why

After a *selected* field of a Ref target was changed through the ordinary gate,
`verify` printed `PASS` and exited 0 for the dependent Object, while at the same
moment `ls` printed `1 stale` and `show` printed `refs moved` with the moved
fields named.

The behaviour is deliberate. `ops::verify` matches
`Dependency::Unchanged | Dependency::Drifted { .. } => {}` and reports only
`TargetIntegrityFailure`, `TargetMissing`, `DigestInvalid`, `SchemaMismatch` and
`ProvenanceUnavailable`. That is a coherent line to draw: a broken seal is
corruption, and drift is a legitimate change somebody made on purpose. The
adversarial matrix confirms the line holds — a bad Ref digest, a repointed Ref
and an unavailable historical commit all make `verify` FAIL with exit 5.

The finding is that nothing tells an agent where that line is. `verify --help`
reads "Verify Object and Section integrity **plus dependencies**", which is true
of dependency *faults* and not of dependency *drift*, and the two sibling
surfaces both report the drift the same second. An agent that picks `verify` as
its health gate — the natural choice, given the name and the help text — gets a
clean answer on a workspace `ls` calls stale, and has no way to learn from the
CLI that this was intended.

Not caused by PR #67: `crates/engr/src/ops.rs` is untouched across the whole PR
range. Evidence: `evidence/adv-verify-vs-show.txt`.

### F2 — a refusal names a command-line flag that does not exist

`backlog add` without `--expect` refuses with:

> run `engr backlog show <item> --json`, read the point, and pass its expect value back

`--json` is not a flag on `backlog show`; the working form is `--format json`.
An agent following the instruction gets a clap parse error. The *next* error
message it hits says `backlog show --format json` correctly, so the two
disagree with each other. Evidence: `transcripts/06-domains.txt`.

### F3 — `--expect` is described as a value and is an object

The same message says "pass its expect value back". `backlog show --format json`
returns `expect` as a mapping of per-operation tokens (`rename`, `add`) plus a
separate `expect` per section. An agent that reads the sentence literally passes
the object and is told it must be 64 hex characters. Evidence: `06-domains`.

### F4 — `engr migrate --help` names a generation that does not exist

> Explicitly upgrade a recognized predecessor workspace to v3

The destination is generation 1, and `.engr/VERSION` contains `1`. Stale
vocabulary on the one surface an agent reads before deciding to migrate.

### F5 — the predecessor refusal calls the workspace "read-only"

> the released predecessor workspace is read-only here; this engr writes generation 1. Run `engr migrate` before mutation

`ls`, `show` and `verify` are all refused on it. Nothing is readable. The
message is doing the important half of its job — it names the next command —
while describing the situation incorrectly. Evidence: `02-upgrade-encounter`.

### F6 — migration leaves the predecessor's dead lock file

`.engr/lock` survives the migration; the generation-1 lock is `.engr/local/lock`.
It is untracked and harmless, but it is a file the destination layout does not
define, and it is the sort of thing a later integrity check might notice.

### F7 — the same idea has a different flag name in each domain

`backlog produced --target`, `work depend --on`, `collection add --target
--reason`. Each is reasonable alone. Together they cost an agent a `--help`
round trip per domain.

### Not a finding

The `??` and `禮` sequences visible in some transcripts are the Windows console
codepage mangling `—` and `§` in the capture harness, not engr output. The same
commands render correctly when written straight to a UTF-8 file — see
`evidence/final-workspace-state.txt`.

---

## AI-agent usability review

Kept separate from correctness on purpose: everything below is about an agent
that repeatedly loses and reloads its context, not about whether the record is
right.

**What works well.**

- *Canonical references are discoverable and machine-readable.* Every read
  surface prints `engr:obj:<compact>` and `engr:obj:<compact>:<n>`, the JSON
  output carries `reference` on both Object and Section, and those strings are
  accepted back as input. An agent can round-trip an identity out of one
  invocation and into the next without holding state. This is a real
  improvement on the predecessor, where the abbreviated id shown at prepare time
  (`01a05e55`) became ambiguous the moment a second Object was created in the
  same millisecond — observed in `01-historical-build`, where three Objects
  shared an eight-character prefix.
- *Refusals name the next command.* The predecessor-workspace refusal names
  `engr migrate`; the incomplete-migration refusal names `engr migrate` to
  resume; the governed-mutation refusal hands back the exact ReviewDigest to
  repeat with. An agent can act on these without reading source.
- *Authority boundaries are stated on every screen.* "UNCONFIRMED STAGING",
  "EXECUTION MEMORY — admitted by nobody", "PLANNING — says nothing about what
  its members mean". An agent that dumps a Work sidecar into its context cannot
  mistake it for the record.
- *State is cheap to keep synchronised* because there is almost none to keep.
  The Challenge is on disk, the review context is frozen inside it, and
  `engr candidate <code>` re-renders the whole question. An agent that dies
  mid-decision loses nothing.

**Where it costs an agent.**

- *`verify` is the obvious health check, and the one place drift does not
  surface* (F1). The split between dependency faults and dependency drift is
  deliberate and defensible, but no CLI surface says where it falls, so an agent
  will run `verify`, see PASS, and never learn its dependencies moved. Of
  everything here, this is the one most likely to produce a confidently wrong
  agent.
- *Two-step Rule Review needs the full command repeated.* The first attempt is
  always a refusal that hands back a digest; the agent must reissue the entire
  mutation with `--review`, `--reviewed-rule` (once per rule), `--review-attempt`
  and `--review-result` appended. That is a lot of argv to reconstruct from a
  refusal, and the refusal names the digest but not the rule ids in a form that
  can be pasted back — they arrive as prose (`governed by audit-scope,
  evidence-discipline`).
- *Per-domain argument shapes have to be relearned* (F3, F7). `backlog`
  subcommands take a positional item; `work` takes a subject reference;
  `collection` takes an id. The `--expect` token is per-operation and per-section
  and is not documented in the parent help.
- *One refusal actively misdirects* (F2). An agent that trusts error text spends
  a round trip on a flag that does not exist.
- *Nothing incentivises bypassing engr.* The Human Gate is the only door for
  durable knowledge, the Agent door requires a real review, and both are cheap
  enough to use. The audit did not once want to write JSON directly.
- *Resuming from engr alone is possible.* `ls --all`, `backlog ls`, `work show`
  and `collection show` together reconstructed what the audit was doing after
  each context reset, without consulting the transcripts.

---

## Answer

**YES, WITH NON-BLOCKING GAPS.**

An agent can start on published `latest`, build a real record through the Human
Gate, upgrade to the PR #67 design, and carry on. Across this audit the upgrade
preserved every Object and Section identity, kept retired ids retired, preserved
historical admission provenance while stamping the migration's own actual
confirmation instant, converted references to the canonical compact identity
attesting exactly the three fields the predecessor seal covered and no more, and
refused every malformed, tampered, non-canonical and forbidden input put to it.
A migration killed mid-publication resumed to a byte-identical result and kept
the instant the human's answer was actually applied, six seconds after the
process died.

The gaps are surface-level, and every one of them is text rather than behaviour.
F1 is the one worth settling before an agent is told to rely on `verify` as its
health gate — the split it draws is defensible, but nothing on the CLI says
where the split falls. F2 and F4 are simply wrong on surfaces an agent reads
first. None of them can put a false fact into the record, which is the property
the record exists to have.

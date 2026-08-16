# engr protocol v0

Normative. Where this document and the implementation disagree, this document is
wrong and should be fixed, or the implementation is a bug — say which.

## The one rule

**Nothing enters the record that a human has not read and confirmed.** There is
no unconfirmed write path. Every action goes `prepare` → the human reads the
change → `confirm` with the exact phrase.

## Model

An **object** is an aggregate. It holds **sections**, each carrying text.

```text
object
├── id                        uuidv7
├── title
├── status                    open | closed
├── rev                       increments on every confirmed action
├── next_section_id           monotonic, never reset
└── sections[]
    ├── id                    integer, never reused, never renumbered
    ├── text                  always the current wording
    ├── based_on?             committed repository context, absent by explicit choice
    ├── refs[]                { object, section, sha256, commit }
    ├── sha256                hash of text + based_on + refs
    └── confirmed_at
```

A section's `text` is always its current wording, because wording only changes
through a confirmed action. Readers never have to ask which of two fields is
authoritative — there is only one.

### Sections are current authority; events are durable history

`.engr/events/<id>.jsonl` is append-only confirmed history and audit evidence.
It is never purged in v0, but it is not a replay authority or a second source of
current truth. **Sections remain authoritative for current wording**, and git
additionally preserves committed projections for look-back and tamper evidence.

This is the inversion from the previous design, and it is deliberate. Event
sourcing bought a replayable past at the cost of a vocabulary nobody used; the
part that produced value was the gate.

### Ids

`object.id` is a uuidv7 — time-ordered, with no date welded in, so nothing caps
how many objects a day can hold and nothing prevents recording something later
than it happened.

Objects are addressed by **unique id prefix**, the way git addresses a commit. A
uuidv7 begins with a 48-bit millisecond timestamp, so its first twelve hex
characters carry no randomness: **abbreviation must widen with the set** rather
than being a fixed width. Two objects created in the same minute share an
eight-character prefix.

Section ids are integers scoped to their object, taken from `next_section_id`.
They are **never reused and never renumbered**. Deleting a section leaves a gap,
and the gap is information: something was there. Reuse would silently repoint
every outside reference to it — which is why the counter must be persisted,
where `max(existing) + 1` would hand out a deleted id.

## Actions

Eight. All of them gated.

| Action | Data | Effect |
| --- | --- | --- |
| `object_created` | — | Creates the object with the confirmed title |
| `object_renamed` | — | Replaces the title; requires an open object |
| `section_added` | — | Appends a section, id from the counter |
| `section_revised` | `section` | Replaces that section's content; id unchanged |
| `section_merged` | `absorbs[]` | New id carrying the confirmed wording; absorbed sections removed |
| `section_deleted` | `section` | Removes the section |
| `object_closed` | — | `status` → `closed` |
| `object_reopened` | — | `status` → `open` |

`object_created`, `object_renamed`, `section_added`, `section_revised` and
`section_merged` carry content; the others must carry none.

A **closed object refuses every section action, and a rename**. Reopen it first.
The friction is deliberate: if a closed object could still change, `closed`
would not mean "this has settled". A title is part of what settled, so exempting
it would narrow `closed` to "the sections have settled" rather than the whole
object.

Sections have no `status` field. Deletion deletes and merging merges, so every
section in the list is by definition current — there is no state to represent.

A merge must absorb at least two distinct sections.

## The gate

`prepare` validates a proposed action against the current object, mints a
six-character challenge from `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` — no `0`/`O` or
`1`/`I` — and stores a candidate.

`prepare` **refuses up front**, so nothing that cannot apply ever reaches a
human: the reducer is preflighted, `based_on` must name a real commit, and every
reference must resolve to an existing section whose current hash matches what is
being pinned. The target wording at `refs[].commit` MUST recompute to
`refs[].sha256`; an uncommitted target wording cannot be referenced. Deferring
reference checks to `verify` is what let one mistyped id in the previous design
poison a global health check permanently, with no way back.

There is **one live candidate per object**. Preparing again supersedes the
previous one, so a human never holds two codes for the same thing.

### The title

An object's title is a label, not a body. Every action that sets a title —
`object_created` and `object_renamed` — MUST refuse one that spans lines or
exceeds 120 characters, and the refusal MUST say where the detail belongs
instead and MUST name the action the caller actually took. The title is the line
a listing prints, so a body pasted into it degrades the listing for every other
object as well as its own.

That check belongs at the gate, **not** in payload validation: payloads are
validated when events are *loaded*, so a limit enforced there would leave a
workspace holding an over-long title unable to replay its own history.

A title is not written against anything. Read surfaces MUST NOT present a
title-carrying action as having a basis, even where one was recorded: a line
that means nothing about the change being confirmed is a line that teaches
people to skim the screen the whole design depends on them reading.

Titles are not unique and are not required to be. A duplicate MUST be reported
with the candidate and MUST NOT be refused — two objects may legitimately share
a title, but they cannot be told apart in a listing, and the moment to
reconsider is while the human is still holding the code. The object being
written MUST be excluded from that check, so a rename that changes only casing
or spacing does not report a clash with itself.

The candidate records `expected_rev`. A candidate prepared against an older state
cannot be confirmed.

### What a human is shown

The **complete semantic change**, not the whole section again. Revisions use a
unified line diff with limited unchanged context and separately show old/new
`based_on` plus added and removed refs, including their pinned hashes and
commits. An explicit absence of repository basis is displayed as such. Omitted
unchanged wording remains part of the complete candidate payload and
confirmation hash.

### Repository basis

`based_on` names the committed repository context against which wording was
formed; it is not an exact wording dependency (that is what `refs[]` records).
With a clean source worktree, omitted `--based-on` defaults to `HEAD`. If source
files outside `.engr/**` are dirty, omission is refused: the caller must select
a real commit with `--based-on` or explicitly assert no repository basis with
`--no-based-on`. The latter omits the field; it is never encoded as `null` or a
magic revision. Changes only under `.engr/**` do not make source context dirty.
Outside a Git repository, omission cannot default to `HEAD`, so `--no-based-on`
is required. If engr cannot positively determine that source is clean, it also
refuses an implicit basis rather than treating an unknown Git state as clean.

### Confirming

The response must be exactly `CONFIRM <code>`.

- Exactly that → admitted.
- `CONFIRM <code>` **followed by anything else** → this is a qualified yes, not
  assent. The candidate is **discarded**. "Yes, but reword the second line" must
  not become a yes, and the agent must not be the one deciding whether it counted.
- Anything else, including whitespace and casing slips → rejected, and the
  candidate survives. A typo is not a qualification.

On confirmation, engr appends the event, projects it into the sections, and
clears the candidate. Projection is immediate: the sections are the authority, so
they may not lag the log.

Re-confirming a code whose event is already applied is **idempotent** — it
reports what happened rather than applying it twice. That closes the crash window
between saving the projection and clearing the candidate.

### Projection is deterministic

The reducer takes an object and an event and nothing else. **No clocks, no git,
no language model, no interpretation of prose.** Everything it needs is inside
the event, because the agent's judgement was frozen there when a human confirmed
it. Structure that was not recorded does not exist.

## Integrity on the read path

Every read surface MUST recompute each section's hash before rendering it, and
MUST say so where the reader is already looking. A reading path that prints
unverified wording under an `ok` is worse than one that prints nothing: it is an
assertion, and this record's whole claim is that a human agreed to these words.

A section MUST also be marked when a section it references fails **its** hash.
Comparing `refs[].sha256` against the target's stored `sha256` cannot see this —
an edit that rewrites the target's text and leaves its stored hash alone moves
neither side of that comparison, so the reference looks untouched while the
wording under it was replaced. Only the directly referenced section is checked;
the target's own read covers what *it* stands on.

Corruption outranks staleness. A section whose content does not match its hash
is not a section that drifted, and its drift assessment describes something
nobody confirmed, so the label MUST report the corruption and not the drift.

## Staleness

Two signals, both computed at read time, both needing nobody to be reading.

| Signal | Computed from |
| --- | --- |
| The basis moved | `based_on` versus HEAD: commits ahead, files changed |
| A dependency changed | `refs[].sha256` versus the target section's current `sha256` |

Sections without `based_on` have no basis-movement signal. Both signals are
reported as **information, not a verdict**. A threshold nobody has
validated would be a guess, and a binary "stale" that fires on every commit is
worthless.

The comparison MUST exclude the workspace's own directory. `confirm` asks for
the object file to be committed, so counting that commit makes every section
stale the moment its own record is saved — the tool's instructions break the
tool's signal, and the only way back to zero is to re-confirm every section,
until the next commit. A signal that is always on is not read, including on the
occasions it is right. The exclusion also means a commit that changes nothing
outside the record — an empty one included — is not the basis moving. The
question the signal answers is *did what I decided against change*.

The pathspec MUST be anchored at the repository root. A cwd-relative exclusion
looks equivalent and is not: with the workspace in a subdirectory it narrows the
whole comparison to that subdirectory and hides changes elsewhere in the tree.
Reporting no change where there was one is the worse failure.

Status is never stored. A stored verdict is wrong the moment HEAD moves.

The case worth surfacing unprompted is a **closed** object whose basis moved:
closed means nobody is looking, which is exactly when drift goes unnoticed.

### Looking back

A hash proves something changed; it cannot say what it used to be. `refs[].commit`
records the commit the target was read at, so the old wording is one command away:

```bash
git show <commit>:.engr/objects/<id>.json
```

**git is therefore a hard dependency, not a nicety.** If objects are not
committed, look-back disappears silently — the recorded commit resolves to
nothing and everything looks fine until someone needs it. `init` says whether
this is a repository, and `confirm` says when an object has uncommitted changes.

## Verify

`verify` recomputes each section's hash from what is stored, and the hash of
each section those sections reference.

It catches a section edited without recomputing the hash. It **cannot** catch an
edit that recomputes the hash too. Append-only confirmed events preserve audit
evidence, and committed git history provides an additional tamper anchor, which
is why `verify` also reports an uncommitted object.

Do not read `verify` as proof that a human confirmed the current wording. It
proves internal consistency. The gate is a convention enforced by the agent's
instructions, not a mechanism — see below.

## What v0 does not solve

`prepare` prints the challenge code where the agent can read it, and the agent
runs `confirm`. **Nothing stops an agent confirming its own proposal.**

Treat `confirmed_at` and a matching hash as evidence about the *content*, never
as evidence that a human was present. Making the gate a mechanism needs the
challenge to travel where the agent cannot read it, or `confirm` to run in a
different process. That is not v0.

### Redaction is not a goal

Taking back something already confirmed — a secret, a person's name, a remark
about someone — is **not** engr's job, and is not waiting on a later version.

It cannot succeed here. Objects are committed and look-back reads them with
`git show <commit>:<path>`, so rewriting the current file leaves every earlier
copy exactly where it was. A redaction that looks like it worked while the text
is still one command away is worse than none, because someone will believe it.

It would also spend the read path's alarm. A section referencing the redacted
one pins its hash, so replacing the text makes every referrer report that a
section it stands on no longer matches — a signal that means *the record was
edited outside the gate*. Firing it for a sanctioned act teaches readers to
dismiss it, and it is the one signal that cannot afford to be dismissed.

The way through is built from what already exists: `section_deleted` through the
gate, which leaves the id gap that records something was there; then rewrite git
history with a tool meant for it; then accept that references into the deleted
section no longer resolve. engr adds nothing to those three steps, and pretending
otherwise would only obscure the second.

## Layout

```text
.engr/
  format.json              workspace format and version
  .gitignore               excludes lock and candidates/
  lock                     one writer at a time
  objects/<uuid>.json      the authority        commit this
  events/<uuid>.jsonl      append-only confirmed history
  candidates/<CODE>.json   awaiting a human     never commit this
```

`init` MUST write a `.gitignore` excluding `lock` and `candidates/`. A candidate's
filename is a live challenge code, and `git add -A` is how a workspace gets
staged: committing one hands the code to everyone with repository access, which
is not where a code the gate expects a single human to return is supposed to go.
The exclusion MUST NOT cover `objects/`, since look-back is delegated to git.

Events are safe to commit. The challenge codes they carry have been spent, and a
spent code resolves to no candidate.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | success |
| 2 | invalid usage, or a confirmation response that did not match |
| 3 | object, section, or candidate not found |
| 4 | malformed or unsupported stored data |
| 5 | a rule of the model was violated |
| 6 | the object moved after the candidate was prepared |
| 8 | filesystem, locking, or external tooling failure |

## Growth rule

Add to this protocol only when a real, recorded use needed it and working around
it cost more than adding it.

Of the 48 event types in the previous design, 35 never fired once during the only
day it was genuinely used. Deliberately absent, each with the signal that would
bring it in:

| Absent | Signal |
| --- | --- |
| A `kind` on sections | `show` becomes unreadable without grouping |
| Section ordering | A document has to be generated, or a merge has nowhere to sit |
| Object-to-object relations | A dependency that belongs to no particular section |
| Typed relations | Something needs to act on the type mechanically |
| Machine observations (test results, progress) | Those need to be in the record |
| Splitting `closed` into done and abandoned | Needing to count them apart, or to ask why something was dropped |
| Statuses between open and closed (`deferred`, `blocked`) | A record is neither being worked on nor settled, and calling it one of the two loses something someone needed |
| A priority on an object | Which record matters more has to be answered mechanically — a listing has to order by it |
| Path scoping on a section (`--about internal/audit/**`) | A basis reads as moved because of a change to an area the section does not cover, often enough that the signal stops being read |
| A human-chosen short id (`AUD-3`) | A uuid prefix misdirects someone in speech or in a commit message |
| More than one action per confirmation | One piece of work needs the same object prepared and confirmed three times over, and the human says so |

Splitting `closed` is the nearest of these, and part of its signal is already
visible: abandoned work can only be closed, so the record goes on reporting a
moved basis for something nobody intends to return to. That is a false alarm in
the signal this design exists to keep believable. Any new status MUST answer the
two questions the existing states answer — does it refuse section actions, and
does it earn the closed-and-drifted alarm — and `abandoned` is so far the only
candidate whose answers differ from `closed`'s.

A priority would be a decision like any other and would go through the gate. The
test for whether it belongs is whether a human wants to be asked to confirm a
change to it: if confirming it would be tiresome, that is the signal it is
tracker data, and admitting it would need a second write path — which is the one
rule. An estimate of size is absent for a different reason and is not expected
back: it is a guess about the future rather than a record of something agreed,
and nothing would ever bring a human to re-confirm it.

Path scoping would have to be a section field carrying into the content hash —
what a section is *about* is part of what was confirmed — and it MUST then be
omitted from the canonical form when empty, or every section already recorded
fails its own hash the day the field is added.

More than one action per confirmation MUST NOT be reached by allowing several
live candidates for one object: each pins `expected_rev`, so confirming one
kills the rest. It would have to be one candidate carrying several actions,
appending an event per action with consecutive revs — which leaves projection
untouched and keeps crash recovery working off the first event.

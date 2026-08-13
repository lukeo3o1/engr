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
├── last_projection_commit
└── sections[]
    ├── id                    integer, never reused, never renumbered
    ├── text                  always the current wording
    ├── based_on              the commit this wording was written against
    ├── refs[]                { object, section, sha256, commit }
    ├── sha256                hash of text + based_on + refs
    └── confirmed_at
```

A section's `text` is always its current wording, because wording only changes
through a confirmed action. Readers never have to ask which of two fields is
authoritative — there is only one.

### Sections are the authority; events are a buffer

`.engr/events/<id>.jsonl` holds confirmed actions until they are projected into
sections, and may then be discarded. **History is delegated to git**: objects live
in the repository and are committed, so `git show <commit>:<path>` recovers any
earlier wording.

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
every outside reference to it — which is why the counter must survive a purge,
where `max(existing) + 1` would hand out a deleted id.

## Actions

Seven. All of them gated.

| Action | Data | Effect |
| --- | --- | --- |
| `object_created` | — | Creates the object with the confirmed title |
| `section_added` | — | Appends a section, id from the counter |
| `section_revised` | `section` | Replaces that section's content; id unchanged |
| `section_merged` | `absorbs[]` | New id carrying the confirmed wording; absorbed sections removed |
| `section_deleted` | `section` | Removes the section |
| `object_closed` | — | `status` → `closed` |
| `object_reopened` | — | `status` → `open` |

`object_created`, `section_added`, `section_revised` and `section_merged` carry
content; the others must carry none.

A **closed object refuses every section action**. Reopen it first. The friction
is deliberate: if a closed object could still change, `closed` would not mean
"this has settled" and could not be used as the signal that it is safe to purge.

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
being pinned. Deferring reference checks to `verify` is what let one mistyped id
in the previous design poison a global health check permanently, with no way back.

There is **one live candidate per object**. Preparing again supersedes the
previous one, so a human never holds two codes for the same thing.

### The title

An object's title is a label, not a body. Creating an object MUST refuse a title
that spans lines or exceeds 120 characters, and the refusal MUST say where the
detail belongs instead. The field is unforgiving — v0 has no rename — so a body
pasted into it is only discovered after confirmation, and rebuilding the
workspace is the way out.

That check belongs at the gate, **not** in payload validation: payloads are
validated when events are *loaded*, so a limit enforced there would leave a
workspace holding an over-long title unable to replay its own history.

Titles are not unique and are not required to be. A duplicate MUST be reported
with the candidate and MUST NOT be refused — two objects may legitimately share
a title, but they cannot be told apart in a listing, and the moment to
reconsider is while the human is still holding the code.

The candidate records `expected_rev`. A candidate prepared against an older state
cannot be confirmed.

### What a human is shown

The **change**, not the whole section again — the previous wording and the new
wording. Requiring a full re-read on every revision is how confirmation decays
into rubber-stamping.

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

Both are reported as **information, not a verdict**. A threshold nobody has
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

## Purge

`purge` discards one object's event buffer and records `last_projection_commit`.

It is **not gated**: it changes nothing a human confirmed, so it is garbage
collection rather than a semantic act. The guard is mechanical instead — purge
**refuses unless every event being dropped is already reflected in the sections**,
because silently dropping an unprojected event would lose confirmed content.

When to purge is a human judgement, not a threshold: the buffer has grown large,
or the object has settled. A `closed` object is the obvious candidate.

## Verify

`verify` recomputes each section's hash from what is stored, and the hash of
each section those sections reference.

It catches a section edited without recomputing the hash. It **cannot** catch an
edit that recomputes the hash too: once events are purged, the hash sits beside
the content it covers. Committed git history is the real tamper anchor, which is
why `verify` also reports an uncommitted object.

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

## Layout

```text
.engr/
  format.json              workspace format and version
  .gitignore               excludes lock and candidates/
  lock                     one writer at a time
  objects/<uuid>.json      the authority        commit this
  events/<uuid>.jsonl      the buffer, purgeable
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

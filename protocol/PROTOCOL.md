# engr protocol v0

Normative. Where this document and the implementation disagree, this document is
wrong and should be fixed, or the implementation is a bug — say which.

## The one rule

**Nothing enters the record that a human has not read and confirmed.** There is
no unconfirmed write path **into the record**. Every action goes `prepare` → the
human reads the change → `confirm` with the exact phrase.

The workspace also holds **backlog**, which is explicitly outside the record and
carries no such guarantee. The scope of the rule is what makes both possible: a
place to put unresolved work, without the record having to mean less.

## Model

An **object** is an aggregate. It holds **sections**, each carrying text.

```text
object
├── id                        uuidv7
├── title
├── type?                     design | decision | risk, absent by design
├── state                     one field, valid for the type
├── rev                       increments on every confirmed action
├── next_section_id           monotonic, never reset
└── sections[]
    ├── id                    integer, never reused, never renumbered
    ├── role?                 decision | risk | supersession | acceptance_criterion
    ├── text                  always the current wording
    ├── content[]?            bounded literal excerpts, ordered
    ├── based_on?             committed repository context, absent by explicit choice
    ├── refs[]                { object, section, sha256, commit }
    ├── relations[]?          { type, target }
    ├── sha256                hash of role + text + content + based_on + refs + relations
    └── confirmed_at
```

A section's `text` is always its current wording, because wording only changes
through a confirmed action. Readers never have to ask which of two fields is
authoritative — there is only one.

Every optional field above is **absent when it carries nothing**. An empty
`content[]`, an empty `relations[]` and a null `role` are not stored, so a
section using none of them is byte for byte what it was before those fields
existed — and hashes to the same value.

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

Ten. All of them gated.

| Action | Data | Effect |
| --- | --- | --- |
| `object_created` | — | Creates the object with the confirmed title |
| `object_renamed` | — | Replaces the title |
| `section_added` | — | Appends a section, id from the counter |
| `section_revised` | `section` | Replaces that section's content; id unchanged |
| `section_merged` | `absorbs[]` | New id carrying the confirmed wording; absorbed sections removed |
| `section_deleted` | `section` | Removes the section |
| `object_closed` | — | `state` → `closed`; untyped objects only |
| `object_reopened` | — | `state` → `open`; untyped objects only |
| `object_classified` | `type?`, `state` | Sets both, explicitly |
| `object_superseded` | — | Appends the reason, records the replacement, `state` → `superseded` |

`object_created`, `object_renamed`, `section_added`, `section_revised`,
`section_merged` and `object_superseded` carry content; the others must carry
none — no text, no basis, no refs, no role, no supplementary content, no
relations.

An object that **does not need attention refuses every section action, and a
rename**. Move it back into the attention set first. The friction is deliberate:
if confirmed knowledge could still change while nobody was looking at it,
`accepted` would not mean "this has settled". A title is part of what settled,
so exempting it would narrow that to "the sections have settled" rather than the
whole object. For an untyped object this is exactly the old "reopen it first"
rule, unchanged.

The rule is about **renewed engineering work** — wording confirmed once being
changed again out of sight of everyone reading the default listing.
`object_superseded` is therefore exempt, and the exemption is the rule rather
than a hole in it: superseding is not resumed work on the object, it is the act
of retiring it, and the object it exists for is an `accepted` design or decision
that a newer one replaced. Requiring a reclassification first would confirm an
intermediate state the object was never in, and would split into two
confirmations the one operation this protocol requires to be atomic. Superseding
an object that is already superseded is refused by the coupled invariant, which
would count two replacement relations — not by a lifecycle sequence, because v0
defines none.

Sections have no `state` field. Deletion deletes and merging merges, so every
section in the list is by definition current — there is no state to represent.

A merge must absorb at least two distinct sections.

## Type and state

`type` is optional, and an untyped object is a **first-class long-term form**,
not a waiting room for classification. Most confirmed knowledge is just
knowledge.

There is exactly one persisted lifecycle field. Which values it may take depends
on the type:

```text
type absent    open | closed
design         draft | proposed | accepted | rejected | superseded
decision       proposed | accepted | rejected | superseded
risk           identified | accepted | mitigated | invalidated
```

The vocabularies are deliberately **not symmetric**. A risk being `accepted` and
a decision being `accepted` are different facts, and one shared enum would
flatten that distinction. A design has `draft` because a design is formed over
time; a decision does not, because a decision that is not yet formed belongs in
backlog rather than in the record.

A new untyped object is `open`. Nothing else is assigned by default anywhere.

`object_classified` MUST carry both halves: the destination type, or its
explicit absence, and a state valid for that destination. There is **no
automatic mapping** between vocabularies — `design.accepted` becoming
`decision.accepted` would be engr making a judgement nobody asked it to make.

v0 defines **no transition graph**. Any destination is reachable when it is valid
for the destination type and the invariants below still hold. An invented
lifecycle sequence would be a process the protocol has no authority to impose.

`object_closed` and `object_reopened` remain, and remain narrow: they are the
Phase 0 spelling of the untyped vocabulary and are what every confirmed event in
an existing workspace says. On a typed object they have no meaning and MUST be
refused.

### Attention

Attention is **derived from `(type, state)` and never stored**:

```text
untyped     open        needs attention
            closed      no
design      draft       needs attention
            proposed    needs attention
            accepted    no
            rejected    no
            superseded  no
decision    proposed    needs attention
            accepted    no
            rejected    no
            superseded  no
risk        identified  needs attention
            accepted    no
            mitigated   no
            invalidated no
```

A stored `attention` field would be a second truth that disagrees with `state`
the moment somebody edits one of them; the whole reason there is one lifecycle
field is to have no such pair.

Default `engr ls` and planning diagnostics MUST use derived attention rather than
assuming `open | closed`. `--all` includes everything.

No attention does not mean finished, correct, approved, or immutable. It means
this is not in the default set of things somebody is currently looking at.

## Section roles

Optional, and the vocabulary is exactly:

```text
decision
risk
supersession
acceptance_criterion
```

A role is independent of the object's type: an untyped object may hold a section
with `role = decision` without itself being a decision important enough to have
its own identity and lifecycle. v0 defines no `type × role` compatibility matrix,
and adds one only if a real combination proves meaningfully invalid.

`acceptance_criterion` states a **verifiable condition, not its verification**.
It MUST NOT gain `passed`, `failed`, `pending`, `waived` or any other local
lifecycle. Whether a criterion currently holds is evidence: it changes without
anyone confirming anything, and putting it here would make the record assert
something no human read.

Because a role changes the machine-readable meaning of confirmed wording, it is
authoritative content: it passes the gate and it is inside the section hash.

## Relations

`relations[]` is section-owned. The vocabulary is exactly:

```text
superseded_by
implemented_by
```

A relation is a typed semantic edge, and each type defines its own legal targets.
This is not an arbitrary-string knowledge graph.

`relations[]` is not `refs[]`. A ref is a **wording dependency** and drifts when
its target is reworded; a relation says what this assertion relates to, and
inherits none of that drift behaviour. Nor is it backlog `subjects[]`, which is
weak, unconfirmed navigation.

### `superseded_by`

```text
target      an existing Object, kind=engr, obj:<id>, no section selector
            never the source Object
            no source/target type compatibility requirement
            no target state requirement
graph       MUST be acyclic
```

The coupled invariant:

```text
state = superseded
  if and only if
exactly one valid superseded_by relation exists on the object
```

Both directions, checked after every projection and wherever an object is loaded.
They are one fact written in two places, so either without the other is a record
that contradicts itself: a superseded object with nothing to forward a reader to,
or a replacement edge the state does not honour.

Supersession is therefore **one atomic semantic operation**, `object_superseded`,
confirming together:

```text
state = superseded
the replacement relation
a human-readable Section with role = supersession
```

A `superseded_by` relation MUST NOT enter through any other action. Splitting
these into separately confirmable steps would be easier to implement and would
mean a record can sit in the state that says it was replaced while being unable
to say by what.

Two consequences follow from the tables rather than from any separate rule, and
both are deliberate. `superseded` is not in the untyped or risk vocabularies, so
only a design or a decision can be superseded — and by the coupled invariant, only
a design or a decision can hold the relation. And nothing removes the pair: see
[What v0 does not solve](#what-v0-does-not-solve).

### `implemented_by`

```text
file target     path + full committed Git object id; path MUST exist at that commit
symbol target   path + symbol + full committed Git object id; path MUST exist at that commit
```

The symbol itself is **not** resolved. v0 does not parse the languages a
repository is written in, and a check that only worked for the ones it could
parse would be worse than none — failing on real code and passing on the rest.

It is implementation-artifact provenance, not a wording dependency, so a section
carrying one is never reported as stale because the file moved on.

## Supplementary content

`text` remains the complete human-readable engineering assertion. A section MUST
be understandable from `text` alone.

`content[]` holds bounded literal excerpts — the code, configuration or data the
assertion needs in order to be precise:

```text
type    ^(code|data)\.[a-z0-9][a-z0-9-]{0,15}$
body    non-empty UTF-8 literal content
```

There is deliberately no `text` content type, and no `markdown`, `note`, `todo`
or `mixed`. Natural-language assertion already has exactly one home, and a second
container for prose is how a section becomes a blob.

The tag is **not a registry**. An unknown but well-formed tag is valid and MUST
survive a round trip untouched; engr does not normalize `yml` to `yaml`, because
an alias table is a maintenance surface with no authority behind it.

A content entry has no id, no state, no refs, no relations and no confirmation of
its own. Changing one is an ordinary revision of the containing section, which is
what keeps the section the single unit of authority, hashing, revision and
reference. Duplicate types are allowed; an empty `content[]` is not stored.

A body is **literal**, so its internal whitespace and indentation are part of the
content and MUST NOT be altered. Whitespace at the very **end** of a body is the
one exception: admission trims it, before the payload is hashed and before a
human is shown it. The reason is the gate rather than tidiness — trailing
whitespace is invisible on a terminal, so `"x"`, `"x\n"` and `"x   "` would
otherwise be three payloads with three different section hashes that a human
reads identically, and `--content-file` makes that the ordinary case rather than
the odd one, because text files end in a newline. A body that was nothing but
whitespace becomes empty and is refused. Nothing is trimmed on the read path, so
every hash written before this rule stays valid, and the "nothing to confirm"
comparison MUST normalize the stored body the same way it normalizes stored sets.

### Bounds

Counted in **Unicode scalar values**, not bytes, because the limit is about how
much a human is being asked to read.

| | normal | hard |
| --- | --- | --- |
| `text` | 1200 | 5000 |
| `content[]` entries | 4 | 8 |
| each `body` | 2000 | 8000 |
| sum of bodies | 4000 | 12000 |

Above a **normal** threshold, the first `prepare` MUST refuse, and the refusal
MUST say where the material belongs rather than only that it is long: another
section for an independent engineering point, backlog for unresolved reasoning,
an `implemented_by` relation for actual implementation, and outside the record
for a large log with only the smallest relevant excerpt kept. An **explicit
oversize retry** may then go through the normal confirmation flow, and the
candidate MUST show the exception where the human reads it.

"First" is a requirement on the implementation, not advice to the caller. An
oversize exception MUST be admitted only as the retry of a refusal engr actually
issued, for that same proposal — identified by the payload hash, so the same
wording against a different basis is a different proposal and earns its own
refusal. An exception over content that breaks no normal threshold MUST also be
refused: there is nothing to except, and a flag that is harmless to pass is a
flag an agent passes by default. How the refusal is remembered is local, and it
MUST NOT be part of the record or of anything shared between machines.

Refusals are tracked **per proposal, not per workspace**. A workspace holds work
on many objects at once, and considering a second proposal MUST NOT revoke the
first one's refusal — an agent that has already done what the rule asks would
otherwise be sent back to do it again for a reason that has nothing to do with
its proposal. Spending one exception leaves every other outstanding refusal
alone. The set may be bounded; forgetting costs an extra refusal, which is the
safe direction.

Above a **hard** ceiling, engr always refuses. The hard ceiling is not a
threshold with an override, and MUST NOT read as one, or an agent that learned to
add the flag for the first would add it for the second.

The exception is **admission-time only**. It travels with the candidate, is
covered by candidate integrity, and is never persisted: no `oversize` field on a
section, no lasting exemption. Every revision is measured again against its own
proposed value.

## Sets and order

```text
refs[]        semantically unordered
relations[]   semantically unordered
content[]     ordered
```

Exact duplicate refs and relations are invalid, on the way in and wherever a
stored payload is read. Two refs to the same section pinned at different wording
are different statements and remain valid.

Reordering a set alone MUST NOT count as a semantic change. engr canonicalizes
the order of `refs[]` and `relations[]` at the gate, before the payload is
fingerprinted and before a human is shown it — so the same members written
another way round produce the same section hash, and a revision that changes
nothing else is refused as having nothing to confirm. No canonical persisted sort
order is required of anyone hand-writing these files, and none is enforced on the
read path, which is why every hash written before this rule existed stays valid.

That last point has a consequence the "nothing to confirm" check MUST honour, and
it applies equally to the trailing-whitespace normalization of a body. A
section stored before these rules holds whatever order its gate wrote, so the
comparison MUST canonicalize the **stored** value too, and only for the
comparison. Otherwise re-proposing the same members against such a section finds
a difference that the model says is not one, and spends a confirmation and a
revision on sorting an array. The stored section is left exactly as it is: its
hash covers the order it was written in, and rewriting it to tidy that order
would be the same non-change from the other direction.

`content[]` is ordered because its entries are excerpts a reader goes through in
sequence. Moving one is a change to the assertion.

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

### Candidate integrity

A candidate stores two fingerprints. `payload_sha256` identifies the mutation:
it travels into the confirmed event, and an already-applied retry is recognised
by it, so **its input may never widen**. `integrity_sha256` covers that value
together with the challenge and the whole prepared context — the binding, the
previous wording a revision is diffed against, and any backlog declarations.
Every load of a candidate MUST check both, not only admission: re-rendering a
candidate hours later is as much a use of its prepared context as confirming it
is.

Without the second value those fields sit outside every check while still
deciding what the human is shown and what confirmation does. This is **not** a
boundary against someone who controls the machine — the file is on that machine
and so is the binary. It is the narrower guarantee that a candidate rewritten on
disk cannot present or bind a different confirmation context and still pass.

The challenge is covered because it is the link between the two halves of the
gate: what a human is shown, and what their answer admits. A candidate MUST
additionally be refused unless the challenge it stores is the one it was looked
up by. Otherwise, with two candidates live, rewriting one file's challenge to
the other's code makes it render its own change while naming the other's answer
— and both files remain internally consistent, so nothing else catches it. What
enters the record would then be a change nobody read.

An envelope that cannot carry that guarantee MUST be refused and re-prepared,
never read as if absence meant protection. Candidates are local, uncommitted and
short-lived, so this costs a moment.

### What a human is shown

The **complete semantic change**, not the whole section again. Revisions use a
unified line diff with limited unchanged context and separately show old/new
`based_on`, old/new `role`, added and removed refs and relations — including
their pinned hashes and commits — and supplementary content entry by entry,
against the entry that held the same position. An explicit absence of repository
basis is displayed as such. Omitted unchanged wording remains part of the
complete candidate payload and confirmation hash.

A supplementary content body is shown **in full whenever it is added or
removed** — the whole body, exactly as it is being admitted, never only its type
and never trimmed by the renderer. A body that *changed* is shown as a unified
line diff against the previous body, the same presentation section text gets.
They are part of the assertion and part of what is hashed, so a human shown only
the type of an entry has not read what they are about to admit, and duplicate
types are valid, so a heading alone names a position rather than a thing. The
bounds exist precisely so that printing a whole body stays reasonable.

"Exactly" is load-bearing: a renderer that trimmed could draw two payloads with
two different section hashes the same way, which is the one thing this gate
exists to prevent. Trailing whitespace is removed at **admission** instead, where
it changes the value actually hashed — see [Supplementary
content](#supplementary-content).

`object_classified` shows the destination type, the destination state, and what
that does to the object's place in the default listing. A state without its type
is a word that means different things on different objects, and the attention
consequence is the thing actually being decided.

An oversize exception is shown before the wording it applies to, so the human
knows engr already refused this once while there is still a decision to make.

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

## Backlog

Where unresolved engineering work waits. It is **not a weaker record** — it is
outside the record entirely.

| | Record | Backlog |
| --- | --- | --- |
| Admission | human confirmation | none; agents edit it directly |
| Authority | current confirmed wording | none |
| History | append-only events, and git | git |
| Integrity | section hashes, tamper alarms | schema validation only |

Committing backlog means only *this was the working thought stored here at that
point*. It never means anyone agreed to it.

Weak guarantees are not absent ones. Backlog storage is schema-validated, and
loading MUST enforce everything writing enforces: a topic that is present,
single-line and label-sized, section text that is not blank, an `updated_at`
that is a real RFC3339 timestamp, and the id rules below. Two validations that
disagree mean the stricter one is decorative — staging is hand-edited by design,
and the shape only has to survive one edit to stop being true. Malformed stored
data is a schema fault; the same value typed at a command line is a usage one,
and the two MUST NOT share an exit code.

That covers the shape as well as the values. A stored backlog resource carrying
a field outside the current schema MUST be refused as a schema fault, not loaded
with the field ignored. Ignoring it is the worse outcome: engr would report the
workspace as valid and then drop the field on the next ordinary rewrite, having
silently edited data whose shape it claimed to understand. `status` and
`resolved` are the cases that matter most, because the lifecycle below says
existence is the only signal there is — and `format` and `version` are refused
for the same reason no resource carries them, since `.engr/format.json` is the
sole schema authority.

```text
backlog item
├── id                  uuidv7
├── topic               what the unresolved work is about
├── next_section_id     monotonic, never reset
└── sections[]
    ├── id
    ├── text
    ├── updated_at
    ├── subjects[]      what this point concerns
    └── produced[]?     confirmed outcomes so far
```

Sections, rather than one blob, because a topic commonly holds several
independent concerns and a blob forces unrelated points to move together.

### The lifecycle is the whole model

```text
a Section exists    = still unresolved
a Section is gone   = somebody judged it resolved
```

There is no `status`, `resolved`, `promoted`, `partial` or `abandoned`, and none
should be added to retain settled entries. A field that can disagree with those
two lines is a field that lets finished work go on looking pending, which is the
failure backlog exists to prevent. Removing the last Section removes the item:
an item is a topic that still has unresolved work in it.

Section ids are monotonic and never reused, for the reason the record's are —
`max(existing) + 1` would hand back the id of a consumed Section and silently
repoint every subject aimed at it.

A confirmed section never moves back into backlog. The confirmed wording remains
the last-admitted wording until another confirmed revision replaces it; the
doubt goes into backlog instead, and is later settled by a normal record action.

### subjects[]

*This unresolved point concerns these things.* Deliberately weaker than `refs[]`:
no dependency, no authority, no ordering, and no claim the target must change.

An `engr` subject may name an Object, an Object Section, another backlog item or
one of its Sections. **Backlog-to-backlog cycles are valid** — this is a
navigation relation, not a dependency graph. Authoritative `refs[]` MUST NOT
gain the ability to target backlog: a confirmed section cannot stand on wording
nobody read. The asymmetry is the point.

A `file` or `symbol` subject pins a path and a full resolved commit. Where the
caller does not choose a committed revision and the path is dirty, engr MUST
refuse rather than pin HEAD — HEAD would not describe what was actually read.
The path MUST exist in the commit pinned. Backlog is allowed to be unresolved;
it is not allowed to claim provenance it does not have. Symbol identity is a
path and a human-readable name; no language-specific resolution is attempted.

A subject that later stops resolving is a stale signpost, and MUST NOT make the
item unreadable. Backlog is staging, not a referential-integrity database.

`subjects[]` is semantically **unordered**, and exact duplicates are refused so
that "the same set" has one meaning. No persisted sort order is required.

### updated_at

When the unresolved statement itself last changed: creation, text revision,
subject changes, a merge result. A topic rename MUST NOT refresh it, and neither
does appending a produced outcome — an outcome deliberately does not change what
remains unresolved, and refreshing would make an untouched point look worked on.
Item-level activity is derived from the Sections rather than stored, so the two
cannot disagree.

Neither does a write that changes nothing. Rewriting a Section with the wording
it already had, or with the same `subjects[]` set in a different order, MUST
leave `updated_at` alone. Order is not content — the resolution basis already
treats `subjects[]` as unordered — and an idempotent write that manufactures
activity puts an untouched point at the top of the list somebody reads to find
what was touched.

The value is an RFC3339 timestamp, and it MUST be compared and rendered as an
**instant**, never as text. RFC3339 carries an offset, so
`2026-08-17T01:00:00+08:00` sorts after `2026-08-16T20:00:00Z` while being
three hours earlier, and shortening a value by cutting the string at its
fractional seconds and appending `Z` reports a different moment entirely. Read
surfaces may normalize the offset for display; they may not change the instant,
and the stored value keeps its own precision and offset.

It is operational metadata and is **not** part of the resolution basis.

### produced[]

Authoritative knowledge already created or materially changed while working on
this point. Targets are authoritative Objects and Object Sections only; backlog,
collections, files and symbols are refused, because `produced[]` answers what
the *record* gained.

```text
produced.length > 0   DOES NOT MEAN   resolved
```

One unresolved point may produce several confirmed outcomes across several
sessions and still have work left in it. They MUST NOT be forced into one batch
confirmation so the point can be consumed. An agent resuming work should read
the text, the subjects and the produced outcomes together before deciding what
is left — that is what stops it re-solving what an earlier session settled.

A declared outcome asserts that authority exists, so `prepare` MUST refuse one
that names an Object or Section which will not exist once the candidate is
admitted. The check is against the **projected** state, not the stored one: the
usual outcome of working on an unresolved point is the very Object or Section
the candidate creates, and refusing that would make the field useless for its
own case. The candidate pins `expected_rev`, so that projection is exact.

Existence is checked when the claim is made and never again. Loading backlog
MUST NOT depend on a recorded outcome still existing: an Object deleted through
the gate afterwards is history, and history cannot be allowed to make the
staging around it unreadable.

### Resolution basis

The transient compare-and-consume token is

```text
canonical(text, subjects[])
```

excluding `produced[]`, `updated_at`, the Section id and the parent topic, with
`subjects[]` canonicalized as an unordered set. It is **not** stored on the
backlog Section: it is comparison state a candidate pins, not a trust hash.

If a produced outcome materially changes what remains unresolved, the agent
revises `text` or `subjects[]` — which moves the basis, as it should.

### Candidate-derived outcomes

A normal record action remains the thing that enters the record. There is no
`backlog_promoted` event and no second confirmation flow.

A candidate derived from backlog MUST explicitly declare its source Section(s),
the outcomes produced from each, and whether confirming settles each one. It
MUST NOT be inferred from the fact that an Object changed. `prepare` pins each
source's resolution basis and refuses up front if the source does not exist.

Those declarations are covered by candidate integrity and stay out of the event:
backlog is not part of the authoritative record.

After successful confirmation, per source:

| Basis since prepare | Candidate says | Result |
| --- | --- | --- |
| unchanged | still unresolved | declared outcomes appended to `produced[]` |
| unchanged | resolved | Section consumed; item removed if it was the last |
| **changed** | either | left untouched, and reported |

The third row is the one that matters. A stale source MUST NOT invalidate
wording a human already confirmed — the record mutation still admits — and an
old candidate MUST NOT mutate newer unresolved staging. Failing to reconcile
because the source moved is an expected outcome, not a failed admission.

A source declared resolved is consumed, so outcomes declared alongside it have
nowhere to be written and are not: the point is settled, and the outcome is in
the record, which is where it belongs.

Reconciliation MUST happen inside the same successful confirmation, holding the
same lock that made the mutation durable, so nothing can edit the source between
the basis check and the write. It MUST also be idempotent: appending an outcome
already listed does nothing, and a consumed Section is simply gone, so the retry
that closes the crash window applies none of it twice.

### Read surfaces

Backlog lives under an explicit namespace and every surface it prints MUST state
that it is unconfirmed. Structured output carries that as a field, because it
travels furthest from any banner.

`ls`, `show` and `verify` are record surfaces and MUST NOT mix backlog wording
into their output. `verify` stays record-oriented: staging validity is not part
of what a record `PASS` claims.

## What v0 does not solve

`prepare` prints the challenge code where the agent can read it, and the agent
runs `confirm`. **Nothing stops an agent confirming its own proposal.**

Treat `confirmed_at` and a matching hash as evidence about the *content*, never
as evidence that a human was present. Making the gate a mechanism needs the
challenge to travel where the agent cannot read it, or `confirm` to run in a
different process. That is not v0.

### Supersession is one-way

The coupled invariant closes in both directions, so there is **no way out of
`superseded`**. Reclassifying away from it leaves a replacement relation the
state no longer honours; deleting or rewording the section that holds the
relation leaves a superseded object with nothing to forward a reader to. Each
half is refused on its own, and no compound operation undoes both together.

This is stated rather than worked around, because working around it means
inventing a semantics for un-supersession that nothing has decided: whether the
record was wrong about being replaced, or whether the replacement was withdrawn,
or whether the two objects merged back — and those are different facts that would
want different rationale. A superseded object remains authoritative, addressable
and readable; what it cannot do is come back as current. If the knowledge is
current again, that is what a new object says.

The signal that would bring an operation in is a real case where the supersession
itself was the mistake, rather than the design being replaced changing again.

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

`.engr/format.json` is the sole schema/version authority for a current
workspace. New resource files do not repeat those fields. Phase 0 workspaces
already carrying version 1 are still recognized as transitional while any
Object uses `status`; workspaces without the authority may also be recognized
from their legacy resource markers. Either form remains read-only until
`engr migrate` is explicitly run. Migration changes only incompatible
representation (`Object.status` to `Object.state`) and preserves compatible
legacy markers and confirmed Event envelopes. Unknown or newer workspace
versions are never mutated.

Migration **classifies nothing**. `status = open|closed` becomes
`state = open|closed` with no type, because the stored record does not contain
enough to infer one and a guessed classification is an engineering judgement
nobody made. Classifying an existing object later is an authoritative change like
any other, and passes the gate with a state valid for the type it is given.
Adding the Phase 3 fields did not move the workspace version: they are absent
when empty, so a workspace that uses none of them is byte-for-byte — and hash for
hash — what it was.

```text
.engr/
  format.json              workspace format and version
  .gitignore               excludes lock and candidates/
  lock                     one writer at a time
  objects/<uuid>.json      the authority        commit this
  events/<uuid>.jsonl      append-only confirmed history
  candidates/<CODE>.json   awaiting a human     never commit this
  backlog/<uuid>.json      unresolved staging   commit this
```

Backlog is committed: git is its only history. A new optional directory is not
a schema change, so adding it does not move the workspace version — a workspace
holding no backlog is byte-for-byte what it was.

`init` MUST write a `.gitignore` excluding `lock` and `candidates/`. A candidate's
filename is a live challenge code, and `git add -A` is how a workspace gets
staged: committing one hands the code to everyone with repository access, which
is not where a code the gate expects a single human to return is supposed to go.
The exclusion MUST NOT cover `objects/`, since look-back is delegated to git.

Events are safe to commit. The challenge codes they carry have been spent, and a
spent code resolves to no candidate.

## References

Object and future Backlog identities remain UUIDv7 values persisted as standard
UUID strings. Their reference form encodes the canonical 128 UUID bits as
exactly 26 lowercase Crockford Base32 characters using
`0123456789abcdefghjkmnpqrstvwxyz`, without padding.

Local standalone forms are `engr:obj:<id>`, `engr:obj:<id>:<section>`,
`engr:backlog:<id>` and `engr:backlog:<id>:<section>`. A Git snapshot selector
may follow as `@<commit>`; it selects an as-of snapshot and is not identity.
Backlog `subjects[]` and `produced[]` name current resources, so they refuse it.
Embedded references omit `engr:` and pair their namespace-relative `ref` with
`kind: "engr"`. The shared parser owns syntax only: each caller decides which
resources and selectors are legal and what they mean. Repository-qualified
resolution is deferred.

Snapshot input may be abbreviated or symbolic, but it is unresolved input, not
canonical reference data. Canonical or persisted output MUST contain the full
resolved Git object ID; an unresolved selector cannot be rendered canonically.

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
| A backlog status (`resolved`, `partial`, `promoted`) | Something has to be kept in backlog after it is settled, and its absence loses information nobody can recover |
| A priority, owner or due date on a backlog item | Which unresolved point to take next has to be answered mechanically rather than read |
| Section ordering | A document has to be generated, or a merge has nowhere to sit |
| Object-owned relations | A relation that belongs to no particular section, and that no confirmed section can express naturally |
| More relation types | A real edge that `superseded_by` and `implemented_by` cannot express, where something needs to act on it mechanically |
| Workspace-defined types, roles or relation types | Discovery labels are needed badly enough to be worth weakening a vocabulary every reader currently agrees on |
| A `type × role` compatibility matrix | A real combination proves meaningfully invalid rather than merely unusual |
| Verification state on an `acceptance_criterion` | Nothing outside the record can say whether a criterion holds, which would first mean evidence has nowhere else to live |
| An operation that leaves `superseded` | The supersession itself was the mistake, rather than the design being replaced changing again |
| A `text`, `markdown` or `note` content type | Something needs prose that is genuinely not the assertion, often enough to be worth a second place for wording |
| Machine observations (test results, progress) | Those need to be in the record |
| Splitting untyped `closed` into done and abandoned | Needing to count them apart, or to ask why something was dropped |
| Untyped states between open and closed (`deferred`, `blocked`) | An untyped record is neither being worked on nor settled, and calling it one of the two loses something someone needed |
| A priority on an object | Which record matters more has to be answered mechanically — a listing has to order by it |
| Path scoping on a section (`--about internal/audit/**`) | A basis reads as moved because of a change to an area the section does not cover, often enough that the signal stops being read |
| A human-chosen short id (`AUD-3`) | A uuid prefix misdirects someone in speech or in a commit message |
| More than one action per confirmation | One piece of work needs the same object prepared and confirmed three times over, and the human says so |

Splitting untyped `closed` is the nearest of these, and part of its signal is
already visible: abandoned untyped work can only be closed, so the record goes on
reporting a moved basis for something nobody intends to return to. That is a
false alarm in the signal this design exists to keep believable. Any new state
MUST answer the two questions the existing ones answer — does it refuse section
actions, and does it earn the unwatched-and-drifted alarm — and `abandoned` is so
far the only candidate whose answers differ from `closed`'s.

A typed object already answers part of that: `rejected` and `invalidated` say
*why* something is out of the attention set, which is exactly the distinction
`abandoned` was reaching for. That is a reason to classify work rather than a
reason to widen the untyped vocabulary, and it is why the row above is now
narrower than it was.

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

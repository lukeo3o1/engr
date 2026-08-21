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

The guard reads the state the confirmation **arrives at**, not the one it left.
A no-attention object may therefore be revised in **one** confirmed operation
when that same operation atomically returns it to a state that needs attention.
The payload carries an optional `becomes`, applied before the action, and the
guard then sees the object back in the listing:

```json
{
  "action": "section_revised",
  "section": 3,
  "object": "...",
  "becomes": { "type": "design", "state": "proposed" },
  "text": "..."
}
```

`becomes` is admissible under **all three** of these conditions, and each is
enforced separately so that a refusal says which one failed:

1. The action is one the attention guard would otherwise refuse —
   `object_renamed`, `section_added`, `section_revised`, `section_merged`,
   `section_deleted`. An action that sets the object's own state
   (`object_created`, `object_closed`, `object_reopened`, `object_classified`,
   `object_superseded`) never carries one.
2. The object **does not currently need attention**. A destination is admissible
   *because* it is what makes the action legal; on an object already in the
   listing there is nothing to unblock, and a destination there would be a
   second, unrelated change hidden inside someone else's confirmation. Use
   `object_classified` for that, on its own, where a reader can see it.
3. The destination `(type, state)` is valid for the type and **needs
   attention**. That is the "only if" half of the rule.

The primary action and `becomes` are **one** confirmed operation: one candidate,
one confirmation, one event, one `rev` increment, and no intermediate
authoritative state. This exists so that no state an object was never really in
has to be confirmed on the way. The two-confirmation `reclassify, then revise`
path remains available and means something different: two authoritative
statements, because there were two. A payload with no destination serializes and
hashes exactly as it did before the field existed.

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

A body is **literal**. Every byte of it is inside the section hash and none of it
is normalized — not on the way in and not on the read path. `"x"`, `"x\n"` and
`"x   "` are three different sections, and a body of nothing but whitespace is a
body; only an empty one is refused.

That leaves a real problem, and v0 answers it as a **presentation** obligation
rather than by changing the value: trailing whitespace is invisible on a
terminal, so the confirmation display MUST say how a body ends whenever the way
it ends cannot be seen. See [What a human is shown](#what-a-human-is-shown).
Whether admission *should* normalize trailing whitespace instead is an open
question, recorded under [What v0 does not
solve](#trailing-whitespace-in-a-body-is-significant-by-default).

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

That last point has a consequence the "nothing to confirm" check MUST honour. A
section stored before this rule holds whatever order its gate wrote, so the
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
reference must resolve to an existing section whose current content matches what
is being pinned. Deferring reference checks to `verify` is what let one mistyped
id in the previous design poison a global health check permanently, with no way
back.

#### A reference pins content, not a claim about content

`section.sha256` and `refs[].sha256` are related and are **not** the same thing:

| | |
| --- | --- |
| `section.sha256` | the target section's confirmed integrity seal |
| `refs[].sha256` | the content this section was actually written against |

The pin MUST therefore be produced by **recomputing** the target's canonical
semantic content — the same representation the section seal covers, never a
second ref-specific or text-only hash — and never by copying the stored seal. A
seal is a claim about what was admitted; a section rewritten outside the gate
keeps its old seal while saying something else, so a pin copied from the seal
would record agreement to wording nobody confirmed.

Admission is ordered, and the order is load-bearing:

1. Load the effective target section.
2. Recompute its content hash.
3. Refuse unless that equals the target's own `section.sha256` — a target whose
   wording no longer matches its seal cannot be referenced at all.
4. Pin the recomputed value.
5. Refuse unless the target content at `refs[].commit` recomputes to it. An
   uncommitted target wording cannot be referenced.

So at the moment a reference is admitted:

```text
recompute(current target content)
  == target.section.sha256
  == refs[].sha256
  == recompute(target content at refs[].commit)
```

Content identity decides drift; git history explains it. `refs[].commit` remains
provenance and recovery, not identity.

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

### Naming a resource to engr

Every addressable entity MUST expose its canonical reference on a normal
**machine-readable** read path. A caller — an agent especially — MUST NOT have to
reconstruct one from the persisted identity, because that means reimplementing
the compact codec outside engr to use the tool's own flags.

```text
object            engr:obj:<26>
object section    engr:obj:<26>:<n>
backlog item      engr:backlog:<26>
backlog section   engr:backlog:<26>:<n>
collection        engr:collection:<10>
```

The requirement is the capability, not a spelling. The structured surfaces are
the contract; where the text surfaces also print it, that is presentation and may
change. **Identity, storage path and canonical reference stay distinct concepts**
— `id` remains the persisted identity and is not replaced by the reference.

A section selector is a **positive integer**. `:0` MUST be rejected by the shared
parser: section ids come from a counter that starts at 1, so `:0` names nothing
that can exist, and a reference that parses and round-trips while being
unresolvable by construction is worse than one that is refused. Whether a
resource kind supports a section selector at all stays a domain rule.

### What a human is shown

**What the change is being applied to**, first. The display MUST name the
section a `section_revised`, `section_deleted` or `section_merged` candidate
acts on, and MUST identify the Object by a name a reader would recognise as well
as by its id. Two sections may carry identical wording, and then a screen that
names neither renders two materially different mutations identically — while
section ids are never reused, so confirming the wrong one breaks every reference
pinning it with no way back. The payload has always carried the section inside
the confirmation hash, which is what stops `delete §3` becoming `delete §5`
after it was displayed; that guarantee is worth nothing if the display never
said which section it was.

Both MUST come from the prepared candidate, never from a fresh read at render
time. The section selector already travels inside the payload hash. A
recognisable name does not exist in the payload at all, so it MUST be
**snapshotted at prepare into the integrity-covered prepared context** — a live
lookup would put part of the confirmation identity outside the candidate, and a
name rewritten afterwards would change what a pending candidate presents while
its payload hash, its integrity value and its binding all still checked out. The
same candidate re-rendered later represents the exact context it was prepared
with, or the confirmation means less than it appears to.

A candidate that predates the snapshot carries no name and MUST render without
one rather than acquiring a current one.

Then the **complete semantic change**, not the whole section again. Revisions use a
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

"Exactly" is load-bearing, and it is not enough on its own. A renderer that
trimmed could draw two different authoritative values the same way, which is the
one thing this gate exists to prevent — but so does a terminal, for any body
ending in whitespace. So the display MUST also **name the ending it cannot
show**: whenever a body ends in whitespace, the presentation MUST state what
that trailing run consists of, counted rather than merely named, since three
spaces and two spaces are as different as anything else here. A body that is
entirely whitespace MUST be identified as such. When a revision moves only
trailing whitespace — which a line diff cannot show — both the previous and the
proposed ending MUST be named.

This applies to bodies already in storage, not only to newly admitted ones.
Nothing normalizes a body anywhere, so any workspace may hold these values, and
the human confirming their removal is confirming the exact literal.

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

## Absence is not failure

Shared resolution MUST keep **genuine absence** apart from **resolution
failure**. `NOT_FOUND` MUST NOT be collapsed with schema, invariant or
reconciliation faults, and a caller MUST be left with enough information to tell
them apart.

On authoritative trust and verification paths this is a hard rule: malformed or
unreadable authority MUST surface as a failure and MUST NOT be downgraded into
`not found`, `gone`, `moved`, or a clean verification. A reference whose target
will not load is reported as **`REF UNREADABLE`**, distinct from `REF TAMPERED` —
one says the words were changed behind the gate, the other says nobody can tell
what the words are, and both are failures rather than drift.

Non-authoritative domains may choose their own presentation — backlog subjects,
work targets and collection members each say "not found" or "unreadable" in
their own words — but they must be able to make that choice, which is why the
shared layer does not flatten it first.

## Integrity on the read path

Every read surface MUST recompute each section's hash before rendering it, and
MUST say so where the reader is already looking. A reading path that prints
unverified wording under an `ok` is worse than one that prints nothing: it is an
assertion, and this record's whole claim is that a human agreed to these words.

A section MUST also be marked when a section it references fails **its** hash.
Comparing `refs[].sha256` against the target's *stored* `sha256` cannot see this
— an edit that rewrites the target's text and leaves its stored hash alone moves
neither side of that comparison, so the reference looks untouched while the
wording under it was replaced. So the current identity a read reports MUST also
be recomputed from the target's content, and the two comparisons then say
different things:

| | |
| --- | --- |
| recomputed ≠ target's `section.sha256` | the wording was changed outside the gate — **tampered** |
| recomputed = seal, ≠ `refs[].sha256` | the target was revised through the gate — **drift** |
| recomputed = seal = `refs[].sha256` | unchanged |

Only the directly referenced section is checked; the target's own read covers
what *it* stands on.

Corruption outranks staleness. A section whose content does not match its hash
is not a section that drifted, and its drift assessment describes something
nobody confirmed, so the label MUST report the corruption and not the drift.

## Staleness

Two signals, both computed at read time, both needing nobody to be reading.

| Signal | Computed from |
| --- | --- |
| The basis moved | `based_on` versus HEAD: commits ahead, files changed |
| A dependency changed | `refs[].sha256` versus the target section's recomputed content hash |

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

## Work

**Execution memory an agent keeps for one object.** It answers "where does this
currently stand" and nothing else. Like backlog it is agent-managed, git-tracked
and confirmed by nobody; unlike backlog it is not a domain of its own but a
**sidecar** hanging off an object.

```text
work object
├── state                 active | paused
├── summary?              the shortest useful checkpoint
├── updated_at            RFC3339
├── next_item_id          monotonic, never reset
├── dependencies[]        required, may be empty; targets are unique
│   ├── target            required, engr:obj:<id> | engr:backlog:<id>
│   └── reason?           <= 200 scalars
├── blockers[]            required, may be empty
│   ├── reason?           <= 200 scalars
│   └── target?           engr:obj:<id> | engr:backlog:<id>
│   (at least one of the two MUST be present)
└── items[]               required, may be empty
    ├── id                integer, never reused
    ├── text              <= 160 scalars
    ├── state             pending | active | done
    ├── result?           <= 240 scalars
    └── commits[]         required, may be empty; unique full Git object ids
```

The four lists are **required and may be empty**: an omitted list and an empty
one MUST NOT be two spellings of the same sidecar. More generally, a stored work
object MUST be held to exactly what the write path can produce — a reader that
accepts shapes the API refuses is a second, larger schema that only ever gets
discovered by something that came to depend on it. A fault in a stored file is a
**schema** fault, not a usage error: nobody currently running a command wrote it.

`updated_at` MUST be a valid RFC3339 timestamp, and anything ordering work
objects by it MUST compare **instants** rather than text — two valid values
written in different offsets do not sort correctly as strings. The stored
spelling is preserved for display.

Stored at `.engr/work/objects/<object-id>.json`, one per object, carrying no
`format` or `version` of its own — `.engr/format.json` remains the single schema
authority. Most objects have none, and absence means only that engr holds no
operational memory.

A work object MUST correspond to an existing object, and that MUST be held on
**read** as well as on write. A sidecar names its object in its filename, so a
copied file can name one that never existed; an implementation that only checked
when writing would then read, list and hand back operational memory for nothing.
An orphan sidecar is invalid work, not a row with a missing title.

There is **no `engr:work:` reference**. Work is not an addressable resource,
nothing points at it, and it has no identity beyond the object it belongs to.

### It owns no authority

This is the whole of why it can live outside the gate.

```text
work never changes object semantic state
work is never promoted wholesale into the record
finishing every item settles nothing
```

An agent may complete every item it wrote and the object is exactly where it
was. If a result turns out to be stable engineering knowledge, it reaches the
record the only way anything does: `prepare`, a human, `confirm`. Unresolved
reasoning belongs in backlog. `summary` is a checkpoint, not a decision record, a
design analysis, a session transcript, or a copy of git history.

`ls`, `show` and `verify` are record surfaces and MUST NOT mix work into their
output, for the same reason they exclude backlog. Every work surface MUST say
what it is showing, **including the structured one**: a machine-readable
non-authoritative discriminator is required there, because JSON is the surface
that travels furthest from any banner and `{"state": "active"}` on its own is
indistinguishable from an object's own state.

Text surfaces must say more than backlog's banner does, because the failure worth
preventing here is not a reader trusting unconfirmed wording but a reader taking
a finished checklist for a settled object.

### `paused` is a human saying stop

```text
active    agents may keep advancing this on their own
paused    a human suspended it; agents must not resume it on their own
```

`paused` MUST NOT be inferred. Not from a session ending, not from a blocker, not
from an empty item list, not from an agent's own judgement that the work should
wait. An agent MUST NOT set it, and MUST NOT clear it, without explicit human
direction.

The same rule covers deletion: an agent MUST NOT delete a paused work object
without explicit human direction.

All of that is **normative on the agent**, not mechanical. engr cannot check it,
because it cannot tell an agent from a human — so it lives here and in the Skill,
exactly like the gate itself does. An implementation MUST NOT turn it into a
lifecycle rule by refusing the deletion: that would stop no agent willing to
clear `paused` first, it would make a human's own instruction impossible to carry
out directly, and it would invent a persisted transition whose only purpose is to
satisfy the refusal. What an implementation SHOULD do is make sure the signal
never disappears in silence — say, when a paused work object is deleted, that a
human's stop signal went with it.

Whether human direction should have a mechanical representation at all is an open
question; see [What v0 does not
solve](#work-has-no-mechanical-notion-of-human-direction).

A work object may otherwise be deleted freely once it no longer carries useful
handoff. Deleting says only that no operational memory is being kept; the object
is untouched. Completed items may likewise be pruned once they stop helping the
next agent. There is no archive: git holds what the sidecar used to say.

### Derived standing

```text
active,  no blockers   -> active
active,  blockers      -> blocked
paused                 -> paused
```

`blocked` is **derived and never stored**, for the same reason attention is: two
fields that can disagree eventually do. There is deliberately no `done` state at
the work-object level either, and that absence is load-bearing — a completed
sidecar must never become a second answer to "is this settled" competing with the
object's own state.

### Dependencies and blockers are different things

```text
dependency   a prerequisite this execution relies on; may hold while nothing is blocked
blocker      a condition currently preventing useful progress; may be temporary
```

They are not collapsed into one list. The same target can legitimately be both,
and when the blocking condition clears the dependency remains true.

A dependency MUST name a target, because one without a target says nothing
actionable. A blocker needs only one of reason or target, because real execution
is stopped by things that are not engr resources — an approval, an environment, a
vendor — and a blocker that could only be written as an edge would not be written
at all. An empty blocker is invalid.

Targets are **whole objects and whole backlog items only**. Not a section, file,
symbol, collection, or another sidecar: the finer the target, the more it reads
like `refs[]`, which pins wording and carries authority. Neither a dependency nor
a blocker ever becomes an authoritative relation, and neither is promoted
automatically.

### Item ids and commits

Item ids come from `next_item_id` and are **never reused**, including after
pruning. Handoff notes and conversations say "work item 3"; letting a later step
take that number would silently repoint every one of those sentences.

`items[].commits[]` are **navigation and evidence, never integrity anchors**:

```text
done          does not require a commit
a commit      does not mean done
unreachable   is a dead signpost, not a corrupt sidecar
```

Research and validation produce no commit; a rebase can make a recorded one
unreachable. This is deliberately weaker than `based_on` and `refs[].commit`,
which do anchor, and an implementation MUST NOT treat a missing commit as
corruption.

### Bounds

```text
summary          300
item.text        160
item.result      240
dependency.reason 200
blocker.reason    200
```

Counted in Unicode scalar values, like everything else. Unlike a section there is
no oversize exception: nothing here is authoritative enough to be worth admitting
past its limit. Text that will not fit has somewhere better to be — the
unresolved part in backlog, the settled part in the object.

## Collections

**Planning metadata: which work is grouped together, and in what order.** The
third domain outside the record, and the furthest from it — backlog holds
wording nobody confirmed, work holds progress nobody confirmed, and a collection
holds neither. It holds only the claim that some things belong together.

```text
collection
├── id                 10 lowercase Crockford Base32 characters, immutable
├── name               the line a listing prints
├── description?
├── state              open | completed | cancelled
├── schedule?          optional calendar context
│   ├── start?         YYYY-MM-DD
│   ├── end?           YYYY-MM-DD
│   └── target?        YYYY-MM-DD
│   (at least one MUST be present; if start and end, start <= end)
└── members[]          required, may be empty
    ├── target         engr:obj:<id> | engr:backlog:<id>
    ├── order?         intended sequencing; absent means unranked
    └── priority?
        ├── level      low | normal | high
        └── reason?    why it matters *in this plan*
```

Stored at `.engr/collections/<id>.json`, carrying no `format` or `version` —
`.engr/format.json` remains the single schema authority. The stored `id` MUST
match the filename.

Unlike work, a collection **is** an addressable resource:
`engr:collection:<id>`. The id is opaque on purpose — no date, no milestone
number, no type — because each of those is a fact that can stop being true while
an id cannot change. Renaming a plan does not make it a different plan.

### Grouping something means nothing about it

```text
membership never changes what a member means
priority belongs to the membership, not to the target
completing a plan is a declaration, not a proof
```

The same object may be `high` in one plan and `low` in another; a priority
stored on the object would make those two plans argue. `reason` is **planning**
rationale — why this matters here — and never engineering rationale, which has
one home and a gate in front of it.

`ls`, `show` and `verify` are record surfaces and MUST NOT mix planning into
their output. Every collection surface MUST say what it is, the structured one
included, with a machine-readable discriminator for the same reason work and
backlog carry one.

### State is declared, never inferred

```text
open        still being pursued
completed   the planner considers it finished
cancelled   no longer being pursued
```

Not derived from dates and not derived from members. `completed` does **not**
require every member to be resolved: a milestone can be finished with work
deliberately deferred or moved out of scope, and a plan that could only close
once everything in it had would be a plan nobody could close honestly. A reader
may derive a diagnostic such as "2 members need attention", and that is a
reading rather than stored state.

Diagnostics about members MUST speak in terms of **derived attention** rather
than `open`, because a typed object has no `open` state and a diagnostic phrased
that way would describe a vocabulary half the members do not have.

`completed` and `cancelled` stay distinct: one plan reached the end it aimed at,
the other stopped being pursued. Retention is a separate question — a closed plan
may stay readable for as long as it is useful, and `archived` MUST NOT be added
to the state enum to express that. Git holds what a plan used to say.

### Members

Whole objects and whole backlog items. Not sections, files, symbols, or other
collections: a plan groups work, and v0 has no hierarchy.

```text
the same target MUST NOT appear twice in one collection
non-null order values MUST be unique within one collection
```

Both are structural, and neither makes a collection authoritative. A rank that
two members shared would be a sequence with a tie it cannot break; unranked
members may of course share their absence, and a partly ordered plan is the
normal state of a plan. Array position is **not** an ordering — a reader sorts
by `order` and leaves the rest explicitly unranked.

A member whose target is later consumed or removed MUST NOT be silently
retargeted. Backlog resolution is not one-to-one: a point can be settled by two
objects, by none, or by something nobody recorded, and repointing the member at
whatever the work became would change what the plan says while nobody was
looking. Surface it, and let a human or agent update the plan explicitly.

### Schedule

Optional, and generic: with no collection type there is nothing to make the shape
depend on. All three values are **ISO calendar dates**, `YYYY-MM-DD`, with no
time and no offset — a collection carries planning context, not a schedule
somebody executes, and accepting a timestamp would claim a precision it does not
have.

No schedule value changes any state anywhere. `overdue` is a question a reader
asks, never something engr stores. A `target` need not fall between `start` and
`end`: a target before the end is an intention, not a contradiction.

### Deleting a plan

> An agent MUST NOT delete a collection unless explicitly directed by a human.

Normative on the agent, and **not** enforced — the same shape as the `paused`
rules on work, and for the same reasons. An implementation MUST NOT refuse the
deletion: it cannot tell an agent from a human, refusing makes a human's own
instruction impossible to carry out directly, and #10 explicitly defers a
stronger technical guard until real use shows one is needed. What an
implementation SHOULD do is report the planning context that went with it.

Ordinary create, rename, describe, state, schedule, member, order and priority
changes are agent-managed with no such rule.

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

### Work has no mechanical notion of human direction

`paused` is a human-directed stop signal, and several of its rules — do not set
it, do not clear it, do not delete a paused sidecar — are obligations on the
agent that engr cannot check. It has no way to tell an agent from a human, so
every one of those is a convention, exactly as the gate is.

The gate at least has a mechanism to lean on: a challenge code goes to one person
and comes back. Work has nothing equivalent, and inventing one here would mean
deciding what "explicit human direction" *is* as a representation — a confirmed
record of it? a second signal only a human can produce? — which #12 does not
define and which would pull Work back toward the gate it was deliberately put
outside of.

So v0 states the rules and reports what happens rather than preventing it. The
signal that would bring a mechanism in is a real case where an agent ignored the
rule and the loss mattered — at which point the question is what represents human
direction, not whether to refuse one operation.

### Trailing whitespace in a body is significant by default

`content[].body` is literal, and #14 says only that it is non-empty UTF-8. So
`"x"`, `"x\n"` and `"x   "` are three different sections with three different
hashes, a body of nothing but spaces is admissible, and a revision that only adds
a trailing newline is a real revision with something to confirm.

Whether that is right is **undecided**, and deliberately not decided here. The
case for normalizing trailing whitespace at admission is that a body read from a
file almost always ends in a newline, so two callers writing the same excerpt two
ordinary ways produce two different records for no engineering reason. The case
against is that normalizing redefines literal equality, payload identity and
"nothing to confirm" all at once — for a field whose whole point is being
literal — and #14 establishes no such exception.

v0 therefore keeps every byte and pays for it in presentation: the display names
the ending it cannot show, so the ambiguity never reaches the human. That closes
the confirmation risk without settling the semantics. Settling it needs an
accepted design decision on #14, not an implementation choice, because it changes
what is persisted for a given input.

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

## Project rules

Everything above is a mechanical invariant: id grammar, reference existence,
valid states, size ceilings, hash integrity. Those are decidable, so engr
decides them. A **rule** carries what is left — whether wording follows *this
project's* recording policy, whether an entry is really unresolved engineering
uncertainty, whether a plan follows the milestone policy. No schema answers
those.

So a rule is not a check engr runs. It is material an agent is required to have
read, named precisely enough that the requirement can be verified afterwards. It
proves nothing about comprehension and does not claim to; it makes silently
skipping the review impossible through the supported path.

Rules are **project policy data, not an authority domain**. There is no event
store, no candidate and no confirmation for a rule file: git is their history.
Changing one changes what the *next* mutation must be reviewed against, and
nothing already admitted.

```text
.engr/rules/*.md    YAML front matter + a normative Markdown body
```

`id` is the stable identity and the filename is only a locator, so renaming a
file does not create a different rule. **Duplicate ids fail closed**: two files
claiming one identity means the applicable set is not determinable, and a review
over an indeterminate set attests to nothing. Ids are `[a-z0-9-]+`.

Front matter is **standard YAML**, then a strict rule schema. The layers are
distinct — YAML decides syntax, engr decides whether the parsed document is a
rule — and a field this version does not understand is **refused rather than
ignored**, because reading past it would review against a rule only partly
understood. The normative body is stored exactly as written; emptiness is
decided by refusing, never by rewriting.

Applicability is domain-only: `object`, `backlog`, `collection`, `work`. What an
**empty** applicable set means belongs to the domain that owns the mutation, not
to this layer.

### How many attempts, and what happens after

```yaml
review:
  max_attempts: 5                      # optional, positive
  on_exhaustion: human_confirmation     # optional: reject | human_confirmation
```

Both fields are optional and both have **effective values**, so every rule
answers "how many attempts does this get" from the rule alone:

```text
max_attempts omitted    -> 5
on_exhaustion omitted   -> reject
```

There is no unlimited rule in v1. A rule may raise or lower the ceiling, and a
ceiling of `0` is refused: it is not a tighter limit but a way of spelling "never
reviewable", which the schema does not offer.

Exhaustion is `attempt > effective max_attempts`, so a ceiling of 5 leaves
attempts 1 through 5 reviewable and exhausts at 6. The attempt number is
**agent-attested process metadata**. engr does not count attempts and stores no
review series; it says what a given count means.

The **effective** values are what participate in review identity — not whether
the YAML happened to spell a default out. These two are one rule, and a binding
over either produces the same hash:

```yaml
review:                          review:
  on_exhaustion: human_confirmation      max_attempts: 5
                                         on_exhaustion: human_confirmation
```

That equality is the point. An author tidying their front matter must not
silently invalidate every attestation made against the rule, and two rules that
mean the same thing must not hash differently because one author was explicit.
The policy is nonetheless *in* the identity, because it decides the outcome: the
same wording under a ceiling of 5 and under a ceiling of 1 is not the same
review, and one that escalates to a person is not one that refuses.

One prepared mutation carries **one scalar attempt**, compared independently
against each applicable rule's own ceiling. There is no per-rule counter and
engr keeps no attempt state:

```text
prepare(exact mutation, attempt=N)

  rule A max_attempts=5, attempt=3  ->  reviewable
  rule B max_attempts=2, attempt=3  ->  exhausted
```

Which rules are past their ceiling is a mechanical fact and the same everywhere.
**What that means is the domain's, and the domains deliberately disagree.**

For an **Object**, an exhausted applicable rule stops the autonomous path. If at
least one *actually exhausted* rule asks for `human_confirmation` the mutation
escalates to the Human Gate, and otherwise it is refused. Escalation outranks
refusal among exhausted rules, because a rule naming a human is asking for a
decision rather than for the attempt to be discarded — and a human can still
decide to refuse. A rule that asks for a human but is not exhausted escalates
nothing: the action describes what happens at the ceiling, not a standing
property.

For the **Backlog**, exhaustion does **not** escalate and does not block, whatever
any exhausted rule's `on_exhaustion` says. The domain exists to hold unresolved
engineering intent, so the mutation is admitted and marked:

```json
"rule_review": { "attempts": 6, "limit": 2 }
```

`attempts` is the mutation-level attempt supplied; `limit` is the smallest
effective ceiling in the applicable set — the one that made this exhausted, since
a shared attempt passes the smallest ceiling first. It is a compact diagnostic,
not a review history: per-rule ids and limits are not recorded, because the
complete applicable set already lives in the review binding. A later successful
revision clears the marker; a later exhausted admission replaces it.

That soft-admission covers mutations that **preserve** unresolved information.
Consuming a Backlog Section is destructive, so it requires a review that actually
passed: an exhausted consume does not happen and the Section stays as it was.

**Collection and Work have no exhaustion behaviour in v1.** It is refused rather
than borrowed from another domain, because a composition that answers for a
domain nobody has decided is an invented rule that looks settled at the call
site.

Because the workspace version governs how a rule is read, **every path that
reads rule semantics enforces that version** — not only the commands. A workspace
at an older version is refused identically whether it is reached through the CLI
or through the library, so persisted meaning never depends on which door a caller
came through.

### What a rule rests on

```yaml
based_on:
  - path: AGENTS.md                    # the current material
  - path: docs/architecture.md         # the exact material it was written against
    commit: <full git object id>
```

Paths are **repository-relative** — relative to the repository top level, which
is what `git show <commit>:<path>` uses and need not be where `.engr` sits. Both
the current and the pinned side MUST resolve the same path the same way; two
roots for one stored path means a rule can be judged against a file it never
named.

A pinned basis stops being usable once the current file says something else. The
comparison is on **content, not commit ids**: a commit that did not touch the
path changed nothing the rule depends on, and staling for it would train
everyone to bump the pin without reading anything.

Missing paths, unresolvable commits, and a path absent at its pin all fail
closed. engr does not guess a rename.

### Rules and bases name real files

v1 **prohibits symlinks** for both `.engr/rules/*.md` and `based_on.path`, and
for the rules directory itself. A link is refused even when its target stays
inside the repository.

The reason is that a link does not denote one material. Git records a symlink as
a blob containing its target's *name*; a working-tree read returns the target's
*contents*. So a pinned basis over an unchanged in-repository link compares a
file's text against a filename and is stale forever, with nothing anyone can do
to the project to make it current. Rules are git-tracked policy, and a rule whose
bytes git does not hold is not a rule.

A rule entry and a basis must both be **regular files**. `.md` is a name, not a
kind: a FIFO so named makes a read block until someone opens the other end,
turning an entry nobody can commit into a workspace that cannot load rules at
all — a hang rather than an answer. Every one of these checks happens **before**
reading, which is while it can still be a refusal.

A **pinned** basis is checked against what git *recorded*, not only what git
prints. `git show <commit>:<path>` prints a symlink's target name as though it
were content, so a historical link whose target name equals a later regular
file's contents would compare equal and the pin would read as current across
exactly the change this prohibition exists to make visible. The tree entry mode
MUST be a regular blob.

A declared `commit` MUST name a commit object, not merely reach one. An
annotated tag peels to a commit, so a reachability check accepts its id while
the stored value is a tag id — a field specified as a commit id that quietly
holds something else is a persisted representation nobody can rely on reading
back.

A **broken** rules directory is not an absent one. Absence is an empty rule set;
a dangling redirection is a refusal, and following the link to decide would
report a workspace as having no policy when what it has is policy pointing
somewhere unreadable.

The prohibition covers **every component on the way to a rule file**, not just
the rule entry and the `rules` directory. A link at `.engr` redirects the whole
policy while leaving everything behind it well-formed, and git would then track
that link rather than the rule bytes — the same policy-versus-source mismatch,
reached one level higher. The check therefore anchors above every component it
validates.

## Layout

`.engr/format.json` is the sole schema/version authority for a current
workspace, and this build writes **version 2**. New resource files do not repeat
those fields. Workspaces at version 1 are recognized and migratable; workspaces
without the authority may also be recognized from their legacy resource markers;
a Phase 0 workspace is transitional while any Object uses `status`. Every one of
those forms remains read-only until `engr migrate` is explicitly run, and each
says which of them it is rather than being reported as one thing. Migration
changes only incompatible representation (`Object.status` to `Object.state`),
moves the authority to the current version, and preserves compatible legacy
markers and confirmed Event envelopes. Unknown or newer workspace versions are
never mutated and never read.

### What the workspace version is for

It governs how **persisted data is interpreted**, including data whose bytes do
not change. Version 2 exists because a project Rule gained `review.max_attempts`
and `review.on_exhaustion` *with effective defaults*: an unchanged rule file with
no `review:` block means one thing to a version 2 build and another to a version
1 build, and an explicit block is an unknown field to the older one. Two builds
accepting one workspace and disagreeing about what its policy says is the failure
this authority exists to prevent, so a build that does not know a version refuses
the workspace instead of reading it under its own rules.

This is distinct from the review binding's own version, which identifies the
deterministic binding contract and changes when *that* contract changes. A Rule
does **not** carry a schema version of its own; the workspace answers for it.

A prepared Rule Review attestation does not survive a migration that changes Rule
interpretation. It named a subject computed under the old semantics, so it is
stale by definition and must be prepared again.

A **historical** snapshot carries the version that was current when it was taken,
and is readable at any version this build recognizes. Refusing an older snapshot
would make every reference pinned before a migration unresolvable — moving the
workspace forward would retroactively break provenance that was correct when it
was recorded. This is safe only while the recognized versions represent the
resource identically, which is true of 1 and 2: version 2 changes how a Rule is
read, and no Rule is read out of a historical snapshot. A future version that
changes a resource representation must decode a snapshot under the snapshot's own
version rather than widening that check again.

Migration **classifies nothing**. `status = open|closed` becomes
`state = open|closed` with no type, because the stored record does not contain
enough to infer one and a guessed classification is an engineering judgement
nobody made. Classifying an existing object later is an authoritative change like
any other, and passes the gate with a state valid for the type it is given.
Adding the Phase 3 fields did not move the workspace version: they are absent
when empty, so a workspace that uses none of them is byte-for-byte — and hash for
hash — what it was.

### Event versions are semantic compatibility generations

An Event envelope version identifies a **generation of meaning**, not a count of
schema revisions. The test is whether two readers could both accept the same
Event and derive different authoritative meaning from it.

An additive change stays in the current version when an older reader either
interprets the Event without changing what it authoritatively means, or does not
understand the addition and **fails closed before accepting or replaying a
different meaning**. Rejecting an Event you do not understand is not
disagreement about what it means — it is the absence of a second opinion.

The version MUST change when an existing field or action changes meaning; when a
valid representation is removed in a way that changes what readers accept; when
an incompatible required field or envelope shape appears; when canonicalization
or hashing semantics change incompatibly; or when an older and a newer reader
could both succeed on one Event and disagree about it.

`becomes` is additive and optional. Its absence preserves the previous meaning
exactly, and an older reader confronted with one fails closed on the
confirmation hash rather than replaying a revision while missing the
classification that made it legal. It therefore stays within Event version 1.

Confirmed history is never rewritten to normalize versions. An Event says what
it said when a human confirmed it.

```text
.engr/
  format.json              workspace format and version
  .gitignore               excludes lock and candidates/
  lock                     one writer at a time
  objects/<uuid>.json      the authority        commit this
  events/<uuid>.jsonl      append-only confirmed history
  candidates/<CODE>.json   awaiting a human     never commit this
  backlog/<uuid>.json      unresolved staging   commit this
  work/objects/<uuid>.json execution memory      commit this
  collections/<id>.json    planning metadata      commit this
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
| A collection type (`milestone`, `roadmap`) | Real behaviour depends on which kind of plan it is, rather than the two merely reading differently |
| Collection hierarchy | A plan has to contain a plan, and expressing it as two collections and a shared member loses something |
| A persisted `overdue`, or any date-derived status | Something must act on it mechanically, which first means a reader cannot compute it |
| A `done` state on a work object | Something must distinguish "no items left" from "the work is over" that the object's own state cannot say |
| Work sidecars for backlog items | Unresolved staging needs dependencies or blockers — and then they belong as fields of the backlog model, since no authority boundary separates them |
| Owners, estimates, deadlines or labels on work | Work has to answer a question a bounded handoff cannot, at which point it has stopped being execution memory |
| An archive for pruned work items | A pruned item has to be recoverable without git |
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

# engr protocol v0

Normative. Where this document and the implementation disagree, this document is
wrong and should be fixed, or the implementation is a bug — say which.

## The one rule

**Every Section says which authority admitted its current semantics.** Human
admission goes `prepare` → the human reads the change → `confirm` with the exact
phrase. Agent admission is direct only when every applicable project Rule was
reviewed against the exact mutation and the review passed. A semantic Agent
mutation with no applicable usable Object Rule is refused; title creation and
rename are the only exception because a title is navigation metadata, not
authority.

The workspace also holds **backlog**, which is explicitly outside the record and
carries no such guarantee. The scope of the rule is what makes both possible: a
place to put unresolved work, without the record having to mean less.

## Model

An **object** is an aggregate. It holds **sections**, each carrying text.

```text
object
├── id                        uuidv7
├── title
├── type                      design | decision | risk        omitted when untyped
├── state                     one field, valid for the type
├── rev                       increments on every admitted action
├── next_section_id           monotonic, never reset
├── sections[]                omitted when empty
    ├── id                    integer, never reused, never renumbered
    ├── admitted              { by: human | agent, at: RFC3339 }
    ├── header                short navigation label            omitted when absent
    ├── role                  decision | risk | supersession | acceptance_criterion
    ├── text                  always the current wording
    ├── content[]             bounded literal excerpts, ordered  omitted when empty
    ├── based_on              { commit }                         omitted when absent
    ├── refs[]                { target, fields[], commit, digest }
    ├── relations[]           { type, target }
    └── digest                integrity seal over every other Section member
└── digest                    aggregate integrity seal over the Object
```

A section's `text` is always its current wording, because wording only changes
through an admitted action. Readers never have to ask which of two fields is
authoritative — there is only one.

The Object and Section representation is exact, and **absence is an omission**.
An optional value that is absent, an empty array and an empty object are all
left out; none of them is written as `null`, and no current Object repeats a
workspace format or version. One meaning has one persisted shape. The two
declared exceptions are an Event's `data`, which is always present and may be
`{}`, and a Collection's `members`, which is always present and may be `[]`.

A persisted digest scalar is `1:<64 lowercase sha-256 hex>`. The version prefix
names the contract the value was computed under, so a proof stays checkable for
its own lifetime rather than only until the calculation next changes. A
self-stored digest is integrity, not a signature: it says the bytes are the ones
that were sealed, and nothing about who sealed them.

A JSON resource is persisted as the **RFC 8785 (JCS) bytes** of its
schema-canonical value, with its sets already in canonical order. That is
enforced on the read path and not only in the writer: a resource arrives through
a git merge, a hand edit, a copy or another implementation as readily as through
a supported write, and a writer that emits one representation beside a reader
that accepts many is not one representation. The same comparison settles
duplicate member names without a second rule — a repeated key collapses during
parsing, so the value no longer re-serializes to the bytes that had two.

Predecessor generations are read under their own contract, which did not require
this. Bringing them forward is what migration is for.

A persisted resource is the bytes git tracks **at that path**, so no component
of the way to one — `.engr` itself included — may be a link. A link breaks the
correspondence in a way no digest can see: git records the link, which is its
target's name, while the tool reads and writes the target's contents, so the
history a reviewer reads is not the state the tool is using and the record can
sit outside the repository entirely. Reads and writes of the resource tree MUST
refuse it rather than follow it. How somebody arrived at the workspace is a
different question and is not restricted — a repository reached through a link
is ordinary. The staging entry a publication writes through is part of that
path, not a private detail: its name is derived from the resource's, so it is as
predictable as the resource, and a link planted there is followed by an ordinary
create and then renamed into the canonical path.

**Every probe of that tree has three answers, not two.** "Is it there" asked as
a boolean collapses a wrong-shaped or unreachable entry into absence, and absence
is the answer that lets work continue: a resource directory that is a regular
file becomes a domain with nothing in it, a redirected resource becomes a
resource that was never written, and a sidecar nobody can establish becomes a
subject safe to delete. Only established absence — the entry is genuinely not
there — may be read as absence; a wrong shape, a link on the way, and any other
failure to establish it MUST fail closed.

There is no public writer for a persisted resource, and that is a contract. A
raw serializer, or an Object save that validates shape, is not an admission
boundary: a self-consistent, correctly resealed Object says nothing about whether
any Event, Human Gate or Rule Review produced it, and holding the writer lock
closes a race rather than that question. Tightening only the Object save leaves
the same bypass one layer down, so a conforming implementation exposes durable
writes only through domain APIs that own their own authority contract.

### Sections are current authority; events are durable history

`.engr/eventstore/objects/<id>.jsonl` is append-only admitted history and audit
evidence. Every Object has a complete stream beginning at revision 1; it is
never purged, but it is not current-state authority or a second source of
current truth. **Sections remain authoritative for current wording**,
and git additionally preserves committed projections for look-back and tamper
evidence. Admitted Event history MAY nevertheless be replayed for verification
or for reconstruction under the explicit repair contract.

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

Ten. Human use is gated; the authority matrix below defines which Agent-reviewed
uses may be admitted directly.

Each has two names, and they are two statements. The **command** is what a
person is asked to assent to and is what a Challenge subject carries; the
**Event type** is what enters durable history.

| Command | Event type | Data | Effect |
| --- | --- | --- | --- |
| `create` | `object.created.v1` | `title` | Creates the object with the admitted title |
| `rename` | `object.renamed.v1` | `title` | Replaces the title |
| `section.create` | `section.created.v1` | `value` | Appends a section, id from the counter |
| `section.update` | `section.updated.v1` | `section`, `value` | Replaces that section's value; id unchanged |
| `section.merge` | `section.merged.v1` | `merge`, `value` | Destination keeps its id and takes the admitted wording; sources are consumed |
| `section.delete` | `section.deleted.v1` | `section` | Removes the section |
| `change_state` | `object.state_changed.v1` | `state` | Moves the object's state within its type's lifecycle |
| `classify` | `object.classified.v1` | `type?`, `state` | Sets both, explicitly |
| `supersede` | `object.superseded.v1` | `value` | Appends the reason, records the replacement, `state` → `superseded` |
| `repair` | `object.repaired.v1` | — | Restores the projection admitted history derives; changes no semantics |

A Section-valued action carries a `value`: the **complete resulting Section
semantic state**, `admitted` included, so replay never has to infer a Section's
admission provenance from the Event's own metadata. `create` and `rename` carry
a title and nothing else — there is no member a hidden basis or reference could
arrive in. `change_state`, `classify`, `section.delete` and `repair` carry no
wording at all.

The two halves of `admitted` are settled at different moments, and they have to
be. `by` is frozen with the rest of the question: which door a value comes
through is part of what a person assents to, and a value that changed doors
between the question and the answer is not the value they read. `at` is **the
instant of admission**, stamped when the act is admitted rather than when it was
proposed — a pending question can sit for a long time, so writing the
preparation instant into the record would state an admission time that predates
the admission, which is a false statement in the one place the record exists to
be true.

For an ordinary admission the Section's `admitted.at` and the Event's
`metadata.admitted.at` are **one instant, read once**. Two clock reads
microseconds apart would have the record say a Section was admitted at a
different moment from the Event that admitted it, and that is a distinction the
record does not have and could not defend. Migration is the one place the two
legitimately differ, and it says so where it does it.

An implementation MUST therefore compare a durable record against the question it
answers on everything except that instant. Nothing is loosened by the exception:
`admitted.at` is not a fact anybody assents to, it is a fact the admission
creates.

Migration adds one more, `object.migrated.v1`, which no command can ask for; see
[The migration](#the-migration).

`repair` is the recovery half of the integrity contract, and it is the
one action that changes nothing. An Object whose stored projection fails
integrity refuses ordinary mutation, so that unrelated work cannot reseal an
out-of-band edit into valid authority; `repair` is how it comes back.

```text
the stored projection is damaged
  -> ordinary Human and Agent mutation rejected
  -> repair proposed through the Human Gate, never admitted by an Agent
  -> the restored state is exactly what admitted history derives
  -> only then are new seals written
```

**Damaged is two states, and `repair` MUST accept both.** A failed seal is one.
The other is a projection that seals perfectly and is not what admitted history
produced — the resealed out-of-band edit below — and it is the state every trust
surface reports and directs a reader here to fix. Eligibility on integrity alone
answers that state with "there is nothing to repair", which leaves the one fault
a reader can see with no supported way to undo it, and hand-editing the file is
what put the record there. Unreplayable history stays a refusal: repair restores
what history derives, so where history derives nothing there is nothing to
restore *from*.

Integrity alone does not close that door, and a conforming implementation MUST
close it. Seals are recomputed from the bytes on disk, so an out-of-band edit
that is *also* resealed verifies perfectly — by hand, by another
implementation, by a script that meant well. The predecessor of an ordinary
admission MUST therefore be both intact and **history-consistent**: the value
its own admitted history produces, up to its own revision. Otherwise an
unrelated legitimate mutation takes the edited projection as its predecessor,
appends normally, and saves a projection the complete EventStore never
produced — and one Event later the unauthorized wording reads as ordinary
admitted authority.

An Event tail that is durable but not yet projected is not divergence; it is the
recovery buffer working as intended, and reconciliation applies it. Divergence
is the other direction: a projection asserting something no admitted Event ever
said. Verification MUST report it, and `repair` — never an ordinary mutation —
is the one path that reconstructs a divergent projection.

The two can hold at once, and that case is the one that matters most:
**reconciliation MUST establish history-consistency before it applies or saves a
recoverable tail.** Reconciliation starts from the stored projection, so
applying a durable Event on top of a divergent one builds an admitted revision
over wording nobody admitted — and then persists it, resealing the unauthorized
semantics into a newer revision and destroying the exact bytes `repair` would
have restored from. The prefix rule is what keeps that from misreading an
ordinary crash tail: only Events up to the projection's own revision are
compared, which is precisely the predecessor the tail would be applied to.

Replaying it is a no-op, and that is what makes it safe to record: history
already holds the projection being restored, so a repair states a fact about the
stored bytes rather than about the record. It therefore cannot carry a change.
Anything worth keeping from the invalid material is admitted afterwards through
the ordinary path, leaving the repair and then the real change in the log
rather than one event that quietly did both. Where history cannot rebuild
the projection, repair refuses — that is a different damage class and is not
guessed at here.

An object that **does not need attention refuses every section action, and a
rename**. Move it back into the attention set first. The friction is deliberate:
if admitted knowledge could still change while nobody was looking at it,
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
  "type": "section.updated.v1",
  "data": {
    "section": 3,
    "value": { "admitted": {}, "text": "..." },
    "becomes": { "type": "design", "state": "proposed" }
  }
}
```

`becomes` is admissible under **all three** of these conditions, and each is
enforced separately so that a refusal says which one failed:

1. The action is one the attention guard would otherwise refuse — `rename`,
   `section.create`, `section.update`, `section.merge`, `section.delete`. An
   action that sets the object's own state (`create`, `change_state`,
   `classify`, `supersede`) never carries one.
2. The object **does not currently need attention**. A destination is admissible
   *because* it is what makes the action legal; on an object already in the
   listing there is nothing to unblock, and a destination there would be a
   second, unrelated change hidden inside someone else's confirmation. Use
   `classify` for that, on its own, where a reader can see it.
3. The destination `(type, state)` is valid for the type and **needs
   attention**. That is the "only if" half of the rule.

The primary action and `becomes` are **one** confirmed operation: one Challenge,
one confirmation, one event, one `rev` increment, and no intermediate
authoritative state. This exists so that no state an object was never really in
has to be confirmed on the way. The two-confirmation `reclassify, then revise`
path remains available and means something different: two authoritative
statements, because there were two. A payload with no destination serializes and
hashes exactly as it did before the field existed.

The rule is about **renewed engineering work** — wording admitted once being
changed again out of sight of everyone reading the default listing. `supersede`
is therefore exempt, and the exemption is the rule rather
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

A merge names one existing destination and at least one distinct source. The
destination is not repeated in `sources[]`; every source is unique.

## Type and state

`type` is optional, and an untyped object is a **first-class long-term form**,
not a waiting room for classification. Most admitted knowledge is just
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

`classify` MUST carry both halves: the destination type, or its
explicit absence, and a state valid for that destination. There is **no
automatic mapping** between vocabularies — `design.accepted` becoming
`decision.accepted` would be engr making a judgement nobody asked it to make.

v0 defines **no transition graph**. Any destination is reachable when it is valid
for the destination type and the invariants below still hold. An invented
lifecycle sequence would be a process the protocol has no authority to impose.

Closing and reopening are `change_state` like any other move, and remain narrow:
`open` and `closed` are the untyped vocabulary, so on a typed object they have no
meaning and MUST be refused.

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
any admission, and putting it here would make the record assert something no
admission path reviewed.

Because a role changes the machine-readable meaning of admitted wording, it is
authoritative content: it passes the applicable admission path and is inside the
Section seal.

## Relations

`relations[]` is section-owned. The vocabulary is exactly:

```text
superseded_by
implemented_by
```

A relation is a typed semantic edge, and each type defines its own legal targets.
This is not an arbitrary-string knowledge graph.

`relations[]` is not `refs[]`. A Ref is a **selective semantic dependency** and
drifts when one of its selected target values moves; a relation says what this
assertion relates to, and inherits none of that drift behaviour. Nor is it
backlog `subjects[]`, which is weak, unconfirmed navigation.

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

Supersession is therefore **one atomic semantic operation**, `supersede`,
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

It is implementation-artifact provenance, not a selective semantic dependency, so a Section
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
reference. Duplicate types are allowed. An empty `content[]` is **omitted**,
like every other empty sequence in an Object or Section: one meaning has one
persisted shape, and a member written `[]` beside one left out would be two.
The declared exceptions to omission are an Event's `data` and a Collection's
`members`; this is not one of them.

`text` is required and MAY be empty only where a non-empty `content[]` carries
the meaning instead. A section with neither asserts nothing, and that is refused
wherever a persisted Section is read, not only where one is written.

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
Challenge MUST show the exception where the human reads it.

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

The exception is **admission-time only**. It is decided at prepare and is never
persisted: no `oversize` field on a
section, no lasting exemption. Every revision is measured again against its own
proposed value.

## Sets and order

```text
refs[]                  set
relations[]             set
section.merged.v1 sources[] set
backlog subjects[]      set
backlog produced[]      set
collection members[]    set
work dependencies[]     set
work blockers[]         ordered
work items[]            ordered by id
work items[].commits[]  set
content[]               ordered
```

Exact duplicate members of a set are invalid, on the way in and wherever a
stored value is read. Two refs to the same section pinned at different wording
are different statements and remain valid.

Reordering a set alone MUST NOT count as a semantic change. engr canonicalizes
sets at the gate, before the payload is fingerprinted and before a human is
shown it — so the same members written another way round produce the same
section hash, and a revision that changes nothing else is refused as having
nothing to confirm.

Every set uses **one** algorithm: serialize each element as JCS, then order the
elements by those bytes. Not a field-local rule — a second canonicalization is a
second place for two implementations to disagree, and the two answers diverge as
soon as the elements differ in length. `[2, 10]` is ascending; canonically it is
`[10, 2]`, because `"10"` sorts before `"2"`. A human-facing rendering MAY sort
however reads best; nothing persisted or hashed follows it — a merge
renders its consumed sections numerically and persists them canonically.

There is no field-local exception. A merge's `sources[]` was once specified
as numeric-ascending; that wording is superseded, and the shared algorithm
applies to it like every other set.

**Current-generation resources are persisted in that order.** A generation-1
resource has one persisted representation, so a stored set written another way
round is not valid current data however sound its seals are, and MUST be refused
on the read path rather than normalized. This is the same rule that makes the
bytes themselves JCS: two encodings of one value are two things a reader can
disagree about.

Historical and predecessor-generation material is read under its own contract.
Before this rule, no canonical persisted order was required and none was
enforced on the read path, which is why every hash written then stays valid —
and why adopting the rule is a **migration**, not merely a new check: a
predecessor resource is rewritten into the current representation as part of
advancing the workspace generation, never reinterpreted in place.

That predecessor case has a consequence the "nothing to confirm" check MUST
honour wherever it compares against material from an older generation. Such a
value holds whatever order its gate wrote, so the comparison MUST canonicalize
the **stored** value too, and only for the comparison. Otherwise re-proposing
the same members finds a difference that the model says is not one, and spends a
confirmation and a revision on sorting an array.

`content[]` is ordered because its entries are excerpts a reader goes through in
sequence. Moving one is a change to the assertion.

## The gate

`prepare` validates a proposed action against the current object, mints a
six-character challenge from `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` — no `0`/`O` or
`1`/`I` — and stores it as a Challenge under `.engr/local/challenges/`.
`prepare --agent` instead validates and admits an Agent mutation under the same
writer lock; it never mints a Human challenge.

A Challenge is one shape for every family that needs a human answer:

```text
challenge
├── id                        the six-character code; the filename stem
├── generator
│   ├── version               human-readable diagnostics
│   └── fingerprint           the exact generator compatibility identity
├── created_at
├── subject
│   ├── type                  object | migration
│   └── data                  owned by that family
└── digest                    over id + generator + created_at + subject
```

The common layer does not understand any family's operation semantics.
`subject.type` selects the family and `subject.data` is the family's own frozen
value. For the Object family that value is the act, the Object, the revision it
is bound to, and the exact payload:

```json
{
  "action": "section.update",
  "object": "<uuidv7>",
  "expected_rev": 7,
  "value": {}
}
```

`action` is the command vocabulary — `create`, `rename`, `classify`,
`change_state`, `supersede`, `repair`, `section.create`, `section.update`,
`section.delete`, `section.merge` — and not the Event type. What is being asked
for and what enters history are two statements, and the Challenge makes the
first. `value` is the same payload shape the resulting Event carries, so there
is one schema rather than two that have to agree — with one member settled
later. `admitted.by` is frozen with the question; `admitted.at` does not yet
exist as provenance and MUST NOT be rendered as an admission time. Confirmation
stamps the actual instant into both the ordinary Section value and Event before
either enters the record. `created_at` says only when the question was prepared.

`generator.fingerprint` is the opaque identity of the generator that minted the
Challenge. A pending Challenge is not migrated across incompatible generators:
one this build cannot interpret is refused and prepared again, never read under
rules nobody agreed on. Challenges are local, uncommitted and short-lived, so
that costs a moment.

`prepare` **refuses up front**, so nothing that cannot apply ever reaches a
human: the reducer is preflighted, `based_on` must name a real commit, and every
reference must resolve to an existing section whose selected current semantics
match its historical snapshot. Agent admission applies the same preflight before
writing. Deferring reference checks to `verify` is what let one mistyped id in
the previous design poison a global health check permanently, with no way back.

#### A reference pins selected semantics, not an integrity seal

`section.digest` and `refs[].digest` answer different questions:

| | |
| --- | --- |
| `section.digest` | whether the target Section still matches its admitted persisted state |
| `refs[].digest` | which selected semantic values the source was written against |

`fields[]` is a non-empty canonical set drawn from `admission`, `based_on`,
`content`, `header`, `refs`, `relations`, `role` and `text`. `admission` selects
the door a Section came through and not when it was admitted; identity,
timestamps and integrity seals are not selectable semantics. The digest is
versioned and computed from the target plus those fields' effective values; it
is never copied from a stored seal.

Admission is ordered, and the order is load-bearing:

1. Validate the current target Object aggregate and every nested Section seal.
2. Validate and canonicalize `fields[]`.
3. Resolve the exact commit and load the historical target under that
   snapshot's own workspace generation, validating historical integrity where
   that generation defines it.
4. Project the selected effective values on both sides and refuse unless they
   are equal. An uncommitted selected value cannot be referenced.
5. Compute and persist `{target, fields, commit, digest}`.

So at the moment a reference is admitted:

```text
selected(current target semantics)
  == selected(target semantics at refs[].commit)
  -> refs[].digest
```

Selected semantic identity decides drift; git history explains it.
`refs[].commit` remains provenance and recovery, not identity. A migrated
predecessor Ref selects exactly `based_on`, `refs` and `text` — the three fields
the released predecessor's whole-content seal covered, which is the whole of
what a predecessor Section was. It gains none of `admission`, `header`, `role`,
`content` or `relations`: those did not exist in that contract, so the original
Ref cannot have asserted them, and a migrated Ref that selected them would
report drift the first time somebody legitimately gave the target one.

There is **one live Challenge per object**. Preparing again supersedes the
previous one, so a human never holds two codes for the same thing.

### The title

An object's title is a label, not a body. Every action that sets a title —
`create` and `rename` — MUST refuse one that spans lines or
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
with the Challenge and MUST NOT be refused — two objects may legitimately share
a title, but they cannot be told apart in a listing, and the moment to
reconsider is while the human is still holding the code. The object being
written MUST be excluded from that check, so a rename that changes only casing
or spacing does not report a clash with itself.

The subject records `expected_rev`. A Challenge prepared against an older state
cannot be confirmed.

### Challenge integrity

`Challenge.digest` covers the complete Challenge except itself: `id`,
`generator`, `created_at` and `subject`. One digest, because a file that could
be rewritten in any of those would present one change, admit another, and still
pass every check of its own. Every load MUST check it, not only confirmation:
re-rendering a Challenge hours later is as much a use of its frozen question as
answering it is.

This is **not** a boundary against someone who controls the machine — the file
is on that machine and so is the binary. It is the narrower guarantee that a
Challenge rewritten on disk cannot present or bind a different act and still
pass.

The `id` is covered because it is the link between the two halves of the gate:
what a human is shown, and what their answer admits. A Challenge MUST
additionally be refused unless the `id` it stores is the one it was looked up
by. Otherwise, with two live, rewriting one file's `id` to the other's code
makes it render its own change while naming the other's answer — and both files
remain internally consistent, so nothing else catches it. What enters the record
would then be a change nobody read.

The Challenge digest is **local only** and is never copied into the EventStore.
A Human Event records the spent challenge id and nothing else about the question
it answered; the digest's whole job is finished the moment the code is answered.

An envelope that cannot carry that guarantee MUST be refused and re-prepared,
never read as if absence meant protection.

Old pending Human-Gate state is not migrated. A question asked under a
predecessor's contract cannot be answered under this one, and the material it
was asked about has moved representation underneath it — so it is prepared
again rather than reinterpreted, and a backward-compatible decoder is not kept
for a withdrawn contract.

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
section a `section.update`, `section.delete` or `section.merge` Challenge
acts on, and MUST identify the Object by a name a reader would recognise as well
as by its id. Two sections may carry identical wording, and then a screen that
names neither renders two materially different mutations identically — while
section ids are never reused, so confirming the wrong one breaks every reference
pinning it with no way back. The payload has always carried the section inside
the confirmation hash, which is what stops `delete §3` becoming `delete §5`
after it was displayed; that guarantee is worth nothing if the display never
said which section it was.

The section selector comes from the frozen subject, which is what stops
`delete §3` becoming `delete §5` after it was displayed. A recognisable name
does not exist in the subject at all, and it is **derived from the record at
`expected_rev`** rather than copied into the Challenge: a second stored copy
could only ever disagree with the record it names, and there is no third place
for the two to be reconciled. What keeps the derived half honest is the Object's
own seal — a projection rewritten outside the gate no longer verifies, so a
screen drawn from it says the record is not what was admitted, and the code
admits nothing.

A Challenge minted by a generator this build cannot interpret MUST be refused
and prepared again, not reinterpreted.

Then the **complete semantic change**, not the whole section again. Revisions use a
unified line diff with limited unchanged context and separately show old/new
`based_on`, old/new `role`, added and removed refs and relations — including
their pinned hashes and commits — and supplementary content entry by entry,
against the entry that held the same position. An explicit absence of repository
basis is displayed as such. Omitted unchanged wording remains part of the
complete frozen subject the code answers for.

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

`classify` shows the destination type, the destination state, and what
that does to the object's place in the default listing. A state without its type
is a word that means different things on different objects, and the attention
consequence is the thing actually being decided.

An oversize exception is shown before the wording it applies to, so the human
knows engr already refused this once while there is still a decision to make.

### Repository basis

`based_on` names the committed repository context against which wording was
formed; it is not an exact semantic dependency (that is what `refs[]` records).
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
  assent. The Challenge is **discarded**. "Yes, but reword the second line" must
  not become a yes, and the agent must not be the one deciding whether it counted.
- Anything else, including whitespace and casing slips → rejected, and the
  Challenge survives. A typo is not a qualification.

On confirmation, engr appends a Human Event, projects it into the sections,
reseals the changed Sections and Object, and clears the Challenge. Agent
admission appends an Agent Event carrying its review provenance — `outcome`,
`result` and `attempts`, and no digest; see [Project rules](#project-rules) —
and projects it in the same locked operation. Projection is immediate: the
sections are the authority, so they may not lag the log.

Each persisted Event record is schema-exact **and is the RFC 8785 (JCS) bytes
of the value it carries**, one record per line. That is checked on the read path
against the raw record text, not against the value it parses to: parsing has
already erased member order, insignificant whitespace, and any duplicate member
name it collapsed — and a duplicate is exactly where two conforming JSON stacks
are permitted to disagree about what a file says. An EventStore arrives through
a git merge, a hand edit or a copy as readily as through a supported append.

The framing is exact in both directions. **Every line is a record and every
record is terminated**: a blank or whitespace-only line is refused rather than
skipped, and a non-empty stream ends with the delimiter after its last record.
Skipping blanks would give a current stream a second spelling the writer never
emits, and it reads past the first sign of framing damage — a truncated write, a
partial copy, a bad merge.

Append-only is the semantics, not the write. A real appending write has a third
state, and it is durable damage: an unlocked reader can observe the file
mid-write, and a crash between a record's bytes and its delimiter leaves a
complete JSON object with nothing after it, so the *next* append concatenates
onto that line and two records become one forever — in a file that is never
rewritten. A conforming implementation therefore **publishes an Event stream the
way it publishes every other resource**: staged beside the file and renamed over
it, so every reader sees the complete old stream or the complete new one, never
a prefix of either. This is why the delimiter requirement above is a read-path
rule and not a courtesy.

**And the name must be as durable as the bytes.** One admission publishes two
resources in two directories — the Event stream, then the Object — and the whole
recovery model rests on their order: history ahead of the projection is the
crash this design expects and reconciles, while the projection ahead of history
is the direction nothing can recover. Flushing a file says nothing about the
durability of the directory entry that reaches it, so without flushing the
containing directory after each rename those two publications have no
established order across a power failure at all, and the caller has already been
told the admission succeeded. Every phase boundary in a workspace is a name — a
published resource, a staged migration, the generation marker — so a
platform that can make a name durable MUST do so before reporting success, and
one that cannot MUST say so rather than imply a guarantee it does not keep.

`rev` starts at 1. Revision zero is the Object before any Event; the first
admitted Event advances it to 1, and no writer emits zero. Adjacency alone
cannot refuse it, because a `0, 1, 2 …` log is perfectly contiguous, so the
lower bound belongs to the record contract itself.

**A well-formed Event is not an admitted one.** Schema-exact, contiguous,
replayable, with provenance scalars spelled perfectly — none of that says a
person was shown anything or that any Rule was read, and a durable append that
assumes it lets a caller write authority the gate never granted. The durable
boundary MUST therefore prove admission, under the same lock the write lands in:

- a Human Event only against the **prepared Challenge it names**. Confirmation
  appends before it discards, so the file is still there, and the code is the one
  value a caller cannot invent. The named Challenge's frozen subject, the applied
  revision and the exact payload must all correspond.
- an Agent Event by **rebinding** the live applicable Rule set for exactly this
  mutation and requiring the record's claim about review to be one that could
  have happened: a record naming a passing review where no Rule governs the
  mutation describes a review nothing could have run. A semantic Agent mutation
  with no applicable usable Object Rule is refused; a title action is the sole
  non-authoritative exception.

  It is a **narrowing** from recomputing a stored ReviewDigest, and a deliberate
  one. History keeps `{outcome, result, attempts}` and not the digest, because
  the digest binds Rule artifacts that will have moved and is checkable only
  while the Challenge and those exact artifacts still stand — see
  [Project rules](#project-rules). What it cost is catching a record that named
  a review of some *other* mutation; what stands in the way of that is that
  there is no public append at all, and that Agent admission verifies the
  attestation against the live binding before it builds anything.

The proof runs after the shape checks, so a malformed record is still refused for
being malformed rather than for failing a proof about a shape nothing could admit
anyway.

Recomputation is necessary and it is not sufficient, which is why there is no
public raw append. Event provenance is deliberately minimal: it carries how the
mutation got in, what the review concluded and which attempt it was — `outcome`,
`result` and `attempts` — and none of the material that decision was made
against. The exact Rule artifacts, the digest that bound them and the agent's
explanation stay out of history, because they are decision-time material. So
every mutation carries one Agent-attested attempt and each applicable Rule
judges it against its own ceiling, and past any ceiling autonomous Object
admission stops — but what survives is the number, not the judgement it was put
through. A boundary that can only re-derive what the record carries therefore
cannot ask the question at all, so a public raw append would be a second Agent
admission API holding strictly less state than the gate.
Asking whether a record *would* be accepted is a separate, read-only question
and may be public.

The title exemption is likewise narrow in both directions. It says there is no
applicable Rule to review against, not that titles are exempt from Rules: where
a workspace governs the Object domain, a title mutation reviews against it like
any other, so the absence MUST be established rather than inferred from the
shape of the action.

The append boundary MUST also refuse a record whose **replay** would leave the
workspace outside the current schema. Some current-state integers are allocated
by the reducer and appear nowhere in the record — a `section.created.v1` carries no
Section id and no counter — so a walk over the record's own numbers passes while
the projection it produces is one canonical sealing would refuse. The log is
append-only, so such a record is durable history its own recovery path can never
materialize.

Re-confirming a code whose event is already applied is **idempotent** — it
reports what happened rather than applying it twice. That closes the crash window
between saving the projection and clearing the Challenge. Recognising it requires
the **exact** correspondence above, the answered code included: two Challenges
can freeze the same mutation and are still two questions, so a restored copy of
an older one MUST NOT be reported as the newer one's retry. It is stale, and
answering it admits nothing.

### Projection is deterministic

The reducer takes an object and an event and nothing else. **No clocks, no git,
no language model, no interpretation of prose.** Everything it needs, including
the admitting path, was frozen into the Event before projection. Structure that
was not recorded does not exist.

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

Every current read surface MUST validate the Object aggregate and recompute each
Section seal before rendering it, and MUST say so where the reader is already
looking. A reading path that prints unverified wording under an `ok` is worse
than one that prints nothing: it asserts authority that the stored integrity no
longer supports. Integrity failure is diagnosed without silently resealing it.

A Section MUST also be marked when a target it references fails current or
historical integrity. Comparing a Ref digest alone cannot see a hand edit that
left the stored seals untouched, so integrity is established before semantic
drift. The diagnostic SHOULD identify which side failed even though both map to
the single `TargetIntegrityFailure` dependency state.

| | |
| --- | --- |
| current or historical target integrity fails | stored authority cannot be trusted — **integrity failure** |
| integrity holds, selected current values differ from the historical selection | the dependency moved — **drift** |
| integrity holds and selected values agree | unchanged |

Only the directly referenced section is checked; the target's own read covers
what *it* stands on.

An authoritative **relation** is verified too, and separately from `refs[]`.
`superseded_by` names an existing different Object, and v1 has no Object delete,
so a target that cannot be established means the invariant already failed. The
source Object's own seals cannot see it — nothing about A changes when B
disappears — and `refs[]` does not include the relation, so without this a reader
following the chain to find current knowledge arrives nowhere while verification
reports a clean record. A missing, unreadable or integrity-invalid replacement
MUST be reported as a verification failure, and MUST NOT be presented as
ordinary Ref drift: drift asks a person whether this wording still holds, while
this says the forward link out of the Object is broken.

The same reading applies where a new supersession is authorized. Traversing the
replacement graph MUST fail closed on a target that genuinely cannot be
established, not treat it as the end of a branch — an unwalked branch can hide
the cycle the traversal exists to find, and a graph that cannot be established is
not a graph that is clear.

Corruption outranks staleness. A Section whose persisted state does not match its
seal is not a Section that drifted, and its drift assessment would describe
something no admission path accepted.

## Staleness

Two signals, both computed at read time, both needing nobody to be reading.

| Signal | Computed from |
| --- | --- |
| The basis moved | `based_on` versus HEAD: commits ahead, files changed |
| A dependency changed | selected current semantic values versus the historical Ref digest |

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

`verify` recomputes each Section seal and Object aggregate from what is stored,
then checks the current and historical sides of every dependency, and last asks
the question no seal can answer: whether the projection is the value its own
admitted history produced.

A seal catches a section edited without recomputing the hash. It **cannot**
catch an edit that recomputes the hash too — that is what admitted history is
for, and the two faults it exposes are different and MUST be reported as
different things:

```text
tampered      the bytes do not match their own seal
divergent     they match it, and no admitted Event produced them  -> repair
unreplayable  admitted history cannot be replayed at all          -> the EventStore is damaged
```

`repair` restores a divergent projection, because history holds what it should
say. It is not the answer to unreplayable history, where there is nothing to
restore *from*, and a surface that reported the second as the first would send a
reader to a path that refuses.

`show` reports all three for the one Object it is about, and **fails** when it
reports any of them: a screen that says an Object is not what its history
produced and then exits 0 tells a script the opposite of what it told the
reader. `ls` surveys and keeps exiting 0 — it would have to replay every
Object's history to answer, and it already sends a reader to `verify`.

Committed git history provides an additional tamper anchor, which is why
`verify` also reports an uncommitted object.

Do not read `verify` as proof that the recorded admitting actor really performed
the path. It proves internal consistency. Human admission in particular is a
convention enforced by the agent's instructions, not an identity mechanism —
see below.

## Backlog

Where unresolved engineering work waits. It is **not a weaker record** — it is
outside the record entirely.

| | Record | Backlog |
| --- | --- | --- |
| Admission | Human confirmation or passing Agent Rule Review | none; agents edit it directly |
| Authority | current admitted wording | none |
| History | append-only events, and git | git |
| Integrity | Section and Object seals, tamper alarms | schema validation only |

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
for the same reason no resource carries them, since `.engr/VERSION` is the sole
generation authority.

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
    └── produced[]?     admitted outcomes so far
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

Because that removal takes the item with it, it is the one Backlog mutation the
Work domain constrains: an item that still owns a work sidecar MUST NOT be
removed, and the consume that would remove it MUST be refused until the sidecar
is discarded explicitly. Every other consume is unaffected, and so is a merge —
see "A sidecar may not outlive its subject".

Section ids are monotonic and never reused, for the reason the record's are —
`max(existing) + 1` would hand back the id of a consumed Section and silently
repoint every subject aimed at it.

### Merging says two points were one

A merge names one **destination** and one **source**, and is one mutation:

```text
destination survives, keeping its Section id, with the merged wording
the source is removed
```

Exactly one source. Taking several is not an ergonomic spelling of repeating
the operation: one review and one atomic apply would consume several unresolved
identities at once, against a different predecessor and a different reviewed
subject than the one this contract froze. Two points to fold in is two merges,
and each consumption is then its own judgement against its own predecessor.

The destination identity survives. A merge MUST NOT allocate a replacement
Section identity, and MUST NOT reuse a source id later. Minting a fresh id to
hold the merged wording is the tempting shape and the wrong one: everything
already aimed at the destination — subjects, `produced[]` entries elsewhere, a
human's own note — would be left naming a Section that stopped existing at the
moment somebody observed the two were the same point.

`produced[]` MUST be carried by **set union**:

```text
destination.produced = union(destination.produced, source.produced)
```

Merging says these were one unresolved point, not that the outcomes never
happened. Dropping a source's outcomes would lose the one thing that stops a
later session re-solving work an earlier one already got admitted.

The merged destination receives a refreshed `updated_at`: it now states
something it did not state before.

It is **atomic**. Consolidation is a single judgement, and half of it applied is
a state nobody decided on — a destination carrying merged wording while a source
it supposedly absorbed still sits there unresolved. An implementation MUST check
every participant before removing anything, so a merge naming a Section that is
not there changes nothing at all.

The precondition binds the parent topic, the complete destination and the
complete source. An unrelated sibling Section changing MUST NOT stale it; any
change to the topic, the destination or a source MUST.

A Section in the record never moves back into backlog. Its wording remains the
last-admitted wording until another admitted revision replaces it; the
doubt goes into backlog instead, and is later settled by a normal record action.

### subjects[]

*This unresolved point concerns these things.* Deliberately weaker than `refs[]`:
no dependency, no authority, no ordering, and no claim the target must change.

Weaker in **protocol coupling**, and only there. A subject is not a weaker fact
about the point than its wording is — what an unresolved point concerns is
central to reading it, and a surface that treats these as decoration is showing
less than the point says. What "weak" withholds is consequence: nothing here
constrains another domain, gates a mutation, or survives as provenance.

An `engr` subject may name an Object, an Object Section, another backlog item or
one of its Sections. **Backlog-to-backlog cycles are valid** — this is a
navigation relation, not a dependency graph. Authoritative `refs[]` MUST NOT
gain the ability to target backlog: an authoritative Section cannot stand on wording
nobody read. The asymmetry is the point.

A `file` or `symbol` subject pins a path and a full resolved commit. The path
MUST exist in that commit: a baseline that never held the file reconstructs
nothing. Symbol identity is a path and a human-readable name; no
language-specific resolution is attempted.

Where the observed target carried changes the pinned commit does not hold, the
subject is still written and MUST record `dirty: true`. Refusing it was the
earlier rule and the wrong trade: the agent genuinely read something, and losing
that context is worse than recording a baseline that is honestly labelled
inexact. The commit stays recoverable; the extra context may be gone.

The comparison is against **the commit being pinned**, not against the repository
head. Those coincide only when the pin is the head: an explicitly chosen older
revision differs from a perfectly clean worktree, and a working-tree cleanliness
check reports `dirty: false` there while the file is not what that commit
reconstructs — which is precisely the claim the field makes.

`dirty` is **target-local**. It says nothing about the repository as a whole and
nothing about `git worktree`. For a `symbol` it means the **containing file**
was modified — proving a diff intersects one symbol's own source range would
require language parsing and AST mapping, which this protocol MUST NOT require
for context metadata, so readers MUST NOT read it as a claim about the symbol.

`dirty` is **not part of subject identity**: the same target re-observed against
a modified worktree is the same subject, and MUST NOT read as fresh activity. It
is absent when clean, so a clean subject is byte-for-byte what it always was.

A subject that later stops resolving is a stale signpost, and MUST NOT make the
item unreadable. Backlog is staging, not a referential-integrity database.

`subjects[]` is semantically **unordered**, and exact duplicates are refused so
that "the same set" has one meaning. Duplicates are judged by the same identity
that decides equality, so a target that differs only in `dirty` is a duplicate:
`dirty` is not part of identity, and a comparison that included it would let one
target sit in a set twice while the rest of the model calls them one subject.

Semantically unordered does not mean unordered on disk. `subjects[]` is a set in
`## Sets and order`, so a current-generation item persists it in the shared
canonical order and a stored item in any other order is refused on the read path.
An earlier version of this section said no persisted sort order was required;
that wording is superseded, exactly as a merge's `sources[]`
numeric-ascending wording is.

### Mutation preconditions

A prepared Backlog mutation binds the predecessor it was written against, and is
applied only while that predecessor still holds. This is **unconditional**: it
is not a consequence of a Rule governing the domain. A Rule decides whether
there is a review to anchor; it never decided whether somebody else's write can
land between a caller's reading and their writing, and conflating the two turns
stale-write protection off in exactly the workspaces nobody has thought about
concurrency in. Creation is the only exception, for a reason of its own below.

```text
read the exact predecessor
-> prepare and review the exact mutation
-> apply only while the predecessor still matches
-> otherwise stale: read again and re-prepare
```

What each mutation binds is exactly what it rests on:

| mutation | binds |
| --- | --- |
| create an item | nothing — see below |
| change the topic | the **complete** parent item |
| add a Section | the parent topic, and the id the add will **receive** |
| change or consume a Section | that **whole** Section, and the parent topic |
| merge Sections | the parent topic, and the **whole** destination and its one source |

A predecessor that still holds is not the same as a predecessor for *this*
mutation, so an implementation MUST also check that the bound predecessor is the
one the mutation rests on: the same item, and the same Sections. Otherwise a
caller holding a genuinely valid predecessor for one item can apply it to
another, or bind §1 and consume §5, and the guarantee fails exactly where it
looks satisfied. Which is the wrong answer to report first, so the mismatch MUST
be distinguishable from staleness — "you prepared against something else" is a
different problem from "what you prepared against moved".

An add binds the id it will **receive**, not merely that some id is absent.
Another writer can take that id and consume it: the id reads as absent again
while the allocation counter has advanced permanently, so an absence check
passes and the add lands on an identity nobody reviewed — under a number the
first allocation's subjects may already name.

Creating an item binds nothing, and that is settled rather than pending. **engr
mints the UUIDv7 while performing the create, and a caller MUST NOT supply or
choose it**, so there is no proposed id whose absence a creation could bind. A
creation MUST refuse a precondition rather than accept one it cannot honour:
whatever id a caller prepared against, the item created is a different one, and
checking the first would authorize the second.

The alternative was letting a caller propose the id so creation would have a
predecessor like every other mutation. It was declined because pre-authorizing a
UUID needs reservation or pending state, or a token proving the caller was
allowed that id — a whole lifecycle bolted on to protect an identity nobody else
can be racing for. Rule Review still governs the create intent; identity is
simply engr's to issue.

The scope is the decision. Binding less than the whole Section is what the
removed `canonical(text, subjects[])` fingerprint did, and it was blind to every
field outside its list — including fields added later, which nobody would think
to add to it. Binding more, such as the whole item for a single-Section change,
would stale a mutation because an unrelated sibling moved, and a staleness
signal that fires on unrelated work stops being read.

An implementation MAY compare a hash internally. There is **no** protocol-level
persisted Backlog fingerprint, and no field records one.

That allowance is what makes the model reachable from a command line, which it
otherwise would not be: an agent cannot hand back a complete Section as an
argument. So a read surface MAY print a short stand-in for each predecessor and
a mutation MAY take it back, as long as nothing persists it and the authority
remains the whole predecessor compared at apply time.

Carrying it is not optional decoration. Preparation happens before the mutation
runs, so a command that reads and writes under one lock still leaves the entire
interval between preparing and running unguarded — a concurrent edit in that
gap lands underneath a mutation nobody prepared against, and every check inside
the lock passes, because they compare against what the command itself read
rather than against what the agent read. **Every existing-state mutation MUST
carry the exact predecessor it was prepared against, whether or not a Rule
Review was required.** Rule presence determines whether review applies, not
whether the stale-write precondition applies.

That requirement belongs to the **mutation**, not to whichever interface reached
it. An implementation that enforces it only at its command line has enforced its
command line: any other caller reaches the same semantic mutation, and the check
has to sit where they meet.

Creating an item is the one exception, because engr issues the identity and
there is nothing for a predecessor to name. Requiring one would make creating an
unresolved point impossible in exactly the workspaces that have rules about
unresolved points, and an implementation MUST NOT resolve that by accepting a
predecessor it will not honour.

Staleness is its own outcome, not a failed mutation and not corrupt data: the
caller did nothing wrong and the world moved underneath it. The refusal MUST say
which part moved, so the retry is intelligent rather than reflexive.

### updated_at

When the unresolved point last saw activity: creation, text revision, subject
changes, a merge result, and recording or forgetting a produced outcome. A topic
rename MUST NOT refresh it — the topic is the context a point is read in, not
the point. Item-level activity is derived from the Sections rather than stored,
so the two cannot disagree.

Bookkeeping counts, and the earlier reading that it does not was withdrawn.
Learning what a point produced is meaningful to whoever picks it up next, so a
`produced[]` change is activity even though the wording did not move. What is
*not* activity is a write that changes nothing at all: rewriting a Section with
the wording it already had, or with the same `subjects[]` set in a different
order, MUST leave `updated_at` alone. Order is not content — `subjects[]` is a
set — and an idempotent write that manufactures activity puts an untouched point
at the top of the list somebody reads to find what was touched.

A change to persisted observation metadata is not that. `dirty` records how a
target looked when it was observed, and it is deliberately outside subject
*identity* — a file re-observed against a modified worktree is the same target.
But it is persisted staging state, so changing it changes what is stored, and a
mutation that changes stored state is activity and takes the ordinary review
bookkeeping with it. Identity equality decides which target is meant; it does not
decide whether anything was written.

The value is an RFC3339 timestamp, and it MUST be compared and rendered as an
**instant**, never as text. RFC3339 carries an offset, so
`2026-08-17T01:00:00+08:00` sorts after `2026-08-16T20:00:00Z` while being
three hours earlier, and shortening a value by cutting the string at its
fractional seconds and appending `Z` reports a different moment entirely. Read
surfaces may normalize the offset for display; they may not change the instant,
and the stored value keeps its own precision and offset.

It is operational metadata, not a concurrency token: whether a prepared mutation
is still applicable is decided by its precondition, never by this timestamp.

### produced[]

Authoritative knowledge already created or materially changed while working on
this point. Targets are authoritative Objects and Object Sections only; backlog,
collections, files and symbols are refused, because `produced[]` answers what
the *record* gained.

```text
produced.length > 0   DOES NOT MEAN   resolved
```

One unresolved point may produce several admitted outcomes across several
sessions and still have work left in it. They MUST NOT be forced into one batch
admission so the point can be consumed. An agent resuming work should read
the text, the subjects and the produced outcomes together before deciding what
is left — that is what stops it re-solving what an earlier session settled.

Object admission and this bookkeeping are **two independent operations**.
Admitting an Object appends nothing here and consumes nothing here; an agent
that worked from a point updates it afterwards, as an ordinary Backlog mutation.
engr never infers the link, because an inferred one would eventually consume a
point nobody meant to resolve.

A declared outcome asserts that authority exists, so appending one MUST refuse a
target that does not exist — and MUST refuse one whose persisted integrity no
longer holds. Existing and sound are different questions: a section edited
outside the gate loads perfectly and reads as authority, and an entry claiming
it would launder that edit into a record of what was produced.

Integrity is checked at **the granularity being claimed**. A Section-qualified
target is judged on that Section's seal. An Object-level target claims the
Object's own authority — creation, a type or state transition, supersession —
and nothing seals `title`, `type`, `state` or the revision, so every Section seal
passing does not establish it. That authority MUST be checked against the
durable history it was admitted through, and the **complete** rebuilt aggregate
MUST be compared rather than a chosen set of fields — a Section removed from the
projection outside the gate is never visited by a per-Section check, and gaps
below the id counter are what legitimate admitted deletion looks like. Where the
history cannot rebuild the Object, the claim MUST be refused rather than
accepted unchecked. **Existence and
integrity are checked when the claim is made and never again** — which is why
they MUST be checked where the claim is *written*, under the same lock. Validating first and appending afterwards leaves a gap an
Object mutation fits through, and the single check this relationship ever gets
would have been against something that no longer existed when it landed. This
holds in that direction only: `produced[]` is a record of what happened,
not a referential-integrity constraint, so it never constrains the Object
domain. A target may later be superseded, deleted, or absorbed by a merge while
an entry still names it; the entry becomes an unavailable historical pointer,
which is not corruption, and it MUST NOT be retargeted to a replacement —
rewriting it would rewrite what was actually produced.

### rule_review

Present on a Section only when the wording standing in it was admitted **without
a passing review** — the attempt had gone past a project rule's ceiling and
Backlog admitted it anyway. See "How many attempts, and what happens after" for
why this domain admits rather than refuses, and for the two numbers.

Absent is the ordinary case, and it means exactly one thing: this went in
normally, or no project rule governed it. It MUST NOT be written for a mutation
that changed nothing, since an idempotent write admitted nothing.

`attempts` MUST be greater than `limit`, and `limit` MUST be at least 1. A
stored value outside that is not a diagnostic but a claim about a review that
never happened, and it MUST be refused on read like any other impossible state.

A topic rename admits no Section wording, so there is nowhere for this marker to
go — and an exhausted rename MUST NOT simply proceed unmarked, which would make
it the one soft-admission nothing records. Marking every Section instead would
claim of each one something true of none. So an implementation MUST either
persist the exhaustion in item-level state or **refuse the exhausted rename**.
What an item-level marker looks like is not settled here, and an implementation
MUST NOT invent one; refusing is the conforming choice until it is.

It is an ordinary persisted Section field, so it participates in the mutation
precondition like any other — a mutation prepared against a Section that has
since been marked is stale.

### Read surfaces

Backlog lives under an explicit namespace and every surface it prints MUST state
that it is unconfirmed. Structured output carries that as a field, because it
travels furthest from any banner.

`ls`, `show` and `verify` are record surfaces and MUST NOT mix backlog wording
into their output. `verify` stays record-oriented: staging validity is not part
of what a record `PASS` claims.

## Work

**Execution memory an agent keeps for one object or one backlog item.** It
answers "where does this currently stand" and nothing else. Like backlog it is
agent-managed, git-tracked and admitted by nobody; unlike backlog it is not a
domain of its own but a **sidecar** hanging off its subject.

```text
subject = object | backlog item
```

A subject has at most one sidecar. The question work answers is the same whether
execution started from an unresolved backlog point or from durable knowledge
already in the record, so the sidecar follows the thing being worked on while
authority and identity stay with the subject. Nothing else about work varies
with the subject kind: one shape, one set of limits, one rule domain, one set of
permitted dependency targets.

**Subject**, not *owner*: the relationship is "the thing this execution is
about", and an owner is a person or a team. A domain word that reads as
responsibility in every other engineering tool is the wrong word for something
that carries no responsibility at all.

Backlog Section `subjects[]` shares the word deliberately, and that is not a
collision. Both answer *what the containing thing is about* — one shared
aboutness vocabulary — while each domain defines its own cardinality and its own
protocol consequences:

```text
work subject          exactly one object or backlog item
                      strong scoping/attachment
                      determines storage and lifetime

backlog subjects[]    zero or more substantively relevant things
                      central context and navigation
                      weak protocol relation only
                      no ownership, dependency, authority, provenance,
                      ordering or lifetime semantics
```

Neither MUST be renamed to make the difference in coupling visible in the name;
the difference is what the owning domain says about it, and the shared core is
real. "Weak" in the backlog sense describes the **absence of protocol coupling**,
never weak relevance to the unresolved point.

```text
work sidecar
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
one MUST NOT be two spellings of the same sidecar. More generally, a stored
sidecar MUST be held to exactly what the write path can produce — a reader that
accepts shapes the API refuses is a second, larger schema that only ever gets
discovered by something that came to depend on it. A fault in a stored file is a
**schema** fault, not a usage error: nobody currently running a command wrote it.

`dependencies[]` and each `items[].commits[]` are sets, in the shared JCS-element
order; their duplicate checks are part of that contract. `blockers[]` is ordered:
removing one names its position, and duplicate-looking conditions can still be
separate observations. `items[]` is ordered by monotonically allocated `id`, not
by the order an agent happened to describe steps. Those classifications apply on
both read and write; their order is representation, not execution priority.

`updated_at` MUST be a valid RFC3339 timestamp, and anything ordering sidecars
by it MUST compare **instants** rather than text — two valid values
written in different offsets do not sort correctly as strings. The stored
spelling is preserved for display.

Stored by subject kind, carrying no `format` or `version` of its own —
`.engr/VERSION` remains the single generation authority:

```text
.engr/work/objects/<object-id>.json
.engr/work/backlog/<backlog-id>.json
```

Most subjects have none, and absence means only that engr holds no operational
memory. The directory is the only statement of the subject kind and the filename
the only statement of the subject id: a stored sidecar MUST NOT carry a subject
member of its own, because a second spelling of the same fact is a second thing
that can be wrong. Both kinds of subject id are UUIDv7, so the path is what
distinguishes them, and anything enumerating work MUST cover both directories —
a check that reads one of them is a check with a hole in it.

There is **no `engr:work:` reference**. Work is not an addressable resource,
nothing points at it, and it has no identity beyond the subject it belongs to.
CLI surfaces name a subject by its canonical standalone reference,
`engr:obj:<id>` or `engr:backlog:<id>`; nothing gains a subject-kind flag or a
work-specific spelling.

### A sidecar may not outlive its subject

A work sidecar MUST correspond to an existing subject, and that MUST be held on
**read** as well as on write. A sidecar names its subject in its path, so a
copied file can name one that never existed; an implementation that only checked
when writing would then read, list and hand back operational memory for nothing.
An orphan sidecar is invalid work, not a row with a missing title.

Objects satisfy that for free, because no mutation removes one. A backlog item
is the case where it has to be enforced, since being removed is how it is
resolved. The rule is stated at the subject's lifetime rather than as a backlog
special case:

```text
a mutation that would remove a subject while a sidecar exists MUST be refused
mutations that preserve the subject are unaffected by work
```

For the current backlog model that is exactly one site:

```text
consume a Section that is not the last  -> allowed
merge Sections                          -> allowed
consume the last Section                -> refused while a sidecar exists
```

The refusal MUST name the explicit removal that clears it. An implementation
MUST NOT instead delete the sidecar as part of the subject's removal, and MUST
NOT leave the orphan. A cascade would discard a `paused` sidecar — a human's stop
signal — inside an operation about something else, which is the silent
disappearance the work domain reports rather than performs; an orphan would
leave the workspace holding memory for nothing. The presence check is on the
file, not on whether it loads, so corrupting a sidecar is not a way past the
invariant.

This constrains **removal only**. Work never decides whether an individual
unresolved point can be resolved, and completing or discarding a sidecar
resolves nothing and consumes nothing: the backlog lifecycle above remains the
whole of how a point leaves.

### It owns no authority

This is the whole of why it can live outside the gate.

```text
work never changes object semantic state
work is never promoted wholesale into the record
finishing every item settles nothing
```

An agent may complete every item it wrote and the object is exactly where it
was. If a result turns out to be stable engineering knowledge, it reaches the
record through the applicable Human or Agent admission path. Unresolved
reasoning belongs in backlog. `summary` is a checkpoint, not a decision record, a
design analysis, a session transcript, or a copy of git history.

`ls`, `show` and `verify` are record surfaces and MUST NOT mix work into their
output, for the same reason they exclude backlog. Every work surface MUST say
what it is showing, **including the structured one**: a machine-readable
non-authoritative discriminator is required there, because JSON is the surface
that travels furthest from any banner and `{"state": "active"}` on its own is
indistinguishable from a subject's own state.

Every work surface MUST also say which **kind** of subject it is about, and MUST
NOT leave that to the id. Both namespaces mint UUIDv7, so an id alone — in a
listing column or a structured field — does not say whether execution memory
belongs to durable knowledge or to an unresolved point, and those are different
claims about how settled the thing is.

Structured output names the subject as `subject`, in the shared embedded engr
target form:

```json
{
  "subject": { "kind": "engr", "ref": "backlog:<id>" }
}
```

`ref` is `obj:<id>` for an object-owned sidecar and `backlog:<id>` for a
backlog-owned one, and it MUST identify a whole current resource of one of those
two kinds — never a Section, a snapshot, a Collection, or another sidecar. This
is the same representation `dependencies[]` and `blockers[]` use in the same
document, which is the point: an implementation MUST NOT emit a work-specific
identity object beside it, because a second way to write a resource identity is
a second thing every consumer has to learn and a second thing that can disagree.

There is no top-level `object` member. It was the pre-#63 spelling, it could
only name one of the two subject kinds, and it is **withdrawn rather than
retained**: carrying it alongside `subject` would leave a field that is correct
for one kind of sidecar and absent or misleading for the other.

Text surfaces must say more than backlog's banner does, because the failure worth
preventing here is not a reader trusting unadmitted wording but a reader taking
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

The same rule covers deletion: an agent MUST NOT delete a paused work
sidecar without explicit human direction.

All of that is **normative on the agent**, not mechanical. engr cannot check it,
because it cannot tell an agent from a human — so it lives here and in the Skill,
exactly like the gate itself does. An implementation MUST NOT turn it into a
lifecycle rule by refusing the deletion: that would stop no agent willing to
clear `paused` first, it would make a human's own instruction impossible to carry
out directly, and it would invent a persisted transition whose only purpose is to
satisfy the refusal. What an implementation SHOULD do is make sure the signal
never disappears in silence — say, when a paused sidecar is deleted, that a
human's stop signal went with it.

Whether human direction should have a mechanical representation at all is an open
question; see [What v0 does not
solve](#work-has-no-mechanical-notion-of-human-direction).

A sidecar may otherwise be deleted freely once it no longer carries useful
handoff, and for a backlog subject that deletion is also what clears the way for
the item to be resolved. Deleting says only that no operational memory is being
kept; the subject is untouched. Completed items may likewise be pruned once they stop helping the
next agent. There is no archive: git holds what the sidecar used to say.

### Derived standing

```text
active,  no blockers   -> active
active,  blockers      -> blocked
paused                 -> paused
```

`blocked` is **derived and never stored**, for the same reason attention is: two
fields that can disagree eventually do. There is deliberately no `done` state on the
sidecar either, and that absence is load-bearing — a completed sidecar must never
become a second answer to "is this settled" competing with its subject's own state.

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
wording nobody admitted, work holds progress nobody admitted, and a collection
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
`.engr/VERSION` remains the single generation authority. The stored `id` MUST
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
a persisted order MUST be > 0, and MUST be unique within one collection
absent means unranked; 0 at the input boundary clears a rank and is omitted
a negative order is invalid, on the way in and on the way out
```

These are structural, and none makes a collection authoritative. A rank that two
members shared would be a sequence with a tie it cannot break; unranked members
may of course share their absence, and a partly ordered plan is the normal state
of a plan. Array position is **not** an ordering — a reader sorts by `order` and
leaves the rest explicitly unranked.

Unranked has exactly one spelling, which is why a stored `0` is refused rather
than accepted as a synonym for it, and a negative is refused because it is not a
rank at all: it would sort ahead of every real one and quietly make itself
first.

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

## What Human admission does not prove

`prepare` prints the challenge code where the agent can read it, and the agent
runs `confirm`. **Nothing stops an agent confirming its own proposal and thereby
claiming `admitted.by = human`.** Agent admission does not rely on that fiction:
it is explicitly tagged and carries the Rule Review that authorized it.

Treat `admitted.by`, `admitted.at` and matching seals as a record of the path
used, never as identity proof that a human was present. Making the Human
Gate a mechanism needs the challenge to travel where the agent cannot read it,
or `confirm` to run in a different process. That is not v0.

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

The way through is built from what already exists: `section.delete` through the
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
store, no Challenge and no confirmation for a rule file: git is their history.
Changing one changes what the *next* mutation must be reviewed against, and
nothing already admitted.

```text
.engr/rules/*.md    YAML front matter + a normative Markdown body
```

`id` is the stable identity and the filename is only a locator, so renaming a
file does not create a different rule. **Duplicate ids fail closed**: two files
claiming one identity means the applicable set is not determinable, and a review
over an indeterminate set attests to nothing. Ids are `[a-z0-9][a-z0-9-]{0,31}`
— bounded, and starting with something, because an id is typed at a command
line, printed in a refusal, and used as a filename.

Front matter is YAML read under a **restricted profile**, then a strict rule
schema. The profile is YAML 1.2 with duplicate keys, custom tags, anchors and
aliases all invalid, and only schema-required forms accepted.

The restriction is not fastidiousness, and a typed deserializer does not deliver
it: that resolves an alias into the value its anchor held, applies a tag, takes
the last of two duplicate keys, and hands back a document that no longer stands
in any recoverable relation to the bytes it came from. Rule Review identity is
**artifact-exact** — any byte-level change to a rule file changes the
ReviewDigest, including semantically equivalent reformatting — so two files
whose bytes differ must never be one policy, and an anchor is precisely a way of
writing one policy twice. Unrestricted, a person reading the file and a build
hashing it are looking at two different documents.

The profile MUST therefore be enforced **before** typed deserialization, because
afterwards the evidence is gone: the constructs have been resolved away and the
parsed value cannot say which of them produced it.

Beyond the profile, the layers stay distinct — YAML decides syntax, engr decides
whether the parsed document is a rule — and a field this version does not
understand is **refused rather than ignored**, because reading past it would
review against a rule only partly understood. The normative body is stored
exactly as written; emptiness is decided by refusing, never by rewriting.

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

**Attempts are counted from 1, and 0 is not a value.** A number below the first
attempt is not a quieter way of saying "not yet" — it is outside what the
protocol defines, and it is refused rather than answered. The failure it prevents
is the silent one: an evaluator handed 0 would report that nothing is exhausted,
which reads exactly like a successful policy result.

The scope of that number is **one active review sequence**, and saying so matters
because it bounds what the ceiling guarantees:

```text
same sequence          attempt = 1 -> 2 -> 3 -> ...
sequence abandoned,
lost, or restarted     a later independent sequence may begin again at 1
```

So `max_attempts` bounds repeated self-review within one continuous sequence. It
is **not a rate limit and not a security control**: nothing persists across a
restart, so cumulative exhaustion is not guaranteed and must not be relied on as
though it were. v1 adds no persisted review series, retry counter, reset or
abandonment record, and no pending-review resource — which is the same decision
seen from the other side, since any of those would be the durable counter this
is not.

### A long-lived proof carries its own contract version

A digest that must still be checkable years later is persisted as a **versioned
scalar**:

```text
<contract-version>:<digest>          1:<64 lowercase hex>
```

The version travels with the value because a bare digest says what it is worth
and not which calculation produced it. Both ways of guessing later are silent:
verify it under current rules and a mismatch looks like tampering, or relabel it
and claim a guarantee nobody made. A stored value may be relabelled under a newer
contract **only** if the calculation is exactly equivalent.

The scalar grammar is shared and owns only syntax:

```text
version := [1-9][0-9]*        positive uint32, 1..4294967295
                              0 is permanently reserved and never means
                              "legacy" or "unversioned"
digest  := lowercase hex      uppercase is invalid, never silently normalized
                              length is not fixed here
```

Contract versions are **field-local**. `ReviewDigestContract 1` and
`EventDigestContract 1` are unrelated contracts that share a number, and each
validates its own digest length — which is what lets one change hash algorithm
without touching the grammar or the others. Versions need not be contiguous.

**A number is never redefined, and that is not the same as a support promise.**
Changing what a version calculates always takes a new version, even while the
contract is experimental — otherwise one number would mean two things and no
stored value could be read with confidence. What a *stable* declaration adds is
the obligation to keep verifying: an experimental contract may be retired and its
verifier dropped, while a version explicitly declared stable for durable use, and
emitted under that status, must stay interpretable by later implementations.

Development or pre-release emission alone does not cross that boundary. Stability
is release policy and is deliberately **not** encoded in the scalar — a stability
bit in the persisted value would make the guarantee a property of the data rather
than of the release that made it.

Where such a scalar takes part in canonical ordering it is compared as the parsed
pair `(version, digest)`, not as text, because `2:` precedes `10:` and string
order gets that backwards.

**Supported is two questions, not one.** A contract may still verify values
written under it long after it has stopped being what new values are written
under; exactly one version per family emits at a time. So a well-formed scalar
naming a version this build does not know is **not malformed data** — it is
readable data this build cannot check, and it is reported that way. Only a
grammar violation is malformed.

**Verification recomputes under the version the value names**, not under the
current one. That is the difference between carrying a version and using one: a
build that only asks "is contract version 1 still supported" will accept a `1:`
value and then compare it against a recomputation under version 2 — those disagree by
construction, because two calculations that agreed would not have needed two
versions. The valid historical proof is then reported as a changed subject, and
the guarantee exists only in the support table. A version listed as verifiable
that this build cannot actually compute is refused rather than served the
current calculation, since being promised in the contract is not evidence that
an implementation has it.

### Unordered sets have one order

Canonical bytes for Rule Review are **RFC 8785 (JCS)**, not merely a stable
serialization. Every field whose semantics are a set — a rule's domains, its
bases, the applicable rule set itself — is canonicalized the same way before it
is hashed:

```text
1. JCS each element on its own
2. sort by the lexicographic order of those canonical bytes
3. reject canonical-equivalent duplicates
4. JCS the resulting structure and hash it
```

A review subject must stay inside the shared protocol integer domain
`-(2^53 - 1)..=(2^53 - 1)`. RFC 8785 can represent some larger integers exactly,
but this contract is cross-language and uses one range for every digest contract
and every persisted resource. Values such as
`9007199254740992` and `2^60` are refused rather than hashed; values needing that
precision are carried as strings. This prevents a subject from being accepted by
one implementation yet rounded, rejected, or represented differently by another.

The canonical *spelling* of every value inside that domain is still the
standard's, not the author's. JCS fixes number formatting as well as object-member
order, so a second implementation computes the same bytes for the same accepted
subject.

Naming a standard is the point: JCS orders object members by **UTF-16** code
units and fixes number formatting, so a second implementation in another
language computes the same bytes. A stable serializer gives determinism for one
implementation, which is a weaker claim wearing the same word — and a review
hash is exactly where the difference bites, because an attestation is meant to
be checkable by whoever recomputes it. The orders genuinely differ: `U+1F600`
precedes `U+E000` under UTF-16 and follows it under UTF-8.

Sorting by whichever field looks natural is the trap this replaces, and it is
not obviously wrong: bases sorted by `path` are deterministic and stable. They
are still in a different order, because canonical JSON sorts keys, so a basis's
bytes begin with `commit` and a pinned basis precedes a floating one whatever
the paths say. Two implementations, one sorting by path and one by canonical
bytes, would hash the same rule differently — and a hash contract that two
conforming implementations disagree about is not a contract.

The order a surface *shows* is a separate question with a separate answer: bases
are listed by path because that is what a reader is looking for, and the
applicable set is reported by id because an agent must be able to name the set
without first reproducing the hash.

Parsed policy uses **effective** values, but review identity also binds exact
Rule-artifact provenance. These two files have the same parsed rule semantics,
but they are different reviewed artifacts and produce different ReviewDigests:

```yaml
review:                          review:
  on_exhaustion: human_confirmation      max_attempts: 5
                                         on_exhaustion: human_confirmation
```

Any byte-level edit, including formatting or explicitly spelling a default,
invalidates the earlier review. This is deliberate: ReviewDigest proves which
artifact was reviewed, not merely one interpretation of it. The effective
policy is also *in* the identity, because it decides the outcome: the
same wording under a ceiling of 5 and under a ceiling of 1 is not the same
review, and one that escalates to a person is not one that refuses.

Because it is artifact-exact, the parsed semantics and the artifact provenance
MUST come from **one** read of the file. `.engr/rules` is deliberately editable
outside the workspace lock, so parsing the Rule and then reopening the path to
fingerprint it is two reads of a moving target: the binding would name one
file's normative text while claiming the next file's `content_sha256` and
commit. A binding must be one coherent snapshot or it must fail; it must never
be a mixed one.

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
escalates to the Human Gate, and otherwise it is refused.

**What escalates has to arrive with what is being overruled.** A human asked to
overrule a review is answering a question about that review, so the Challenge
subject MUST freeze it:

```json
{
  "review": {
    "digest": "1:<review-digest>",
    "result": "passed | failed | exhausted",
    "attempts": 6,
    "rules": ["<rule-id>"],
    "explanation": "<agent-generated explanation>"
  }
}
```

`explanation` is absent where there is none. `rules` is the applicable Rule-ID
set, for a person to read. The ReviewDigest lives **here** rather than in
history, because here is where it is still checkable: while the Challenge is
pending it binds the exact mutation against the exact Rule artifacts, and
confirmation rebinds and compares it.

The whole object is inside `subject.data`, so all of it is under
`Challenge.digest` — the thing somebody is overruling is part of what their
answer is bound to. Rendering a pending Challenge MUST use this frozen context;
a screen that recomputed any of it from live state would let a rule edited in
the meantime change what a frozen Challenge appears to say. Live Rule
recomputation is for staleness and failing closed, not for replacing what the
human was actually asked to consider.

**What history keeps is a different three facts**, and the difference is the
point. Where a Rule Review participated in an admission, the Event records:

```json
{
  "review": {
    "outcome": "passed | overridden",
    "result": "passed | failed | exhausted",
    "attempts": 6
  }
}
```

`outcome` says how the mutation got in — `overridden` is Human-only, because
only a human can admit something despite the review. `result` says what the
review concluded, which `outcome` alone cannot: overruling a failure and
overruling an exhausted ceiling are not the same act. `attempts` says which
attempt it was of. No Rule Review applied means the member is absent.

**The ReviewDigest MUST NOT be persisted here.** It binds an exact mutation
against exact Rule artifacts *at review time*, which is what makes it useful to
a Challenge and useless to history: once the Challenge is gone and the Rule
files have moved there is nothing left for it to be compared against, and a
field that can only be checked against material that no longer exists is not
provenance.

**The agent's explanation MUST NOT be persisted here either**, for a different
reason. It is decision-time material, written to persuade a person in a
particular moment, and the Challenge is where that moment lived. Keeping it in
history would quietly turn an agent's argument into the human's recorded
rationale, which is not a thing anybody wrote. Durable human rationale, if it is
ever wanted, is its own design rather than a reinterpretation of this one.

Live rules still decide whether the code is **stale**. They do not decide what
is displayed, and they do not decide whether the code is answerable at all —
that is the Challenge's own state, and it is asked first. Only a Challenge whose
pinned revision is still current and whose predecessor an admission may build on
may offer the confirmation instruction. One whose Object has moved offers none.
One whose admission is already durable offers the idempotent cleanup retry and
is **not** subjected to live Rule material, because current Rules do not
reinterpret an admission that already happened. Asking Rule freshness first
produces two contradictory screens — an instruction to confirm above a notice
that nothing can be, and a notice that nothing can be confirmed above the
instruction to retype the code.

`repair` carries **no Rule Review**, and every surface MUST mean the same thing
by that. It restores exactly what admitted history derives, so the projection is
identical either side and there is no proposed semantics for a Rule to judge. A
frozen `review: None` on a repair therefore does not assert that no Object Rule
applies, a Rule edited after such a Challenge was prepared cannot stale it, and
its state is measured against admitted history rather than against the stored
`rev` the corruption may itself have moved. Reading `review: None` as "no Rule
may apply" closed the one route back from an integrity-invalid Object in every
workspace that had any Object policy at all.

Escalation outranks
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
Removing a Backlog Section destroys it, so removal requires a review that
actually passed. A Section leaves only two ways, and both are reviewed: a consume,
or atomically as the source of a merge. Exhausted, neither happens, the Sections
stay exactly as they were, and **no marker is written** — nothing was admitted for
a diagnostic to describe.

**Collection and Work have no exhaustion behaviour in v1.** It is refused rather
than borrowed from another domain, because a composition that answers for a
domain nobody has decided is an invented rule that looks settled at the call
site.

That is a refusal, not an exemption. A `domain: collection` or `domain: work`
Rule that exists is applicable, and every mutation in that domain MUST pass the
same boundary as any other: the applicable set has to be **establishable** —
every basis readable, every ceiling known — and the caller's attested attempt has
to be inside those ceilings. Skipping it lets a Rule with missing material, or
one already past its limit, sit in a workspace while every mutation proceeds
exactly as if it were not there. These two domains have no prepared Challenge to
bind, so the attempt is the whole of what a caller attests.

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

Historical enumeration is anchored the same way. `-C <root>` gives git a cwd
prefix, so an unanchored pathspec — and unqualified output — are both read
relative to it: a workspace at `project/.engr` would be looked for under
`project/project/.engr`, and a real legacy snapshot would report itself as
having no Objects at all.

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

### An exact OID means the exact object it names

Every provenance value that carries a Git commit — a Rule or basis `commit`, a
selective Ref's `commit`, an `implemented_by` pin — names the exact native OID
whose content is the material in question. Reconstructing it MUST therefore be
independent of anything local:

- **Replacement objects MUST be ignored.** Git transparently substitutes
  `refs/replace/*` by default, so a persisted OID can resolve to different
  content on a machine that has a local replacement and on one that does not.
  That is not history moving; it breaks the content-addressed identity the whole
  proof rests on.
- **A persisted path is a literal path, never a pathspec pattern.** These paths
  are schema-valid repository-relative names and may legally contain `*`, `?` or
  `[`, which Git otherwise reads as magic. "The last commit that touched *that
  exact path*" is not "the last commit that touched something matching it".
- **A persisted path is repository-root relative**, including when the
  `.engr` workspace lives in a subdirectory. Historical lookup already resolves
  `<commit>:<path>` from the top level, so a worktree query that resolves the
  same string from the workspace directory answers about a different file — the
  same stored path then proves against one artifact and reports `dirty` from
  another. Internally constructed pathspecs that intend magic, such as the
  record's own exclusion, are separate and stay that way.
- **Historical bytes are bytes.** A lossy UTF-8 conversion turns an invalid byte
  inside a JSON string into U+FFFD before any parser sees it, so a malformed blob
  can be rewritten into a value whose seals then verify. Historical material is
  decoded from the exact bytes or refused as schema.
- **A historical resource is read under its attested generation.** A commit
  whose `.engr/VERSION` names this generation holds resources that had to satisfy
  this generation's persisted representation when they were written; becoming
  historical does not make an invalid encoding valid.

## Layout

`.engr/VERSION` is the sole generation authority for a current workspace. It
holds exactly one spelling — the digits, one newline, nothing else — because a
reader that accepts `" 1 "`, `"01"` and `"1"` alike has already conceded that a
workspace may say the same thing several ways, which is what lets two
implementations disagree while each believes it agrees. Current resource files
do not repeat it.

This is the first generation of the redesigned workspace, and its number is
unrelated to the predecessor's `format.json` version.

The single supported migration source is the officially released `latest`
workspace as it stood when this redesign began — release commit
`e7d9f99733407a8c31cec33af18a92480f4f4c6f` — recognized by its bootstrap:

```json
{"format":"engr-workspace","version":1}
```

That predecessor is read-only until `engr migrate` is explicitly run and
confirmed. Unknown and newer generations are never mutated and never read as
current authority.

Version 1 is the predecessor because it is the **released** generation: a record
a person built with the shipped binary MUST have a supported path forward under
this one, and reaching it MUST NOT require locating and running an intermediate
historical build.

"Released generation" is the whole of what it means here, and it is narrower
than "every workspace whose `format.json` said 1". Compatibility is promised for
persisted contracts that were actually published; a shared development-time
version number is not evidence that a schema introduced before the next bump
belongs to the released contract. A supported predecessor therefore owns exactly:

```text
format.json
objects/
events/
candidates/
```

`lock` is local process state and never migration authority. `rules/`,
`backlog/`, `work/`, `collections/` and `eventstore/` arrived in later builds
that still declared version 1, or belong to this generation. They are **not**
released version-1 state, and a predecessor workspace holding any of them MUST
fail migration closed, before a stage is installed and before anything is
written, with a diagnostic that names the domain.

The Rule case is the one that changes what the record can do, and it is why the
rule is a refusal rather than a quiet omission. A rule file is authored by a
human and never written by engr, so its presence beside a workspace says nothing
about which build made that workspace — and carrying it forward would make
policy the released build never recognized begin governing agent admission,
bought with nothing but somebody running `engr migrate`. Authority MUST NOT
arrive through a representation change.

Workspaces genuinely written by a later unreleased build are outside this path.
Preserving them is a separate compatibility decision and MUST NOT be inferred
from the shared version number.

That is one migration, not a chain. The migrator decodes the released
predecessor under the released predecessor's own rules and derives this
generation from that single validated source. Nothing is ever written in an
intermediate generation's spelling, which is what keeps a historical serializer
out of the permanent contract.

The predecessor schema is **enumerated, not inferred**. An implementation MUST
list the exact members the released generation persisted — for the Object, the
Section, the Event envelope and its payload, including which actions that
generation's reducer had — and MUST enforce that set **before** any decoding. A
model that carries several generations necessarily defaults the members the
older ones lacked, so a file validated only by prohibition decodes as a
well-formed member of a generation it does not belong to, and everything
downstream then reads those defaults as things the predecessor said.

### What the admitting path may do

Durable knowledge arrives through two paths — the Human Gate and Agent Rule
Review — and a Section records which one admitted its current semantics.

`Object.title` is **non-authoritative navigation and discovery metadata**. It is
what a listing prints so a reader can find the record; it is not identity, it
need not be unique, and nothing about what the Object *means* rests on it. After
the protocol-defined neutral initialization, the human-only Object metadata is
exactly `type` and `state`.

From that, and from a Section's own admission, the whole authority matrix
follows:

```text
agent MUST NOT carry a becomes destination
agent MUST NOT use change_state, classify, supersede or repair
agent MUST NOT delete a section admitted through the Human Gate
agent MUST NOT reword one either, which would leave assented
              wording standing as ungated
agent merge MUST consolidate only agent-admitted sections
agent sections MUST NOT carry relations[] or role=supersession
agent MAY create and rename a title
```

`type` and `state` are human-authoritative, and a field does not become
agent-writable because it is reached through a different action — which is why a
destination is admissible on the Human path and refused on the Agent one, on the
very same action.

**Deletion is decided by the current state, not by the envelope.** Whether
removing §3 is legal depends on what §3 currently is, which no record carries and
no schema can express. That rule therefore belongs to the current-state model,
and a reducer MUST NOT rely on a later envelope layer to make its own state
transition legal. For the same reason a projection MUST be closed under the
model's invariants: the Section a transition produces is checked as it is built,
rather than left for a later write to refuse.

### The migration

Migration is itself **Human-confirmed**, through the ordinary Challenge
primitive with `subject.type = migration`. There is no generic
migration-effects schema: each predecessor-to-destination pair owns the exact
frozen `subject.data` and the presentation a person reads before answering.

The order is preflight, then question, then publication:

1. compute and verify the predecessor's **effective current Object state** under
   the released historical contract, including any recoverable durable Event
   tail newer than the persisted projection. *Under* that contract, which is a
   constraint in both directions: the migration may not accept less than the
   release verified, and may not demand more than it wrote. Where the released
   build accepted a pruned history prefix or a missing history file beside a
   valid projection, so must this — refusing them would be a published contract
   strengthened after the fact, and a record somebody made with the shipped tool
   left with no way forward. Where a complete history *is* retained, the
   projection must be exactly what it derives, because migration is where the
   first aggregate seal is minted and an edit nothing can establish must not be
   granted one;
2. preserve stable Object and Section identities;
3. map historical Human-only Sections to `admitted.by = human`;
4. preserve historical `confirmed_at` as `Section.admitted.at`;
5. migrate Ref dependency semantics without adding later fields the original Ref
   never attested;
6. recompute this generation's digests only after predecessor validation;
7. use the released `events/` only to obtain and verify effective predecessor
   state, then **discard** that legacy history rather than translating it into
   the new Event vocabulary;
8. emit exactly one `object.migrated.v1` bootstrap Event per migrated Object at
   `rev = 1`;
9. the resulting migrated Object also has `rev = 1`;
10. Event metadata records the migration's Human confirmation and time, while
    nested Sections retain their original Human admission provenance;
11. old pending Human-Gate state is unsupported and is not migrated;
12. publish coherently and atomically or fail closed — a mixed
    predecessor/destination steady state is invalid.

Point 10 is why `Section.admitted` and `Event.metadata.admitted` are separate
concepts rather than one fact written twice. They normally align; migration is
the case that shows they are not the same statement. The Sections keep the
instant a person really admitted their wording, years before, while the Event
records the moment somebody confirmed the migration.

Before writing one authoritative Object, migration MUST preflight the whole
workspace: predecessor Object and Event reconstruction, every predecessor
Section seal, every Ref's complete transitive historical closure, and the shared
safe-integer bounds. Any ambiguous reconstruction, integrity failure or
unsupported value fails before the generation moves.

Every predecessor Object MUST be proven from its admitted history, and the set
of predecessor *projections* is a different set from the Objects the plan
publishes. A projection that is missing while its admitted history still
establishes it is the EventStore doing its recovery job: preflight rebuilds it,
and the commit phase MUST NOT then require it to have been on disk all along.
Comparing only the ids the predecessor's history happens to know leaves an
Object with no Event file uncompared, and its Section seals say nothing about
the Object level at all — not its title, state, revision, counter, or which
Sections belong to it. Granting that projection the first aggregate seal
launders something nothing can establish into current authority.

The predecessor bytes the plan is built from are the bytes the manifest records.
An implementation MUST capture the digest of each input **as it reads it**, not
by re-reading the workspace afterwards. The manifest is that captured set, and
the closing walk may only be asked whether it still agrees: a walk that
*becomes* the manifest promotes whatever it happens to find into "expected
predecessor", so a file that appeared after its own domain was enumerated is
named as validated when nothing ever validated it. Any divergence between what
was validated and what is on disk MUST fail rather than become the new expected
predecessor.

**The whole destination MUST be written outside the predecessor's own paths
before any of it is published**, and this is the invariant the transaction rests
on rather than a tidiness preference. The destination Object lives at the same
`objects/<id>.json` the predecessor occupies, so the first published Object
destroys bytes the preflight needs in order to decode a predecessor at all. From
that instant re-deriving is impossible, and a transaction whose only recovery
plan was to re-derive has none: a crash there leaves a workspace with no
`VERSION` and no predecessor to rebuild from, which is neither generation and
has no way back.

So publication is finishing forward from a staged destination, never starting
over. Recovery MUST NOT re-derive once publication may have begun, and MUST NOT
simply trust what it finds staged either — those are the two failure modes and
the staged material has to answer both. Each staged Object is checked back
against the digest the **confirmed subject** already pins, which is the same
claim re-deriving would have established; each staged bootstrap Event is checked
against the seal the transaction recorded when it wrote it, because that Event
carries a fresh id and the admission instant and so is pinned nowhere else. A
staged file that fails either check MUST fail closed rather than be published.

Every write in the publication is therefore the same bytes the stage holds,
which is what makes re-running it after a crash at any point converge rather
than compound. The predecessor's own directories go last, and only after every
staged artifact has been published may `VERSION` be written; cleanup happens
last. A conforming implementation MUST demonstrate resumption at each
publication step, because a boundary nothing crosses in a test is a boundary
nothing has checked.

### Migration is a maintenance window

An ordinary authoritative write keeps the shared-state rule this protocol relies
on everywhere else: a reader holding no lock observes a complete old state or a
complete new one, never a mixture.

A coordinated workspace migration is the one explicit exception.

```text
ordinary authoritative write
  -> old-or-new read invariant

coordinated workspace migration
  -> maintenance window
  -> current-workspace reads unavailable while the migration is incomplete
  -> never a mixed-generation interpretation
```

While a migration stage exists, current reads MUST fail closed rather than
interpret partially published resources. `unavailable` is therefore a normative
third outcome during migration, not incidental behaviour of one implementation:
a second implementation is required to refuse there too, and a reader is entitled
to treat the refusal as meaning the workspace is mid-migration rather than
damaged.

If the process dies during publication, reads stay unavailable until `engr
migrate` resumes and completes the transaction. That is the intended cost.

**The predecessor build must be locked out durably, and a lock cannot do it.**
The predecessor's writer lock is held by a process, so the moment a confirmed
migration is interrupted it is free — and a workspace whose bootstrap still
declares the predecessor generation is one the predecessor build is *entitled* to
write to. It would admit predecessor state legitimately, and the resume would
publish straight over it: the new writes discarded, or a newly created
predecessor Object left standing as previous-generation bytes after the history
it belongs to was removed.

So a confirmed migration MUST leave a durable marker the **predecessor** reads
before it decides it may write — its own bootstrap file, replaced by one that
generation cannot accept — and MUST leave it before the first published byte:

```text
destination staged and verified
  -> predecessor bootstrap replaced by the migration barrier   (durable lockout)
  -> publication
  -> VERSION
```

One window remains, between staging the destination and raising the barrier, and
a resume that reaches a staged destination **before publication has begun** MUST
establish that the predecessor did not move — the confirmed subject pins the
digest of every source file it was derived from, so this is a comparison rather
than a judgement. The barrier itself is the one intentional exception to that
map, and it is checked as a barrier bound to *this* transaction rather than
skipped.

**The barrier MUST NOT be what decides that publication has begun.** It is a
marker: it says the predecessor is shut out now, and it cannot say that anything
was established before it was written. A resume that skipped the source
comparison whenever the bootstrap merely looked shut would accept a forged
barrier, another migration's barrier, or a deleted bootstrap as proof of a check
nobody ran. Every marker has that defect, including one an implementation writes
into its own staging area — this protocol already treats self-consistent
persisted bytes and local staging material as claims rather than proof.

What may decide it is the **destination itself**, because publication is the
only thing that can produce it: the first Event stream it publishes, holding
exactly the bytes this transaction staged for that Object, under a path the
predecessor generation never had. A barrier is additionally bound to the
Challenge it belongs to and the generation it is on the way to, and one that
names anything else is refused rather than adopted.

Nothing has been published while that comparison is being made, so the
transaction stays withdrawable there and only becomes forward-only once the
predecessor is shut out.

Answering the migration's own code is the one thing that must stay reachable
while the stage exists, because resolving the workspace *is* a confirmation. So
`confirm` is exempt from the stage check and nothing else is; what a Challenge
may then do is still decided by its own family, and an Object confirmation is
refused mid-migration like every other mutation.

### Four ways a workspace can refuse to be read as current

They are different facts about a workspace and lead to different next actions,
so an implementation MUST keep them distinguishable rather than collapsing them
into one refusal:

```text
recognized released predecessor -> read-only, run `engr migrate`
incomplete migration            -> unavailable, run `engr migrate` to resume
unsupported generation          -> refused; a newer one needs a newer engr
malformed or corrupt            -> refused, and named as damage
```

A predecessor a build can migrate is not damaged and MUST NOT be reported as if
it were; a workspace from a generation the build has never heard of MUST NOT be
offered migration, because no migration exists to offer. In particular a build
MUST NOT hold a **newer** generation to its own persisted-representation rules:
that generation decides its own encoding, so reporting it as non-canonical
reports a workspace from the future as a damaged one.

Nothing but the migration reads a predecessor. Its Objects, Events and Refs are
a different schema, so a current reader that fell through to them would not be
lenient — it would be answering about files it cannot interpret. Every current
surface therefore refuses by name and says what to do.

### What the workspace generation is for

It governs how **persisted data is interpreted**, including data whose bytes do
not change. Rules are the sharpest case: a rule file with no `review:` block
carries an effective ceiling and an exhaustion action under this generation and
meant nothing under the released predecessor, which had no Rules at all. Two
builds accepting one workspace and disagreeing about what its policy says is the
failure this authority exists to prevent, so a build that does not know a
generation refuses the workspace instead of reading it under its own rules.

This is distinct from a digest contract's own version, which identifies a
deterministic calculation and changes when *that* calculation changes. A Rule
does **not** carry a schema version of its own; the workspace answers for it.

**The marker is written last, and that is true of creation as well as
migration.** Its presence is the whole statement that a workspace is this
generation, and nothing afterwards re-checks the layout it certifies — so
writing it before the layout is complete means an interruption leaves an
*active* workspace that is missing part of itself. The `/local/` ignore line is
the part that shows why this is not tidiness: a live Challenge's filename is its
code, and that line is what keeps `git add -A` from handing the code to everyone
with repository access. A workspace that activated without it is one where the
Human Gate's own secret is Git-trackable, and no later command can tell.

A prepared Rule Review attestation does not survive a migration that changes Rule
interpretation. It named a subject computed under the old semantics, so it is
stale by definition and must be prepared again.

A **historical** snapshot carries the generation that was current when it was
taken, and is readable at any generation this build recognizes. Refusing an
older snapshot would make every reference pinned before a migration unresolvable
— moving the workspace forward would retroactively break provenance that was
correct when it was recorded. A snapshot MUST be decoded under **its own**
generation's persisted schema rather than under the loosest one the build
recognizes, and the current decoder is never widened to accommodate one.

Migration **classifies nothing**. `status = open|closed` becomes
`state = open|closed` with no type, because the stored record does not contain
enough to infer one and a guessed classification is an engineering judgement
nobody made. Classifying an existing object later is an authoritative change like
any other, and passes the gate with a state valid for the type it is given.
Migration makes no new engineering judgement: every representation change
follows deterministically from verified predecessor state.

### The Event vocabulary is a generation of meaning

An Event type ends in a version — `section.updated.v1` — and that suffix
identifies a **generation of meaning**, not a count of schema revisions. The test
is whether two readers could both accept the same Event and derive different
authoritative meaning from it.

An additive change keeps the current suffix when an older reader either
interprets the Event without changing what it authoritatively means, or does not
understand the addition and **fails closed before accepting or replaying a
different meaning**. Rejecting an Event you do not understand is not
disagreement about what it means — it is the absence of a second opinion.

The suffix MUST change when an existing field changes meaning; when a valid
representation is removed in a way that changes what readers accept; when an
incompatible required field or envelope shape appears; when canonicalization or
hashing semantics change incompatibly; or when an older and a newer reader could
both succeed on one Event and disagree about it.

The envelope itself carries no generation number. The workspace does, and one
answer is what stops a stream and its workspace from disagreeing about which
rules a record was written under.

Admitted history is never rewritten to normalize versions. An Event says what
it said under the authority path that admitted it.

```text
.engr/
  VERSION                        "1", and nothing else
  .gitignore                     excludes /local/
  objects/<uuid>.json            the authority          commit this
  eventstore/
    objects/<uuid>.jsonl         append-only admitted history
  backlog/<uuid>.json            unresolved staging     commit this
  work/objects/<uuid>.json       execution memory       commit this
  work/backlog/<uuid>.json       execution memory       commit this
  collections/<id>.json          planning metadata      commit this
  rules/*.md                     project review policy
  local/
    lock                         one writer at a time
    challenges/<CODE>.json       awaiting a human       never commit this
```

Backlog is committed: git is its only history. A new optional directory is not
a schema change, so adding one does not move the workspace generation — a
workspace holding no backlog is byte-for-byte what it was.

`init` MUST write a `.gitignore` excluding `/local/`. A predecessor workspace
does not have that line, and its migration needs the exclusion in place before it
mints a code — but **preparing a migration MUST NOT change a tracked byte**.
Asking what a migration would do is not doing it, and somebody who asks and then
declines must be left holding no change they never confirmed. The exclusion is
therefore made where git keeps its own local state, and the tracked line is
written as part of the publication a human confirmed. `local/` holds
the writer lock, the pending Challenges and any migration stage: all of it is
this machine's state and none of it is shared authority. A Challenge's filename
is a live code, and `git add -A` is how a workspace gets staged — committing one
hands the code to everyone with repository access, which is not where a code the
gate expects a single human to return is supposed to go. The exclusion MUST NOT
cover `objects/`, since look-back is delegated to git.

Events are safe to commit. The challenge codes they carry have been spent, and a
spent code resolves to no Challenge.

## References

Object and future Backlog identities remain UUIDv7 values persisted as standard
UUID strings. Their reference form encodes the canonical 128 UUID bits as
exactly 26 lowercase Crockford Base32 characters using
`0123456789abcdefghjkmnpqrstvwxyz`, without padding.

Local standalone forms are `engr:obj:<id>`, `engr:obj:<id>:<section>`,
`engr:backlog:<id>` and `engr:backlog:<id>:<section>`. A Git snapshot selector
may follow as `@<commit>`; it selects an as-of snapshot and is not identity.
CLI arguments documented as standalone engr references consume these canonical
forms, including references copied from machine-readable command output.
Backlog `subjects[]` and `produced[]` name current resources, so they refuse it.
Embedded references omit `engr:` and pair their namespace-relative `ref` with
`kind: "engr"`. The shared parser owns syntax only: each caller decides which
resources and selectors are legal and what they mean.

A repository-qualified reference is a Git repository URI whose fragment is a
valid workspace-relative canonical engr reference:

```text
<git-repository-uri>#engr:obj:<id>
<git-repository-uri>#engr:obj:<id>:<section>
<git-repository-uri>#engr:obj:<id>:<section>@<commit>
```

The repository URI supplies resolution and location context, not resource
identity. The optional Git commit supplies snapshot context, not identity. The
grammar is canonical now; only full remote Git resolution is deferred.

Snapshot input may be abbreviated or symbolic, but it is unresolved input, not
canonical reference data. Canonical or persisted output MUST contain the full
resolved Git object ID; an unresolved selector cannot be rendered canonically.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | success |
| 2 | invalid usage, or a confirmation response that did not match |
| 3 | object, section, or challenge not found |
| 4 | malformed or unsupported stored data |
| 5 | a rule of the model was violated |
| 6 | the object moved after the challenge was prepared |
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
| Object-owned relations | A relation that belongs to no particular Section, and that no admitted Section can express naturally |
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
| A `done` state on a work sidecar | Something must distinguish "no items left" from "the work is over" that the subject's own state cannot say |
| A responsible person, estimates, deadlines or labels on work | Work has to answer a question a bounded handoff cannot, at which point it has stopped being execution memory |
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
far the only contender whose answers differ from `closed`'s.

A typed object already answers part of that: `rejected` and `invalidated` say
*why* something is out of the attention set, which is exactly the distinction
`abandoned` was reaching for. That is a reason to classify work rather than a
reason to widen the untyped vocabulary, and it is why the row above is now
narrower than it was.

A priority would be an authoritative decision like any other and would need an
explicit admission path. The test for whether it belongs is whether a reader
needs it as durable engineering meaning: if reviewing it would be tiresome,
that is the signal it is tracker data. An estimate of size is absent for a
different reason and is not expected back: it is a guess about the future rather
than a record of something agreed.

Path scoping would have to be a section field carrying into the content hash —
what a Section is *about* is part of what was admitted — and it MUST then be
omitted from the canonical form when empty, or every section already recorded
fails its own hash the day the field is added.

More than one action per confirmation MUST NOT be reached by allowing several
live Challenges for one object: each pins `expected_rev`, so confirming one
kills the rest. It would have to be one Challenge carrying several actions,
appending an event per action with consecutive revs — which leaves projection
untouched and keeps crash recovery working off the first event.

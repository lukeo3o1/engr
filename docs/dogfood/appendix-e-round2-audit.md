# Audit, round 2 — three items against RECORDING-DOCTRINE.md

Auditor's note on method. I worked the doctrine as it tells a reviewer to:
"Work items C1 through C12 in order, in writing, one at a time. For each item
answer `PASS` or `FAIL` and quote the exact words of the mutation that decide
it." Where Part C or Part D removes an item, I say so and skip it rather than
inventing an answer. Where an item's own test cannot be run on the kind of
mutation in front of me, I say that instead of guessing, and it is listed in
section 3.

Everything quoted below is quoted verbatim from the doctrine, the rules, the
stored files under `.engr/`, or the source.

---

## 1. Per-item verdicts

### Item 1 — backlog topic `01a0b7f5`, section 1

**Part applied: Part C.** The mutation is a Backlog point, and Part C says
"Backlog points are checked against C1, C4, C5, C6, C8, C9 and C10 only", with
"C2 applies as the subject association rather than an implementation relation."
So: C1, C2 (as subject association), C4, C5, C6, C8, C9, C10. C3, C7, C11, C12
are not on the Part C list. (That Part C's "only" list and its exceptions
paragraph disagree with each other about C2, C7 and C11 is a defect in the
doctrine, not in this mutation; see section 3.)

The wording under review, in full, from
`.engr/backlog/01a0b7f5-81cf-7972-b7ca-a7335042e790.json`:

> "Whether the pool should be divided between the tenants that are outstanding
> at the instant of the division, or between the tenants that have been
> outstanding across some recent stretch of time, is undecided. A tenant that is
> briefly neither being worked on nor waiting leaves the division for that
> instant, and another tenant can take the whole pool in it and hold it until
> its own work drains, so the tenant that stepped away for a moment waits behind
> all of that; but a stretch means the pool is divided between tenants that have
> nothing to run, and it stands idle in proportion to how long the stretch is."

Stored subjects: `"kind": "file", "path": "src/dispatcher.py"` and
`"kind": "symbol", "path": "src/dispatcher.py", "symbol": "Dispatcher"`, both at
commit `c9b87c22`. There are no other stored fields on the section beyond `id`,
`text` and `updated_at`.

**C1 — the quantity the decision is about is in the prose, in words. UNDECIDABLE
(recorded as vacuous PASS).** C1's trigger is "If the decision is about a number
a person agreed to". Nothing here was agreed to: the point's own words are "is
undecided". The one quantity in the wording, "some recent stretch of time", is
unspelled — which is verbally identical to C1's FAIL example, "released after
losing its turn a set number of times". But that example is of a *decision*
hiding an agreed number, and there is no agreed number here to hide. The trigger
condition cannot be satisfied by a backlog point, yet Part C lists C1 as
applicable. I record PASS because the antecedent is false, and flag the item.

**C2 (as subject association) — PASS.** The point is about specific code and
carries `"kind": "file", "path": "src/dispatcher.py"` plus
`"symbol": "Dispatcher"`. C2's second half asks: "If the assertion names
behaviour that one function provides, does it have the symbol relation too?" The
behaviour in question — deciding who the pool is divided between — is not
provided by one function: it is `demand_tenants()` together with `share()` in
`src/dispatcher.py`. A class-level symbol is therefore the correct grain, and one
is attached.

**C4 — the reason that makes the assertion decidable is present. PASS.** Test:
"can a reader who was not present act on this without asking anybody?" Yes: the
wording gives both horns and the cost of each — "another tenant can take the
whole pool in it and hold it until its own work drains" against "it stands idle
in proportion to how long the stretch is." A reader who was not present can take
this up and decide it.

**C5 — one assertion only. PASS, with the tension recorded.** Test: "is there a
second thing in this wording that a reader could separately agree or disagree
with?" Literally, sentence two makes two claims a reader could split on: that an
instant starves the tenant that stepped away, and that a stretch leaves the pool
idle. But the thing this mutation asserts is one thing — that this question "is
undecided" — and the two horns are the material C4 requires to make that
actionable. Splitting them into two points would leave neither decidable. PASS.

**C6 — no narration. PASS.** C6 bans "restating the problem, no history of how
the team got here, no options not taken, no description of what happens next."
There is no history and no next step in the wording. The two options are both
live and neither is "not taken" — that is precisely what "is undecided" means.

**C8 — no code-related material in the prose. PASS on the reading C8's purpose
supports; FAIL on its literal words. Flagged.** C8.1 forbids "identifiers —
module, class, function, method, variable, field or constant names, in any
spelling or casing", and C8 asserts "This list is exhaustive and each entry is
checkable by reading." The prose says "tenants" and "tenant". In
`src/queue.py`, `tenants` is a method name (`def tenants(self):`) and `tenant` is
a field of `Job` (`tenant: str = field(compare=False)`); in
`src/dispatcher.py`, `tenants` is also a local variable. On the literal words of
C8.1 — any spelling, any casing, exhaustive, checkable by reading — the prose
contains two identifiers and FAILS. I do not record that as my verdict, because
the same reading condemns every admitted Section in this record (object
`01a0b58f` §1 opens "A tenant's permitted share of the worker pool…") and would
make the doctrine unsatisfiable for a system whose domain noun is *tenant*. Read
by purpose — "Where each kind belongs instead: … a symbol goes in the
implementation symbol relation" — a domain noun that a field happens to be named
after has no other field to go to. PASS, and see section 3.

Nothing else in the prose trips C8: no paths, no flags, no literals, no code
fences, no excerpt.

**C9 — no laundering. PASS.** Test: "for every code-related thing the change is
about, name which field now carries it. If the answer for any of them is 'the
header' or 'the title', this item FAILS." The file goes to the subject
association, the symbol to the subject symbol, the commit to `"commit":
"c9b87c2299155e9a4f91b5211f05c941aeb793fe"`. The title, "Whether the pool is
divided at an instant or over a stretch", carries none of them.

**C10 — not vague. PASS.** Test: "read the prose alone and state what a reader is
now supposed to do or believe." A reader is to believe that the divisor for the
pool is not settled, and to settle it by weighing a momentary departure costing a
tenant its place against idle capacity proportional to the window. The one
unquantified phrase, "some recent stretch of time", is unquantified because
choosing it *is* the open question.

**Result under Part C: PASS on all applicable items (C1 vacuously, C8 with the
literal reading noted).**

#### The title of the topic

Part D says "An Object title is navigation, not an assertion. It is checked
against C8 and C9 only, and it must be short." A backlog topic title is not an
Object title, and the doctrine's scope line — "It governs Object Sections and
Backlog points" — never mentions backlog titles. Applying Part D by analogy:
"Whether the pool is divided at an instant or over a stretch" is short, is not a
complete sentence with its reason, and carries no code material. PASS by
analogy; the doctrine does not actually say this check exists.

---

### Item 2 — pending candidate `CONFIRM EHU2BP`

**Part applied: Parts A and B in full (C1–C12).** This is `section.create` on
object `01a0b58f`, role `decision` — an Object Section, so Part C does not
narrow it and Part D does not apply.

The wording, verbatim from `engr candidate EHU2BP`:

> "Nothing about a tenant is kept once it has no work outstanding: the moment its
> last piece of work finishes with none of its own still waiting, it is dropped,
> and the time for which anything about it is kept beyond that is zero. Any
> longer would divide the pool between more tenants than are asking for it and
> leave part of the pool unused for exactly as long as that time lasted, and no
> length can be chosen well, because one short enough to waste nothing is too
> short to change anything."

Fields: header "Lifetime of a tenant"; content `[0] code.lifetime` (an excerpt
of the body of `Dispatcher.complete`); `implemented_by -> file
src/dispatcher.py @c9b87c22`; `implemented_by -> symbol src/dispatcher.py ::
Dispatcher.complete @c9b87c22`; `Based on c9b87c22`. **No `refs`.**

**C1 — PASS.** The decided quantity is in the prose as a word: "the time for
which anything about it is kept beyond that is **zero**." Cover every other
field and the prose still says what the number is. This is a number a person
decided, not one the code picked, so C1 is the right home for it and it is
there.

**C2 — PASS.** "does the mutation have a `implemented_by` file relation?" Yes:
`implemented_by -> file src/dispatcher.py`. "If the assertion names behaviour
that one function provides, does it have the symbol relation too?" It does, and
the function is the right one: `implemented_by -> symbol … Dispatcher.complete`,
and `complete` is where the drop happens (`self.running.pop(job.tenant, None)`).

**C3 — FAIL.** This is the decisive failure. C3: "If the wording relies on what
another Section asserts, the mutation **must** depend on it through a selective
reference naming the exact fields relied on." The wording relies on §1 of the
same object: "Any longer would **divide the pool between more tenants than are
asking for it**" presupposes §1's admitted assertion that "A tenant's permitted
share of the worker pool is derived from every tenant that has work outstanding".
Without §1, the second sentence gives no reason at all — there is nothing to say
why an extra counted tenant costs anything. The candidate carries no reference;
`engr candidate EHU2BP` lists `Relation` lines only, and object §2 by contrast
shows a `refs 01a0b58f §1` line, so the field exists and is unused here. C3's
FAIL example is exactly this shape: "a criterion that restates the decision it
constrains, with no reference."

The candidate's own Reason field states the same thing and states why it cannot
be fixed: "C3 requires a selective reference onto section 1, whose share basis
this wording rests on. With the reference, engr refuses the command: a human
section may reference only human-admitted authority, and section 1 is
agent-admitted. Without the reference, C3 fails."

**C4 — PASS.** The reason is present and a reader can act on it: "Any longer
would divide the pool between more tenants than are asking for it and leave part
of the pool unused for exactly as long as that time lasted."

**C5 — PASS, narrowly.** The admitted assertion is one: the lifetime is zero.
"no length can be chosen well, because one short enough to waste nothing is too
short to change anything" is a separately disputable sentence, and a strict
reader could call it a second assertion; I read it as the closing half of the
C4 reason rather than a thing being admitted on its own.

**C6 — PASS on C4's precedence; FAIL on the literal words. Flagged.** C6 bans
"options not taken". The entire second sentence is the rejection of an option not
taken — a nonzero lifetime — and contains no other content. But C4 demands "a
sentence that gives the reason the assertion follows", and for a decision whose
content is *zero*, the only available reason is why a nonzero value is wrong.
C6 read literally forbids the only wording C4 permits. I record PASS and flag
the collision in section 3. There is no history, no problem restatement, and no
"what happens next" in the prose.

**C7 — PASS.** "No wording about what a later change might do." The prose
contains none. (The sidecar's dependency, "If the pool is divided over a stretch
rather than an instant, this lifetime is revised", is in the work sidecar, not in
this mutation.)

**C8 — PASS, subject to the same caveat as item 1.** No paths, no commands, no
flags, no code fences, no excerpt in the prose; the excerpt is where it belongs,
in "content [0] code.lifetime". The only numeral-bearing word, "zero", is C1
material, which C8.4 explicitly carves out: "numbers that belong to the
implementation (see C1 for the numbers that belong to the decision and MUST be in
the prose)". "tenant" is again literally a field name; same reading as item 1.

**C9 — PASS.** Excerpt → `content [0] code.lifetime`; file →
`implemented_by -> file`; symbol → `implemented_by -> symbol …
Dispatcher.complete`; commit → `Based on c9b87c22`. The header, "Lifetime of a
tenant", carries none of it.

**C10 — PASS.** Read alone, the prose tells a reader what to believe: a tenant
with no outstanding work retains nothing, immediately.

**C11 — FAIL, on the same words as C3.** "Nothing another live Section asserts
is repeated here (use C3 instead)." The clause "divide the pool between more
tenants than are asking for it" repeats what live §1 asserts, and the
parenthesis names the remedy that is missing. No contradiction with any live
Section: §1, §2 and §3 remain true alongside it. I report this as one defect
seen twice, not as an independent second one.

**C12 — PASS.** The assertion itself is settled — the sidecar records "State the
pacing-state lifetime invariant and pin it with regression tests" as `"state":
"done"` with sixteen passing tests (I ran them: `Ran 16 tests … OK`). That it
would be revised if the open division-window question resolves the other way does
not make it unsettled; the open question is in the backlog, which is C12's own
prescribed destination.

**Result: FAIL at C3** (and at C11 on the same words). The doctrine's front
matter is unambiguous about the consequence: "If any item is `FAIL`, the whole
review fails; say which item and why." **C3.** The candidate's own notice is
accurate: "confirming this admits work no passing review allowed."

---

### Item 3 — the work sidecar on `01a0b58f`, and plan membership

**Part applied: none — this is my reading, not a finding.**

The doctrine's scope sentence is "Single source of truth for what may be written
into the paceq engineering record. It governs Object Sections and Backlog
points." A work sidecar is neither. Parts A–D never name it: Part C is headed
"how these items apply to the backlog", Part D is "the title". Both rules are
declared `Governs backlog, object` — neither declares a `work` domain, so no
review gate reaches this material at all.

My reading: the doctrine deliberately does not govern the sidecar, and the
sidecar's own banner says why — "EXECUTION MEMORY — agent-managed, admitted by
nobody, and not what the record says", as the collection's banner says
"PLANNING — agent-managed, admitted by nobody, and says nothing about what its
members mean". Applying C1–C12 to it would be applying a rule to a thing the rule
excludes by its own scope line. I therefore give no PASS/FAIL verdicts here, and
report accuracy instead, which is the only thing that can be checked.

What is stored (`.engr/work/objects/01a0b58f-….json`) and what I could verify:

- `"summary": "The lifetime of a tenant's pacing state is decided, implemented
  and tested; the wording is a pending Human candidate, EHU2BP. Nothing is
  admitted yet."` — accurate. EHU2BP renders as pending, and `engr show
  01a0b58f` lists three sections, none of them the lifetime.
- item 1 `"result": "Sixteen tests pass. …"` — verified: `Ran 16 tests in 0.001s
  / OK`, and `tests/test_fairness.py` defines exactly 16 `def test_`.
- `"blockers": [{"reason": "The wording needs a person to answer EHU2BP; no
  combination of fields passes both C2 and C3"}]` — consistent with what I found
  independently at C3, though it is a claim about the tool's refusals that I did
  not exercise (I was instructed not to run `prepare`).
- `"dependencies"` → `backlog:01m2vzb0eff5sbfjn76d845swg` with reason "If the
  pool is divided over a stretch rather than an instant, this lifetime is
  revised" — that reference resolves to backlog topic `01a0b7f5`, item 1 of this
  audit. Coherent.
- Stored top-level `"state": "active"` renders as `State blocked` in `engr work
  show 01a0b58f`. I take the displayed value to be derived from the presence of a
  blocker; it is not a doctrine matter, but the stored and displayed words differ
  and a reader of the raw file should know it.
- item 3, `"Classify this object; it is still untyped and open, and that is
  human-only"` — consistent with `engr show`, which prints no type for the
  object.

Plan membership (`engr collection show pacing-fairness`): the object sits at
order 10, "The defect the rest of this plan builds on"; the new backlog topic at
order 40, `[low] unresolved`, "Raised by this fix and unresolved; it would revise
the lifetime decision, not the fix". That matches the sidecar's dependency and
does not assert anything about admission. Nothing in the collection claims the
lifetime decision is in the record.

One observation, offered as such: the sidecar's blocker text quotes doctrine item
numbers ("both C2 and C3") back at the reader. If this were a Section, C11's
first clause — "Nothing that this doctrine says is quoted back into the record" —
would be in play. It is not a Section, and the banner disclaims it, so I raise no
finding.

---

## 2. Was the 12/12 PASS on the backlog point correct?

**No — though not because a substantive check was missed.**

On the merits, I could not find an item that should have failed. Working Part C's
list in order, every applicable item passes, C1 vacuously and C8 on the purposive
reading. That is a real clean pass and I record it as one.

The verdict is nonetheless wrong in form, and the doctrine makes it impossible to
be right. Part C says, of backlog points:

> "Backlog points are checked against C1, C4, C5, C6, C8, C9 and C10 only."

Seven items, plus C2 in the modified form the next paragraph grants it — eight at
most. Five of the twelve answers (C3, C7, C11, C12, and C2 in its unmodified
form) were answers to items the doctrine says are not to be checked here. The
rules say the same: `mss-ssot` is "Seven answers" but adds "Where the mutation is
a backlog point, apply Part C of the doctrine: C3, C12 and C11's second clause do
not apply there" — four answers, not seven. With `prose-purity`'s five, a backlog
point admits at most nine written answers under the rules as written. Twelve
cannot be produced by following them.

And the front matter demands exactly the thing Part C forbids:

> "A review that does not produce twelve answers is not a review of this
> doctrine."

So a reviewer of a backlog point must either produce twelve answers and violate
Part C, or obey Part C and fail the front matter's definition of a review. The
12/12 verdict is the front matter's answer. It is not Part C's. That is a defect
in the doctrine, and the reviewer who produced twelve PASSes inherited it — but
the verdict as recorded asserts that C3, C7, C11 and C12 were checked and passed
on a mutation where three of those four have no meaning, and the record now
carries that claim.

## 3. Items that were unanswerable or ambiguous as written

**C1, on a backlog point.** Its trigger is "a number a person agreed to". A
backlog point is by definition a thing nobody has agreed. Part C nevertheless
lists C1 as applicable. The test — "cover every other field. Reading the prose
alone, can you say what the number is?" — returns "no" for "some recent stretch
of time", which is word-for-word the shape of C1's FAIL example, "a set number of
times"; yet failing it would forbid ever staging an open question about a
quantity, which is what a backlog point is for. I recorded vacuous PASS. The
doctrine does not say which it is.

**C2, on a backlog point.** Part C's list says C1, C4, C5, C6, C8, C9 and C10
"only" — and then its next paragraph says "C2 applies as the subject association
rather than an implementation relation." The two sentences contradict each other.
I applied C2; a reviewer who read only the "only" sentence would not, and would
be equally faithful to the text.

**C7 and C11, on a backlog point.** Same contradiction from the other side. The
"only" list omits both. But the exceptions paragraph disapplies only "C11's 'no
repeating another Section'" — naming one clause of C11 implies the others still
apply — and says nothing whatsoever about C7. So C11's first and third clauses,
and the whole of C7, are simultaneously excluded by the "only" list and left
standing by the exceptions paragraph. Undecidable as written.

**C8, on any wording in this project.** C8.1 forbids "identifiers — … variable,
field or constant names, in any spelling or casing" and declares the list
"exhaustive and each entry is checkable by reading". The domain noun of this
system is *tenant*, and `tenant` is a field of `Job` while `tenants` is a method
of `Queue`. Read literally, C8 fails every mutation in this audit and every
Section already admitted. The doctrine offers no test that distinguishes a domain
word from an identifier spelled the same way, and C9's remedy — "name which field
now carries it" — has no answer, because there is no field that a domain noun can
be moved to. I could not decide this item from the text; I recorded PASS on
purpose and FAIL on letter, and I would not defend either as *the* reading.

**C6 against C4, on the EHU2BP wording.** C6 forbids "options not taken"; C4
requires "a sentence that gives the reason the assertion follows". For a decision
whose content is that a duration is zero, the reason *is* the rejected nonzero
option. The doctrine does not say which item wins, and nothing in it warns that
the two can collide. I recorded PASS and named the collision.

**Backlog topic titles.** Part D is written for "An Object title". No part of the
doctrine says whether a backlog topic's title is checked at all, and the scope
line covers only "Object Sections and Backlog points". I applied Part D by
analogy, having nothing else.

## 4. Numbering versus principles

The numbering earned its place exactly twice, and both times on presence rather
than on wording. C3 gave me a verdict on EHU2BP that no amount of reading the
prose for quality would have produced: the sentence is well made, the reasoning
is sound, and it is still missing a field, and having a numbered slot for "the
fact you are leaning on lives in another Section and you did not say so" turned a
vague unease into a citable failure with a named remedy. C2 and C9 did the same
work on the backlog point — I could check "is the symbol association there" in
seconds and be certain I had checked it, which is the whole promise of a list.
Everywhere else the numbering cost more than it returned. It forced three
verdicts I do not believe: a vacuous PASS on C1 because the item's trigger cannot
fire on the kind of thing Part C points it at; a PASS on C8 that the item's
literal words forbid, because the word "tenant" cannot be moved to a field and
the list that claims to be "exhaustive and checkable by reading" is neither; and
a PASS on C6 that only survives because C4 overrules it, which the doctrine never
says it may. Worse, the numbering produced a *count* — "A review that does not
produce twelve answers is not a review of this doctrine" — and a count is a
target. Twelve PASSes on a backlog point reads as thoroughness and is in fact the
opposite: five of them are answers to questions Part C withdrew, and the only way
to notice is to stop counting and read Part C against the front matter, which is
prose reasoning about the checklist rather than use of it. Judging wording
against principles risks a reviewer passing what they meant rather than what is
there; judging it against a numbered list risks a reviewer producing the required
number of answers rather than the true ones. This document got the second failure
and got it in a form that looks, on the page, like a perfect score.

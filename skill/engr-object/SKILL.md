---
name: engr-object
description: >-
  Use before any `engr prepare`, `engr confirm`, `engr candidate`, `engr
  changeset`, `engr repair`, `engr show` or `engr ls` in a repository that has
  adopted `.engr/`. Recording a decision, constraint or fact as a Section; the
  Human challenge flow, and never confirming your own candidate; direct Agent
  admission after a delegated review against the surfaced ReviewDigest; several
  Sections under one review with a ChangeSet; writing for a reader who was not
  there; and what a drifted or tampered Section means. Read the engr skill
  first.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr-object

The record: Objects, the Sections they hold, and the two paths a Section is
admitted through. When something belongs here and when to write it is in the
`engr` skill; this one is how.

## The loop

```bash
engr prepare --object <id> --add --text-file draft.txt
```

1. **Prepare.** engr prints the change and a code.
2. **Show the human the change**, as engr rendered it. Do not summarise it — the
   wording they are assenting to is the wording that gets recorded.
3. **Wait.**
4. **They give you the code** → `engr confirm 'CONFIRM ABC123'` with exactly what
   they gave you.
   **They raise a problem** → prepare a new candidate. Do not argue the old one
   through.

Pass the response through **verbatim**. If they write `CONFIRM ABC123 but tighten
the second line`, send that whole string. engr will refuse it and discard the
candidate, which is correct — that was a qualified yes, and deciding it counted
as a yes is not your call.

## Writing for a reader who was not there

Everything engr keeps is read later by somebody who has only the page — a
Section, a title, a backlog point, a sidecar, a plan's reason. You write it out
of everything you know, and when you read it back, what you know fills in
whatever the page left out. So the draft looks complete to you and to nobody
else, and reading it again more carefully does not help: the second reading has
the same knowledge as the first.

The gaps are not random. Cold reviewers keep finding the same few, and each was
invisible to the agent that wrote it:

- **The reason stayed with you.** "When a job repeatedly loses its turn" read as
  a justification to the agent that knew the argument. It is a scope clause. If
  no words on the page say *why*, the reason is not on the page.
- **The number stayed with you.** "Released after losing its turn a set number
  of times", and in the next candidate "the agreed number of times" — the author
  knew the number and the record never said it. A quantity somebody decided is
  written out.
- **Several claims read as one.** Three assertions in one Section, read as one
  "because I meant them as one". If a reader could accept part and reject the
  rest, it is more than one.
- **The story of the work instead of its result.** "Nobody has weighed the two
  against each other here" says what the analysis has not done yet. What was
  tried, who looked, and what comes next are not what is unresolved.
- **Written against the unit, not its neighbours.** A backlog revision that
  quietly ruled out its sibling point, because its author was looking at one
  point and its reviewer at the topic. A sidecar still reporting a candidate as
  pending a session after the human had answered it.
- **Material moved instead of placed.** A required header typed into the prose;
  whole functions copied in as excerpts because the field that says where code
  lives was not open on that path. A substitute field is still the wrong field.
- **Vague after a fix.** A flagged word replaced with an emptier one, until the
  wording is clean and a reader can no longer say what to do.

So do the work in an order that asks those questions before there is wording to
defend:

1. **Choose where it goes.** Settled is the record, unresolved is the backlog,
   where execution stands is a sidecar, and a passing thought is nowhere.
2. **Read what it sits beside.** The whole Object or topic, not only the section
   you are changing; and for a sidecar, the current state of everything it
   claims.

   ```bash
   engr show <id>
   engr backlog show <id>
   engr work show <subject>
   engr candidate
   ```

3. **Write the content down before the wording** — for anything that asserts,
   a few lines in a scratch file, never in engr:

   ```text
   claim      the one thing a reader should believe or do
   because    why it holds, in words a stranger could check
   quantity   any number a person decided — or "none"
   source     the code it is about, and the field that will carry it — or "none"
   rests on   the Section and the fields it depends on — or "none"
   beside     what nearby it could contradict, repeat or rule out
   ```

   "none" is a real answer. The note is not for engr and nobody attests to it.
   It exists so that, before the draft does, there is something to compare the
   draft against other than your own reading of it.
4. **Draft from the note**, one claim per unit. Everything that is not prose
   goes in the field that owns it. When that field is not open on your path —
   an agent-admitted Section carries no implementation relation — report that
   rather than putting the material somewhere else.
5. **Have the screen read cold.** Take what engr renders — the first
   `--agent` call, the candidate, or `show` on what you just wrote — and hand a
   reader that never saw the draft *that screen and nothing else*: no Rule, no
   note, no account of what you meant. Where engr renders nothing before a
   review — a governed backlog mutation, a governed Human `prepare` — give it
   the exact command and the files it reads instead, and say that is what it
   is. Ask exactly this:

   ```text
   1. In one sentence of your own: what does this ask a reader to believe or do?
   2. Why? Quote the words that give the reason, or say there are none.
   3. Is there a quantity somebody decided? Quote it, or say there is none.
   4. List each thing it asserts that a reader could accept or reject alone.
   5. What would you have to ask the author before acting on it?
   6. If the screen shows more than this change: does the change contradict,
      repeat or rule out anything else on it?
   ```

   Compare the answers with the note, line by line. Every difference is a
   fact about the page, not a disagreement to argue: the reader had only the
   page, and so will everyone after you. Fix the wording, and ask a reader who
   has not seen this version. The page carries what you meant when the answers
   match the note and the last two come back empty.
6. **Then review it against the Rules**, below. Check the mechanical
   requirements by doing rather than reading: count the words against a ceiling,
   point at the question mark a Rule asks for, find the header in its field on
   the screen, and the relation or reference your note's `source` and `rests
   on` named — those are fields rather than wording, so a paraphrase never
   notices they are missing. Your reading passes all of these; counting does
   not.

**The cold read is drafting, not Rule Review, and it is not an attempt.** It is
never shown a Rule and it returns no verdict — it says what the page says, and
you decide whether that is what you meant. That is why it can be repeated
without spending the ceiling, and why it stops being free the moment it is
handed a Rule or asked whether the wording passes. Then it is a review, and it
counts.

Spend a subagent on it where a mistake is expensive: every Section and every
Human candidate, because a failed review is one attempt out of a small budget
and a rejected candidate is a person's reading spent; and a backlog point that a
Rule governs or that somebody will decide from. A title asserts nothing for the
questions to find; what fails there is mechanical, and counting is its check. A
sidecar entry or a plan's reason does not need a reader either — what goes wrong
there is staleness, and reading what it names is what catches it.

## Agent admission

An Agent semantic mutation needs at least one applicable, usable Object Rule and
a passing review. Start with the exact intended command plus `--agent`. engr
does not write it yet: it surfaces the ReviewDigest and every Rule id governing
that exact predecessor and result.

```bash
engr prepare --object <id> --add --text-file draft.txt --agent
#   open  [decision] The thing being decided
#
#   ── §2 [decision] Sync Budget ──
#   Because ...
#       based_on none
#
#   NEEDS REVIEW  governed by <rule-id>. ... --review <digest> ...
#
#   Nothing has been written.

engr rules show <rule-id>
```

Read every surfaced Rule and every file it rests on, then review the exact
mutation.

**Do not review your own wording yourself. Delegate it.** The first `--agent`
call prints exactly what would be stored and writes nothing — hand a subagent
*that screen*, plus the Rule text and every file the Rule rests on, and nothing
else. No draft history, no explanation of what you were going for, no opinion
about whether it passes. Ask for one answer: does this meet every requirement,
and if not, which one does it miss.

Nothing that asks for agreement, either. "This was verified independently" and
"this is the final attempt" both appeared in one watched run's review prompts; a
reviewer told either has been told what answer is wanted.

Have the screen read cold first — see
[Writing for a reader who was not there](#writing-for-a-reader-who-was-not-there).
A review is an attempt against a ceiling of a few, and spending one to learn
that the reason never reached the page spends it on something the cold read
finds without costing any.

**Hand over the screen, not the prose.** A Section is more than its text: the
header, the role, the supplementary content and the basis are all inside the
seal, and a Rule may require any of them. An agent that described the wording to
its reviewer by hand was correctly told the two-word header its policy required
was missing — and "fixed" it by moving the header into the first line of the
prose. The header field stayed empty, the wording gained a paragraph the policy
forbade, and the re-review, handed the same partial view, passed it. The screen
is the whole value; a retyped summary of it is not.

You will pass your own work. Not dishonestly — you have just written the thing,
so you read what you meant rather than what is there, and the requirements you
miss are the mechanical ones you would catch in anybody else's text. Watched
under a policy that fixed a six-word ceiling on a title, an agent wrote seven
words and attested `passed`; under one that required a question, it wrote a
statement and attested `passed`. Both had read the rule minutes earlier. A
reader who never saw the draft catches both in a sentence.

The subagent's answer is what you attest to. If it says no, fix the wording and
count the next attempt honestly — a failed delegated review is a real attempt,
not a rehearsal.

This is a practice, not a mechanism: engr cannot tell who reviewed, and an
attestation says a review happened rather than who ran it. It is worth doing
because it is the only part of the review a second reader can actually improve.

If it passes, repeat the unchanged command with the complete attestation:

```bash
engr prepare --object <id> --add --text-file draft.txt --agent \
  --review <digest> --reviewed-rule <rule-id> \
  --review-attempt 1 --review-result passed
```

Repeat `--reviewed-rule` for the whole surfaced set. engr recomputes the
ReviewDigest while holding the writer lock; if the Object, Rule bytes, or any
reviewed basis moved, read and review again rather than copying the new digest.
A passing Agent admission writes immediately and returns no challenge.

`prepare --new` takes the same two steps. engr mints the object id while it
performs the create, so the id is not part of what you reviewed and you carry
nothing but the digest between the two attempts.

If review fails, fix the proposed work and count the next review attempt
honestly. Once the applicable ceiling is exceeded, report `exhausted`. A Rule
whose exhaustion policy is `human_confirmation` may then produce a Human
candidate only when the attestation includes the exact explanation the human is
being asked to override. A `reject` policy ends the autonomous path; do not
route around it through an ordinary Human candidate.

Creating or renaming a title is the only Agent operation allowed without an
applicable Rule, because the title is navigation metadata. All Section semantics
still need governed Agent admission.

### Several Sections of one Object, one review

A review costs a reader who never saw the draft, and paid once per Section it is
what pushes decisions to the end of the work or out of the record. When one
piece of work settles several things about the same Object,
add each as a step of a ChangeSet when it settles, and pay for one review when
you apply them:

```bash
engr changeset new --object <id>
engr changeset add <changeset> --add --header "Lock Once" --text-file lock.txt
engr changeset add <changeset> --revise 2 --text-file budget.txt
engr changeset show <changeset>
#   CHANGESET  <changeset>  for <id>  rev 4 → 6
#      1  add     §5  Lock Once
#      2  revise  §2  Sync Budget
#
#   ... the Object as the last step leaves it ...
#
#   NEEDS REVIEW  governed by <rule-id>. ...
engr changeset apply <changeset> --review <digest> --reviewed-rule <rule-id> \
  --review-attempt 1 --review-result passed
```

A step takes the same arguments as `prepare` and is checked the same way the
moment you add it; one that would be refused alone is refused there, by its
number. Hand the reviewer the `show` screen — every step and the whole Object —
as you would the screen of a single mutation, and ask for a verdict on each
step. If it failed some, apply with `--review-result failed` and a
`--failed-step <n>` for each: the steps it passed are admitted, and the failed
ones stay. To fix one, `engr changeset rm <changeset> --step <n>` and add the
corrected wording.

**A step that failed is on its next attempt when it is reviewed again** — in the
ChangeSet or alone. Taking it out does not start its count again. One run whose
ChangeSet had failed three times took the steps out and admitted each at attempt
1, past a Rule that asked for a person after three; the count is yours to keep,
because engr cannot. A step that runs out of attempts goes alone to `prepare
--agent` at its next attempt, and the Rule's exhaustion policy decides.

A ChangeSet takes Section work on one existing Object and nothing else — not a
title, not a lifecycle move, not another Object — and a step may not reference a
Section the same ChangeSet changes: apply, commit, and reference what it wrote.
A step that revises a Section another step adds cannot be admitted if that other
step fails; apply refuses it rather than guess.

A step is on disk the moment it is added, and survives a lost context on this
machine — but it is not a record. The next session will not see it in `engr
show`, only in `engr changeset ls`, and the hooks in `skill/hooks/` print that
list at the start and ask once before stopping while one is unapplied. Apply
before the work it records is committed, not at the end of the session.

## Coming back later

A human often replies hours later, and your terminal output is gone.

```bash
engr candidate            # what is pending, and whether it is still live
engr candidate ABC123     # render it again, in full
```

**Do not re-run `engr prepare` to show it again.** That mints a new code and voids
the one they are holding.

## Reading the record

What to run at the start of work is in the `engr` skill. This is what the
listings and `show` do and do not tell you.

**A search that finds nothing is not proof that the record holds nothing.**
`engr ls` and `engr ls --all --sections` read stored projections only — that is
what keeps them cheap, and why every row says `unchecked` — so wording admitted
in a crash tail the projection never caught up to does not appear, and an object
whose projection is missing is not listed at all. `unchecked` is about how far a
row can be trusted; it says nothing about which rows are absent. Before
concluding that something was never recorded, run `engr ls --verify` to discover
what the record holds but the projections do not, and `engr show <id>` for the
effective wording of anything you are about to build on. `engr verify` is the
one that decides whether the record adds up at all.

```bash
engr ls                              # what needs attention
engr show <id>                       # sections, and how far each can be trusted
engr show <id> --format json         # the same, structured
engr ls --all --sections | grep <term>
```

Both `show` surfaces print the object's canonical reference, and the structured
one gives each section its own. That is the string every flag that takes a
reference wants — `--ref`, `--subject`, `work depend --on`,
`collection add --target` — so read it from there rather than trying to build
one. `engr backlog show` prints its item's the same way.

`show` puts the admitted wording and its trustworthiness on the same screen.
There is no second command to fetch the authoritative text — what you see is what
was admitted.

Objects are addressed by unique id prefix, like a git commit. A uuidv7 prefix is
a timestamp, so objects created close together need more characters; engr widens
the abbreviation for you.

## When an object or a section is marked

`show` answers two questions, and the first one is about the object as a whole.
Its `integrity` has four states, and three of them mean stop:

| `integrity` | What happened |
| --- | --- |
| `ok` | The seals verify and the projection is what its own admitted history produced |
| `tampered` | The stored bytes do not match their own seal |
| `divergent` | They match it, and **no admitted Event produced them** — something rewrote and resealed the record. `engr repair` restores what history proves |
| `unreplayable` | The admitted history cannot be replayed at all, so nothing can check the projection. This is damage to the EventStore, and `repair` is *not* the answer — there is nothing to restore from |

`divergent` is the one no seal can find, which is why it is worth knowing about:
a hash recomputed over edited bytes verifies perfectly, and only the record of
admissions shows that nobody admitted them. `engr show` and `engr verify` both
fail on all three.

Then, per section, `show` marks seven things and tells you what to do about each:

| Marking | What happened |
| --- | --- |
| `TAMPERED` / `tampered` | This Section or its Object aggregate does not match its stored integrity seal |
| `REF TAMPERED` / `ref_tampered` | A current or historical dependency fails integrity; the detail names the side |
| `REF UNREADABLE` / `ref_unreadable` | A section this one stands on will not load at all — malformed authority, not a missing one |
| `REF MISSING` / `ref_missing` | A section this one stands on is gone: the authority it rests on no longer exists |
| `REPLACEMENT UNAVAILABLE` / `replacement_unavailable` | This object says another replaced it, and that replacement cannot be established |
| `basis moved` / `stale_basis` | Real changes landed since the commit this wording was written against |
| `refs moved` / `stale_refs` | One or more selected dependency fields moved through an admission path |

The first five are a different kind of problem from the last two, and they are
not something to work around — as is any object `integrity` other than `ok`.
**Stop and tell the human.** Either someone edited
the stored file directly rather than going through the gate, or authority this
wording rests on — or the replacement it points forward to — has vanished. So
either nothing about that wording was agreed to by anyone, or what was agreed
to can no longer be checked. `show` hands you `git show <commit>:<path>` — run
it, and report what the record said before the edit. `engr show` and `engr
verify` exit non-zero here; `engr ls` still exits 0 so a survey of many objects
is not cut short.

For the last two: **do not quietly reason from a drifted section.** Take the
`git show` command `show` hands you, read what the dependency used to say, and
decide whether this section still holds. If it does not, prepare a revision —
and put *why* in the text, because that is what a reader three months out needs.

A drifted section is not wrong. It is unverified. A tampered one is neither.

Committing `.engr` does not make anything stale: the comparison ignores the
record's own files, so `basis moved` means real work landed.

## Choosing an action

| Situation | Action |
| --- | --- |
| Something new to record | `--add` |
| The same point, worded differently or corrected | `--revise <n>` |
| Several sections saying one thing | `--merge <destination> --sources <a>,<b>` |
| No longer belongs | `--delete <n>` |
| The object's title no longer describes it | `--rename --text "..."` |
| An untyped object has settled | `--close` |
| What kind of thing this is, or where it now stands | `--classify` |
| Another object has replaced this one | `--supersede <object>` |
| A **settled** object needs work again | that same action, plus `--type` and `--state` — [one confirmation](#type-state-and-attention), not two |

`--close`, `--reopen`, `--classify` and `--supersede` set the object's own
lifecycle, and only a person admits that: `--agent` is refused for each of them.

The last row is the one most easily missed. `--add`, `--revise`, `--merge`,
`--delete` and `--rename` all refuse an object nobody is looking at — but they
refuse it *bare*. Give the same command a destination that needs attention and
it does both in one confirmed operation. Reclassifying first and acting second
is still allowed and is a different statement; it is not the required route.

`--rename` replaces the title and nothing else. Do not reach for it to record
that the work changed shape: that belongs in a section, where it can say why.

Prefer `--revise` over delete-then-add. A revision keeps the section's id, so
every reference to it stays meaningful; delete-then-add breaks them, and the id
is never reused.

## Type, state and attention

An object may have a type. Most do not need one, and untyped is a real answer
rather than a gap to fill in:

```text
untyped     open | closed
design      draft | proposed | accepted | rejected | superseded
decision    proposed | accepted | rejected | superseded
risk        identified | accepted | mitigated | invalidated
```

`engr ls` shows what **needs attention**, which is derived from the pair: an
untyped `open`, a `draft` or `proposed` design, a `proposed` decision, an
`identified` risk. Everything else is out of the default listing — which does not
mean finished or correct, only that nobody is being asked to look at it. Use
`--all` to see the rest.

Classifying always states both halves, because the vocabularies do not overlap
and engr will not guess a mapping:

```bash
engr prepare --object <id> --classify --type decision --state accepted
```

**A new object arrives untyped and open, and `--new` takes no type.** Recording
one settled thing is therefore three acts, not one, and the third is easy to
walk away from:

```bash
engr prepare --new --title "..."                              # 1. untyped, open
engr prepare --object <id> --add --text-file draft.txt        # 2. the wording
engr prepare --object <id> --classify --type decision --state accepted   # 3.
```

Only the third gives it a type, and it is Human-only — no `--agent` path exists
for it, because what an object *is* is not an agent's to declare. So the
sequence ends at a challenge code even when the first two steps were agent
admissions. With nobody available to answer it, stop and report the code, and
say the object is still untyped and open: that is the honest end of the work,
and "recorded" without it overstates what is in the record.

Untyped is a real answer, not an unfinished one — say `--untyped` when you mean
it. What is not an answer is asking for a type, being refused at step 1, and
never coming back.

Use `--untyped` to say explicitly that an object has no type. There is no
transition order to follow: any state valid for the destination type is
reachable, and every hop is a separate confirmation.

**A no-attention object refuses section work** — unless the same command puts it
back in the listing. Add `--type <TYPE>` (or `--untyped`) and `--state <STATE>`
to the revision itself and both land in one confirmation:

```bash
engr prepare --object <id> --revise 1 --text "..." --type design --state proposed
```

Prefer that to reclassifying first and revising second. Two confirmations means
two authoritative statements, and the intermediate one is a state the object was
never really in — a reader three months out cannot tell that from a real one.

A destination that still needs no attention is refused, because that is the whole
point: renewed engineering work returns to the default listing rather than
happening where nobody sees it.

**And it only works on an object that is out of the listing.** If the object
already needs attention, adding `--type`/`--state` to a section action is
refused — there is nothing to unblock, so it would be an unrelated change riding
along inside a confirmation about something else. Classify it separately with
`--classify`, where a reader can see it as its own statement.

`--supersede` is the exception, because it is not renewed work — it is how an
object stops being current, and the object it exists for is an `accepted` one.
Supersede it where it stands.

## Roles, excerpts and relations

A section may carry a role, saying what it asserts: `decision`, `risk`,
`supersession`, `acceptance_criterion`. An `acceptance_criterion` states a
condition that must hold — never whether it currently passes. Verification
results are evidence and belong outside the record.

`--content <type> <body>` adds a bounded literal excerpt, `code.<tag>` or
`data.<tag>`, in the order you give them. Use it when the assertion needs the
literal to be precise. The section must still be understandable from its text
alone: if the text reads "use the following", the excerpt has swallowed the
assertion.

`--content-file <type> <path>` is the same entry with the body read from a file.
The two can be mixed freely; entries come out in the order you wrote them on the
command line, not grouped by which flag you used.

A body is stored **exactly** as given — including a trailing newline, which is
what a file almost always ends with. So `"x"` and `"x\n"` are different sections
with different hashes, and revising one into the other is a real revision. Decide
deliberately which one you mean rather than letting the shell decide for you. The
candidate screen names any ending it cannot show, so a human is never confirming
whitespace they could not see.

If engr refuses a section as too large, do not shorten prose until the number
goes down. Split an independent point into another section, move unresolved
reasoning into `engr backlog`, point at the implementation with
`--implemented-by-file` or `--implemented-by-symbol` instead of pasting it, and
keep only the smallest relevant excerpt of a log. `--oversize` exists for when it
genuinely is one bounded assertion, and the human sees that it was used.

`--oversize` is only ever a **retry**. Adding it to the first attempt is refused,
and so is adding it to something that breaks no limit — engr admits the exception
only for a proposal it has already refused, unchanged. So there is nothing to
gain by reaching for the flag early: prepare it normally, read the refusal, and
decide. If you genuinely have no better destination, run the same command again
with `--oversize`.

`--implemented-by-file <path>` and `--implemented-by-symbol <path> <symbol>`
record where an assertion is implemented, pinned to a real commit. Unlike
`--ref`, they carry no semantic dependency and never go stale.

Superseding is one command and one confirmation, and it needs a reason:

```bash
engr prepare --object <old> --supersede <new> --text "why the replacement"
```

It does not need the object brought back into the attention set first, and you
should not do that: the object this exists for is an `accepted` one, and moving
it back through `proposed` would confirm a state it was never in. Superseding is
not resumed work on the object — it is how the object stops being current.

That state and that relation cannot be separated afterwards, and there is no way
back out of `superseded` — a superseded object stays readable and addressable,
but if the knowledge is current again, say so in a new object.

Give `--based-on` when the wording is about code as it stood at a specific
commit. With clean source files it defaults to HEAD. If source outside `.engr/`
is dirty, engr refuses an omitted choice: select a committed basis, or use
`--no-based-on` only when the assertion genuinely has no repository basis.

Use `--ref <object>:<section> <fields>` when this wording depends on selected
semantics of another Section, including a sibling in the same Object. `fields`
is a comma-separated set such as `text,role`; select only what the source really
relies on. Commit the target first: the reference's commit must contain the same
selected values. A Section cannot directly reference itself.

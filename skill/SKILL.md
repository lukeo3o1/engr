---
name: engr
description: >-
  Use engr in a repository that has adopted `.engr/` to keep engineering records
  whose every word a human confirmed. Propose sections through the gate and wait
  for the human's challenge code, re-render a pending candidate instead of
  re-preparing it, read current wording and staleness with `engr show`, act on a
  section whose git basis or referenced section has moved, park work that is
  still unresolved in `engr backlog` rather than the record, and keep the
  shortest useful execution handoff for an Object in `engr work`, and group work
  into plans with `engr collection`. Not for application
  event-sourcing architecture, EventStoreDB or Kafka work, ordinary logs,
  personal journals, private session checkpoints, or writing decision documents
  outside an adopted project.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr

The runtime guide for working with a project's record. For changing the engr
repository itself, read `AGENTS.md` there instead.

This is all you need to use the record. When something comes up that it does not
answer — what a signal actually guarantees, what a stored field means, why a
command exited the way it did — the binary carries its own specification:

```bash
engr protocol
```

Read it **then, not first**. It is normative and it matches the build you are
running, but it is written for people implementing engr: most of its rules are
obligations on the tool rather than on you, and reading it up front costs a lot
to learn nothing you can act on. If it and this guide disagree, the protocol is
right about what the tool does — say so rather than working around it.

## The one rule

If engr reports a legacy workspace, reading remains safe but mutation is
blocked. Get explicit human direction before running `engr migrate`; never
silently create `.engr/format.json` or rewrite stored records. Never change an
unknown or newer workspace version.

Use canonical `engr:obj:<26-character-id>` references outside workspace
commands. Embedded targets use `kind: engr` with a namespace-relative `ref`;
shared syntax does not make different reference-bearing fields semantically
equivalent.

**You propose. A human admits.**

`engr prepare` puts a change up and prints a challenge code. That code exists so
a *human* can hand it back after reading the change. Nothing in the tool stops
you typing it yourself, which is exactly why this is on you:

> **Never run `engr confirm` with a code the human did not give you in this
> conversation.** Not to finish a task, not to unblock yourself, not because the
> change is obviously right.

If you confirm your own proposal, every guarantee the record makes becomes a
lie — and it is a lie no later reader can detect.

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

## Coming back later

A human often replies hours later, and your terminal output is gone.

```bash
engr candidate            # what is pending, and whether it is still live
engr candidate ABC123     # render it again, in full
```

**Do not re-run `engr prepare` to show it again.** That mints a new code and voids
the one they are holding.

## Project rules

Some projects write down rules engr cannot check for itself — what belongs in a
record, what belongs in backlog, what a plan may contain. They live in
`.engr/rules/*.md` and they are project policy, not engr's:

```bash
engr rules ls                    # what exists, and what it governs
engr rules ls --domain backlog   # what governs a backlog mutation
engr rules show <id>             # one rule in full, with what it rests on
```

**Read the ones that govern what you are about to do, and read what they rest
on.** A rule names project files in `based_on`; those files are part of the rule,
not background reading. `rules show` prints them so you know exactly which
material you were meant to have read.

A rule marked **UNUSABLE** cannot be reviewed against — its material is missing,
or a pinned basis no longer matches the project. Say so rather than proceeding
as though the rule were absent: an unusable rule is not a rule that does not
apply.

Every rule also says how many attempts you get and what happens when they run
out. `rules show` states it; `rules ls` mentions it only where it is not the
default:

```text
Review     5 attempts; on_exhaustion = reject
Review     3 attempts; on_exhaustion = human_confirmation
```

Both halves have defaults — five attempts, and `reject` — so a rule that says
nothing about review still has a limit. There is no unlimited rule.

The attempt count is **yours to report honestly**. engr does not track it, stores
no history of your tries, and can only tell you what a number means. It counts
one run of self-review: if you lose the thread or start over, a later independent
attempt at the same work legitimately begins at 1 again.

That is not a way around the ceiling. It is there because you are the only one
who knows how many times you have tried, and reporting a low number to get past a
rule is lying about the one input the rule depends on.

The line states the rule's policy, not what will happen to you. **A rule does not
have one consequence** — that is decided by the domain you are mutating, below.

What running out costs you depends on the domain, and the difference is
deliberate:

- On an **Object**, it stops *you*. Your autonomous path ends there, and if an
  exhausted rule asks for one, a human is brought in to decide. `reject` means
  engr will not escalate on your behalf — not that the mutation is forbidden. A
  human can still raise the same change and decide, having seen the review.
- In the **Backlog**, it does not stop you. Unresolved work is worth keeping, so
  the entry goes in marked `rule_review { attempts, limit }` — which is a
  standing note that this went in without a passing review, not a free pass.
  **Consuming** a Backlog point is the exception: that destroys unresolved work,
  so it needs a review that actually passed.

Do not treat the Backlog marker as somewhere to put work you could not get past
a rule. It is visible, it says what happened, and the point that produced it is
still unresolved.

engr does not author or edit rules, and there is no gate for them. Git is their
history.

## Reading the record

At the start of engineering work, run `engr ls --stale`. It lists **sections that
no longer verify cleanly** — a moved basis, a rewritten reference, wording that
was tampered with, or a dependency that will not load — and it marks the ones
belonging to objects nobody is looking at, which is where that goes unnoticed.
It is not a second spelling of `engr ls`: that one answers what needs attention,
this one answers what stopped adding up. Before making or revisiting a
significant architectural or behavioral decision, search existing titles and
section wording with `engr ls --all --sections` and an appropriate text search.
Re-evaluate any relevant moved basis or dependency before relying on it.

Also run `engr backlog ls`, `engr work ls` and `engr collection ls` — an
unresolved point recorded in
one and an execution checkpoint left in the other are exactly the context a
previous session left for you, and re-deciding or redoing something an earlier
session already handled is the failure they exist to prevent.

After a durable decision, constraint, assumption, or rationale is established,
consider capturing it when a future agent would need it. Do not record transient
task state, guesses, routine observations, or every thought merely because engr
is available. An open question is not a decision: stage it instead.

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

`show` puts the confirmed wording and its trustworthiness on the same screen.
There is no second command to fetch the authoritative text — what you see is what
was confirmed.

Objects are addressed by unique id prefix, like a git commit. A uuidv7 prefix is
a timestamp, so objects created close together need more characters; engr widens
the abbreviation for you.

## When a section is marked

`show` marks four things, and tells you what to do about each:

| Marking | What happened |
| --- | --- |
| `TAMPERED` / `tampered` | This section's wording does not match the hash confirmed with it |
| `REF TAMPERED` / `ref_tampered` | A section this one stands on does not match *its* hash |
| `REF UNREADABLE` / `ref_unreadable` | A section this one stands on will not load at all — malformed authority, not a missing one |
| `basis moved` / `stale_basis` | Real changes landed since the commit this wording was written against |
| `refs moved` / `stale_refs` | A section this one references was rewritten through the gate |

The first three are a different kind of problem from the last two, and they are
not something to work around. **Stop and tell the human.** Someone edited the
stored file directly rather than going through the gate, so nothing about that
wording was agreed to by anyone. `show` hands you `git show <commit>:<path>` —
run it, and report what the record said before the edit. `engr show` and
`engr verify` exit non-zero here; `engr ls` still exits 0 so a survey of many
objects is not cut short.

For the last two: **do not quietly reason from a drifted section.** Take the
`git show` command `show` hands you, read what the dependency used to say, and
decide whether this section still holds. If it does not, prepare a revision —
and put *why* in the text, because that is what a reader three months out needs.

A drifted section is not wrong. It is unverified. A tampered one is neither.

Committing `.engr` does not make anything stale: the comparison ignores the
record's own files, so `basis moved` means real work landed.

## Work that is not settled yet

Do not force an undecided question through the gate, and do not carry it in your
head across sessions. It goes in the backlog, which needs no confirmation:

```bash
engr backlog ls                          # what is still unresolved
engr backlog show <id>                   # points, subjects, outcomes so far
engr backlog new --topic "..." --text "the unresolved point"
engr backlog add <id> --text "another point in the same topic"
engr backlog revise <id> --section 2 --text "sharpened"
engr backlog rm <id> --section 2          # removing it is what says "settled"
```

Every screen says `UNCONFIRMED STAGING`, and it means it. **Never reason from a
backlog section as though it were the record**, and never quote one to a human
without saying where it came from. If a point has become something you can
assert, propose it through the gate like anything else — the wording a human
confirms is usually not the wording you staged.

Two rules keep it honest:

- **A section that is still there is still unresolved**, whatever it has already
  produced. `produced` lists confirmed outcomes that came out of working on it;
  it is progress, not a verdict. Read it when you resume so you do not re-solve
  what an earlier session already got confirmed.
- **Removing a section is the act of judging it resolved.** There is no status
  to set, so do not remove one you have not actually settled.

Give `--subject-file <path>` or `--subject-symbol <path> <name>` when the point
concerns specific source. engr pins the commit and refuses to pin HEAD while
that path is dirty — commit it first, or pass `--subject-commit <rev>`. Use
`--subject engr:obj:<id>:<section>` for a record section the point concerns;
that is context, not a dependency, and it does not make the record depend on
anything unconfirmed.

## Where execution stands

Backlog is for what is not decided. **Work** is for what is being done: the
shortest useful handoff to whoever picks this up next, hanging off one Object.

```bash
engr work ls                             # objects with execution memory
engr work show <object>                  # where this one stands
engr work start <object> --summary "..."
engr work summary <object> --text "..."  # replace the checkpoint
engr work item add <object> --text "one step"
engr work item state <object> --item 2 --state active
engr work item result <object> --item 2 --text "what it produced"
engr work item commit <object> --item 2 --commit HEAD
engr work item rm <object> --item 2      # prune it once it stops helping
engr work depend <object> --on engr:obj:<id> --reason "why it matters here"
engr work block <object> --reason "waiting for the customer"
engr work unblock <object> --index 0
engr work rm <object>                    # when there is nothing left to hand off
```

No confirmation, no challenge code — you write this directly, like backlog. What
makes that safe is that **finishing it settles nothing**. You can mark every item
done and the Object has not moved. If something you learned is stable knowledge,
propose it through the gate; if it is still an open question, put it in backlog.

Start by reading it, not by writing it. `engr work ls` is the first thing to run
when resuming: it says which Objects have execution memory, which are blocked and
which a human stopped.

**Write the shortest useful handoff, not the history of the work.** One action or
point per item, concrete verbs, outcomes rather than reasoning. The limits are
enforced — 300 characters for the summary, 160 for an item, 240 for a result, 200
for a reason — and there is no oversize exception, because nothing here is worth
admitting past its limit. If it will not fit, it belongs in backlog or the Object.

Keep the language already in the sidecar; if it has none, follow the repository's
working language. Do not translate existing entries because this conversation is
in another language.

### `paused` means a human said stop

```text
active    keep going
paused    a human suspended this; do not resume it on your own
```

**Never set `paused` yourself.** Not because your session is ending, not because
everything is blocked, not because you judge the work should wait — those are
what `blockers` and item states are for. And never clear it without being told
to, in this conversation, by the human.

**And never delete a paused sidecar** without being told to. `engr work rm` will
do it — it cannot tell you from a human, so it carries the instruction out and
then says a stop signal went with it. That line is not permission; it is the tool
telling you what you just discarded.

All of this is on you. engr enforces none of it, the same way nothing stops you
typing your own challenge code. It is the same kind of rule, and it fails the
same way: quietly, and only a human ever finds out.

`engr work rm` on work nobody paused is fine — a sidecar that no longer helps the
next agent is clutter, and git keeps what it said.

### Dependencies are not blockers

- `depend` — something this work relies on. It stays true even when nothing is
  currently stopping you.
- `block` — a condition preventing progress right now. It may be temporary, and
  it does not need a target: "waiting for the customer" is a real blocker.

The same Object can be both. Neither is an authoritative relation — if the
dependency turns out to be a stable engineering fact, that goes in the record
through the gate, and `implemented_by` is a different thing entirely.

Targets are whole Objects or backlog items, never sections.

Commits on an item are signposts, not proof. An item can be done with no commit,
and a rebase can strand one. Do not treat a missing commit as a problem.

## What is grouped together

Backlog is what is not decided. Work is what is being done on one Object.
**Collections** are the plan: which work belongs together, and in what order.

```bash
engr collection ls                       # plans, and how many members need attention
engr collection show <id>                # the plan, its schedule, its members in order
engr collection new --name "Q3 authentication" \
                    --description "..." --start 2026-07-01 --end 2026-09-30
engr collection add <id> --target engr:obj:<object> --order 10 \
                         --priority high --reason "Blocks the rest of this plan"
engr collection order <id> --target engr:obj:<object> --order 20
engr collection priority <id> --target engr:obj:<object> --priority low
engr collection rm <id> --target engr:obj:<object>
engr collection state <id> --state completed
engr collection delete <id>              # only on explicit human direction
```

No confirmation, no challenge code — you edit this directly, like backlog and
work. **Grouping something changes nothing about it.** An Object in a plan means
exactly what its confirmed sections say; moving it, ranking it, or calling the
plan complete is planning activity and nothing more.

Members are whole Objects or whole backlog items, given as
`engr:obj:<id>` or `engr:backlog:<id>` — never a section.

**Priority belongs to the membership, not the thing.** The same Object can be
`high` in this quarter's plan and `low` in the someday one. `--reason` says why
it matters *here*; engineering rationale belongs in the Object, through the gate.

`--order` is intended sequencing. Leaving it off means **unranked**, which is a
real answer — most plans are partly ordered. Two members cannot share a rank.
Never read the order of members in the file as the plan's order.

### Completing a plan proves nothing

```text
open        still being pursued
completed   you consider it finished
cancelled   no longer being pursued
```

You declare this. It is never inferred from dates or from what the members are
doing, and `completed` does **not** require every member to be resolved — work
gets deferred and moved out of scope, and a plan that could only close once
everything in it had would be a plan nobody could close honestly. Say which of
`completed` and `cancelled` you mean; they are different facts.

### Two things not to do

**Never delete a collection unless a human told you to, in this conversation.**
`engr collection delete` will do it and then tell you how much planning context
went with it. That report is not permission. Same kind of rule as `paused` on
work, and as the gate itself: engr enforces none of it.

**Never repoint a member whose target is gone.** A backlog item you added may
later be consumed; the plan will show it as gone. Resolution is not one-to-one —
the point may have become two Objects, or none — so retargeting it would change
what the plan says on a guess. Remove the member or add the real one, explicitly.

Dates are calendar dates, `YYYY-MM-DD`, and change nothing on their own. There is
no `overdue` state; a schedule is context for judging whether the plan still
makes sense.

## Choosing an action

| Situation | Action |
| --- | --- |
| Something new to record | `--add` |
| The same point, worded differently or corrected | `--revise <n>` |
| Two sections saying one thing | `--merge <a>,<b>` |
| No longer belongs | `--delete <n>` |
| The object's title no longer describes it | `--rename --text "..."` |
| An untyped object has settled | `--close` |
| What kind of thing this is, or where it now stands | `--classify` |
| Another object has replaced this one | `--supersede <object>` |
| A **settled** object needs work again | that same action, plus `--type` and `--state` — [one confirmation](#type-state-and-attention), not two |

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
`--ref`, they carry no wording dependency and never go stale.

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

Use `--ref <object>:<section>` when this wording depends on another section's
wording, including a sibling section in the same object. Commit the target
wording first: the reference's commit must actually contain its pinned hash.
That is what makes drift detectable later — without it, nothing notices when the
thing you relied on changes. A section cannot directly reference itself.

## Committing

Objects and confirmed history live in the repository. **Remind the human to
commit `.engr/objects`, `.engr/events`, `.engr/backlog`, `.engr/work` and
`.engr/collections`.**

All five, not the first three. Work and Collections are non-authoritative, but
git is the only history they have — an uncommitted plan or handoff is simply
lost, and losing it silently is worse than never writing it. `.engr/candidates`
is the one directory that must never be committed, and `.gitignore` already
excludes it.

This is a safety rule, not a convenience. The hash that proves a section was not
edited sits in the same file as the section — so it catches a careless edit and
not a careful one. Committed history is what actually anchors the wording:
`git show` is the only thing that can say what the record said before someone
changed it. Until an object is committed, `engr verify` can tell you the file is
inconsistent but nothing can tell you what it used to say. It is also where
earlier wording is recovered from for drift. `engr confirm` says when an object
has uncommitted changes.

`git add -A` is safe: `engr init` writes a `.engr/.gitignore` that keeps the lock
and any pending candidate out. Do not stage a candidate by hand to work around
it — its filename is the challenge code, and putting that in shared history hands
it to everyone with repository access.

## What not to do

- Do not look for a way to write without confirmation. There isn't one, and the
  absence is the point. The backlog is not one either — it is outside the record,
  and putting an assertion there does not make it recorded.
- Do not put a decision's reasoning in a commit message instead of a section. The
  record is where it belongs; the commit message is not addressable and cannot be
  referenced.
- Do not paste secrets into section text. It is committed and hashed, and there is
  no redaction.
- Do not batch several unrelated changes into one section so it passes in one
  confirmation. One point per section, or merging and referencing stop working.
- Do not treat a finished work sidecar as a settled Object. Every item done means
  the steps you wrote are done, and nothing else. The Object moves through the
  gate or it does not move.
- Do not set or clear `paused` on your own, and do not delete a paused sidecar.
  That signal is the human's, not yours.
- Do not treat membership in a plan as a fact about the member. A collection
  groups work; it says nothing about what any Object means or how settled it is.
- Do not delete a collection, or repoint a member whose target is gone, on your
  own judgement. Both discard planning somebody made.

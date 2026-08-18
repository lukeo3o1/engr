---
name: engr
description: >-
  Use engr in a repository that has adopted `.engr/` to keep engineering records
  whose every word a human confirmed. Propose sections through the gate and wait
  for the human's challenge code, re-render a pending candidate instead of
  re-preparing it, read current wording and staleness with `engr show`, act on a
  section whose git basis or referenced section has moved, and park work that is
  still unresolved in `engr backlog` rather than the record. Not for application
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

## Reading the record

At the start of engineering work, run `engr ls --stale`; it includes relevant
objects outside the default attention set. Before making or revisiting a
significant architectural or behavioral decision, search existing titles and
section wording with `engr ls --all --sections` and an appropriate text search.
Re-evaluate any relevant moved basis or dependency before relying on it.

Also run `engr backlog ls` — an unresolved point recorded there is exactly the
context a previous session left for you, and re-deciding something already being
worked on is the failure it exists to prevent.

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
| `basis moved` / `stale_basis` | Real changes landed since the commit this wording was written against |
| `refs moved` / `stale_refs` | A section this one references was rewritten through the gate |

The first two are a different kind of problem from the last two, and they are
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

`--rename` replaces the title and nothing else, and an object nobody is looking
at refuses it — bring it back into the attention set first. Do not reach for it
to record that the work changed shape: that belongs in a section, where it can
say why.

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

**A no-attention object refuses section work.** That is the old "reopen first"
rule in the wider vocabulary: classify it back into `draft`, `proposed` or
`identified`, then revise. Renewed engineering work returns to the default
listing rather than happening where nobody sees it.

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
commit `.engr/objects`, `.engr/events` and `.engr/backlog`.**

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

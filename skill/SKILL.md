---
name: engr
description: >-
  Use engr in a repository that has adopted `.engr/` to keep engineering records
  whose every word a human confirmed. Propose sections through the gate and wait
  for the human's challenge code, re-render a pending candidate instead of
  re-preparing it, read current wording and staleness with `engr show`, and act
  on a section whose git basis or referenced section has moved. Not for
  application event-sourcing architecture, EventStoreDB or Kafka work, ordinary
  logs, personal journals, private session checkpoints, or writing decision
  documents outside an adopted project.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr

The runtime guide for working with a project's record. For changing the engr
repository itself, read `AGENTS.md` there instead.

## The one rule

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

```bash
engr ls                              # open objects
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

## When a section has drifted

`show` marks two things, and tells you what to do about each:

| Marking | What happened |
| --- | --- |
| 地基已變動 / `stale_basis` | HEAD moved past the commit this wording was written against |
| 依據已變動 / `stale_refs` | A section this one references was rewritten |

**Do not quietly reason from a drifted section.** Take the `git show` command
`show` hands you, read what the dependency used to say, and decide whether this
section still holds. If it does not, prepare a revision — and put *why* in the
text, because that is what a reader three months out needs.

A drifted section is not wrong. It is unverified.

## Choosing an action

| Situation | Action |
| --- | --- |
| Something new to record | `--add` |
| The same point, worded differently or corrected | `--revise <n>` |
| Two sections saying one thing | `--merge <a>,<b>` |
| No longer belongs | `--delete <n>` |
| The work has settled | `--close` |

Prefer `--revise` over delete-then-add. A revision keeps the section's id, so
every reference to it stays meaningful; delete-then-add breaks them, and the id
is never reused.

Give `--based-on` when the wording is about code as it stood at a specific
commit; otherwise it defaults to HEAD, which is usually what you want.

Use `--ref <object>:<section>` when this wording depends on another record's
wording. That is what makes drift detectable later — without it, nothing notices
when the thing you relied on changes.

## Committing

Objects live in the repository. **Remind the human to commit `.engr/objects`.**
git is where earlier wording is recovered from; uncommitted, that recovery
silently is not there. `engr confirm` says when an object has uncommitted changes.

## What not to do

- Do not look for a way to write without confirmation. There isn't one, and the
  absence is the point.
- Do not put a decision's reasoning in a commit message instead of a section. The
  record is where it belongs; the commit message is not addressable and cannot be
  referenced.
- Do not paste secrets into section text. It is committed and hashed, and there is
  no redaction.
- Do not batch several unrelated changes into one section so it passes in one
  confirmation. One point per section, or merging and referencing stop working.

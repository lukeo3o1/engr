---
name: engr-delegate
description: >-
  Use when the work will be done by subagents rather than in this conversation:
  you discuss the work and define it as items in engr, hand each item to a
  worker subagent with a packet built from what engr holds, and record what it
  returns before handing out the next. What an item needs before it can be
  delegated, the packet, the report a worker owes, what to do when it returns,
  and how to pick the loop up after the context is compacted or cleared. Read
  the engr and engr-work skills first.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr-delegate

You are the coordinator. You talk the work through with the human, decide what
it is, write it down, hand it out one item at a time, and record what comes
back. Workers read and write the code. You do not — and when a fix is too small
to be worth a packet and you make it yourself, what a worker would have drafted
and reported is yours to write, now.

## Why this shape

In the watched runs where one agent both wrote the code and kept the record, the
record lost to the code every time. In the last of them none of 19 design
decisions reached the record in the session that made it; nine arrived a
session or two later, drafted from the code by an agent that got three of them
wrong; ten never did. The sidecar was right at neither cut, and not one of its
eight items said when it was done. The rule "record a decision when it is made"
was loaded and in front of the agent the whole time.

This shape moves each of those to a moment you cause instead of one you have to
notice:

- **An item is defined before a worker can do it.** A worker has none of this
  conversation, so an item it cannot tell is finished is an item it cannot
  finish.
- **A decision is drafted by the worker that made it, in the run that made it**,
  while it still knows the reason.
- **Recording is what you do when a worker returns.** That is a tool call you
  made, and a hook can see it.
- **Your context holds definitions and results, not code.** The code is read and
  written in the workers' contexts, so what is left in yours is what engr can
  give back after a cut.

None of this has been measured yet. It is the shape the failures above point to,
not a result.

## The loop

```text
discuss    open questions to backlog; what the human settles, to Sections
define     work items: a verb, a thing, and when it is done
delegate   one worker per item, with a packet built from engr
return     check it, record it, apply its decisions, commit
           then the next item
```

### Discuss: what is open and what is settled

Most of the work gets decided while you talk it through. Every question raised
and not settled in the same exchange is a backlog point (`engr-backlog`). Every
answer that settles something is a Section (`engr-object`) — through the Human
path when the human is here and just decided it, since that is what happened.

**Do not define an item on top of an open point.** A worker handed an item that
depends on an open question settles it by guessing, in code, where nobody
reviews the guess. Settle it first, or make settling it the item.

### Define: an item a worker can finish without asking

The shape is `engr-work`'s: one verb, one thing, and the condition that makes it
done. Delegating asks more of each part:

- **"Done when" is a check someone else can run.** A test name, a command and
  what it prints, a behaviour you can show. "Works", "is clean" and "handles
  errors" are not checks; the worker will decide what they mean.
- **What does not fit in 160 characters is not part of the item.** A constraint
  on how is a Section, a question is a backlog point, and the packet carries
  both. An item that needs a paragraph is an item with its decisions still in
  your head.
- **One worker's run.** If it will not finish in one, it is several items.
- **Finding something out is an item too:** "find where X is decided; done when
  the report names the function". What the worker learns comes back as drafted
  Sections, like a decision, rather than as one line of a result — one run kept
  a subagent's findings as a sentence, and the next session spent 37 turns
  finding them again.

Write every item you can see before delegating the first. The items are the plan
a coordinator resumes from, and the summary says which one is next.

### Delegate: the packet is engr's output

Build the worker's prompt from the template below and the output of engr
commands, pasted as they print. **Do not describe the work in your own words
beside them.** Anything you want to add is something you know and engr does not:
record it first — a Section if it is settled, a backlog point if it is open, the
item's text if it is the step — and then it is in the packet. A paraphrase next
to the record is a second statement of the same thing, and when they differ the
worker cannot tell which one counts.

That rule is also what keeps your own context rebuildable: what a worker needs to
start cold is exactly what you need after a cut.

Before the worker starts:

```bash
engr changeset new --object <id>
engr work item state <subject> --item 3 --state active
engr work summary <subject> --text "item 3 delegated; its decisions go in ChangeSet <changeset>"
```

The summary goes first because a worker's run is where a cut is most likely to
land, and "a review was in flight" and "a reader had already answered" are
exactly what one run did not write down — the next session sent both out again.

The packet:

```text
You are doing one step of a larger piece of work in this repository. You have
none of the conversation that defined it; everything you are given is below.
If something you need is not here, do not guess: stop, and say what is missing.

THE STEP, item 3 of `engr work show <subject>`:
<the item's line, as it prints>

WHAT IS SETTLED, from `engr show <object>`:
<as it prints>

WHAT IS OPEN, AND NOT YOURS TO SETTLE, from `engr backlog show <id>`:
<as it prints, or: nothing>

HOW A DECISION IS WORDED HERE, from `engr rules show <rule>`:
<as it prints>

While you work, settle nothing listed as open; if the step cannot be done
without settling it, stop and report. Run no engr command that writes, except
the one below.

When the step's check passes:

1. Commit your change. In the message body, put your OPEN and DEVIATION lines
   from the report below: your reply can be lost, the commit cannot.
2. For each choice you made that neither the step nor a settled Section
   dictated — anything a reader of the diff would ask "why this way?" about —
   and each fact you learned about the code that constrains later work, add
   one step to the ChangeSet: one assertion, with its reason, worded as the
   rule above asks.

   engr changeset add <changeset> --add --header "<the claim, short>" --text "<the claim, and why>"

3. Reply with exactly this, writing "none" rather than leaving a line out:

   DONE WHEN   the check, the command you ran, and what it printed
   COMMIT      the hash
   DECISIONS   the ChangeSet steps you added, one line each
   OPEN        questions you met and did not settle
   DEVIATION   where what you did differs from the step as written
```

Commit before drafting: a step added with the change committed rests on the
commit that implements it, and one added over a dirty tree needs a basis chosen
by hand.

"none" is required because an empty line and a forgotten one look the same.

**One worker at a time.** Two workers in one working tree edit the same files
and commit each other's changes.

### Return: record it before anything else

When a worker returns — before the next delegation, and before answering the
human — in this order:

1. **Run the check yourself.** The report says what the worker saw; marking the
   item done is yours. If the check fails, the item goes back to pending with a
   result saying what failed, and the next packet carries that.
2. **Read the diff against DECISIONS.** A choice you can see in the code that no
   step records is a decision nobody wrote down. Draft it yourself if the reason
   is in evidence — the code, a comment, the report. If it is not, it is a
   backlog point asking why. For a large diff, give it and the DECISIONS lines
   to a reader and ask which choices in it no step states.
3. **Record the item**, the way `engr-work` closes one:

   ```bash
   engr work item state <subject> --item 3 --state done
   engr work item result <subject> --item 3 --text "test_idle_tenant passes; 2 decisions drafted"
   engr work item commit <subject> --item 3 --commit HEAD
   ```

4. **Stage each OPEN line as a backlog point** in its own words. Do not settle
   it here because you happen to know the answer; if you do, that is a Section.
5. **Review and apply the ChangeSet** — a cold read, then a reviewer that never
   saw the drafts (`engr-object`). Fix a failed step yourself unless fixing it
   needs the code to change, which is an item.
6. **Rewrite the summary** to the next item, and commit `.engr`.

A DEVIATION is not a failure; it is the step changing under you. If the worker
had a reason, it is a decision and should be a step. If the item was wrong,
rewrite it, or add the item it implies.

A worker that stopped because something was missing is the definition failing,
not the worker. Record what was missing — a Section, a backlog point, or a
question for the human — rewrite the item, and delegate it again.

**Apply before the next delegation.** The next packet is built from `engr show`,
which shows only what was admitted, and a worker that cannot see the last
worker's decisions makes them again, differently. If the review has to wait,
put `engr changeset show <changeset>` in the next packet under "decided, not yet
admitted".

Reviewers and readers are subagents too, and return the same way. Nothing above
applies to them; carry on with the step that sent them out.

## When your context is gone

Your context will be compacted or cleared, and on long work that is certain
rather than likely. The loop is built so that **between items, nothing in your
context is missing from engr**: the plan is the items, the decisions are
Sections or drafts, the open questions are backlog points, and what is in flight
is the summary. That is the moment the conversation can be lost for nothing.
Keep it that moment: never start a delegation with something only you know, and
never leave a report unrecorded while you do something else.

After a cut, the start hook prints every sidecar, every ChangeSet, and what the
repository did since the newest sidecar. Find the item the summary says is in
flight, then:

| What you find | What happened | What to do |
| --- | --- | --- |
| The item active, a commit after the summary, drafts in its ChangeSet | The worker finished and its reply was lost | Read OPEN and DEVIATION from the commit message, then the return steps |
| The item active, uncommitted changes, no commit | The worker was cut off, or stopped | Delegate it again with `git status --short` and `git diff --stat` in the packet, to continue from the working tree |
| The item active, nothing changed | The worker never started, or stopped at once | Delegate it again |
| A ChangeSet, and no item active | A finished item's drafts, not yet applied | Review and apply it before delegating |

Then run the resume drill in `engr` if the summary and the repository disagree
about anything: the next coordinator is you with nothing but this.

## What not to do

- Do not paraphrase engr into the packet, or add the conversation, or a summary
  of it. What a worker needs from the conversation belongs in engr first.
- Do not let a worker apply, review, confirm or stage. It drafted the decisions,
  so it cannot be their reviewer, and admitting them is yours.
- Do not mark an item done on the worker's word. Run the check.
- Do not delegate an item whose check you could not state. It is a question, or
  several items.
- Do not run two workers in one working tree.

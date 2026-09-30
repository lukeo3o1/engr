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

Measured twice, on the same task. Both times most decisions reached engr in the
session that made them — against none of 19 when one agent did everything —
many drafted by the worker that made them, and a resuming session found its
place within a few turns of every cut. Both times the review of what was
recorded cost more turns than the work that produced it: about 70% of the
coordinator's turns in the first, about 85% in the second, which finished three
items short. What follows answers that by sending less through review and
spending one reader per round, which is not yet a result.

## The loop

```text
discuss    open questions to backlog; what the human settles, to Sections
define     work items: a verb, a thing, and when it is done
delegate   one worker per item, with a packet that points at engr
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

- **"Done when" is a check someone else can run, and it is in the item.** A
  test name, a command and what it prints, a behaviour you can show. "Works",
  "is clean" and "handles errors" are not checks; the worker will decide what
  they mean. One run wrote detailed checks into every worker's prompt and not
  one into an item, so no check outlived a cut: the coordinator after one
  closed items against whatever tests it found. Name the command and the Sections whose behaviour it must show, and
  it fits: `implement session.rs; done when cargo test session passes with a
  test for each of §4–§7`. What the behaviour is, is the Sections' job.
- **What does not fit in 160 characters is not part of the item.** A constraint
  on how is a Section, a question is a backlog point, and the worker reads
  both. An item that needs a paragraph is an item with its decisions still in
  your head.
- **Tests are part of each item's check, not an item of their own.** One run's
  "tests" item repeated the four before it and was closed when they were.
- **One worker's run.** If it will not finish in one, it is several items.
- **Finding something out is an item too:** "find where X is decided; done when
  the report names the function". What the worker learns comes back as drafted
  Sections, like a decision, rather than as one line of a result — one run kept
  a subagent's findings as a sentence, and the next session spent 37 turns
  finding them again.

Write every item you can see before delegating the first. The items are the plan
a coordinator resumes from, and the summary says which one is next.

### Delegate: the packet points at engr

The worker's prompt is the template below, and what it tells the worker to read
is engr. **Do not describe the work in your own words beside it.** Anything you
want to add is something you know and engr does not: record it first — a
Section if it is settled, a backlog point if it is open, the item's text if it
is the step — and then the worker reads it there. A paraphrase next to the
record is a second statement of the same thing, and when they differ the worker
cannot tell which one counts.

That rule is also what keeps your own context rebuildable: what a worker needs to
start cold is exactly what you need after a cut.

**Name the commands; do not paste their output.** The worker runs them itself.
One run pasted `engr show` into every packet: by its 70th turn it had sent
60–75k characters of prompts, and its context was larger than that of an agent
writing the code itself at the same turn.

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
none of the conversation that defined it. What you need is in engr: read it
first, from the repository root.

  engr work show <subject>
  engr show <object>
  engr backlog ls
  engr rules show <rule>

Your step is item 3 in the first, and its "done when" is your check. The second
is what is settled: build on it and contradict none of it. What the backlog
lists is open and not yours to settle — `engr backlog show` any that touch the
step. The last is how a decision is worded here. If something you need is in
none of them, do not guess: stop, and say what is missing.

Run no engr command that writes, except `engr changeset add` below.

When the step's check passes:

1. Commit your change. In the message body, put your OPEN and DEVIATION lines
   from the report below: your reply can be lost, the commit cannot.
2. Add a step to the ChangeSet for each decision that someone outside the code
   you touched relies on — a later step, another module, a caller of what you
   built: what it must keep doing and why, an attractive alternative you ruled
   out, a constraint a caller could break. A reason that matters only to
   whoever edits these lines is a comment beside them, as this repository's
   comments are: why, not what. A name, a visibility, which helper or type is
   neither. One assertion, with its reason, worded as the rule asks:

     engr changeset add <changeset> --add --header "<the claim, short>" --text "<the claim, and why>"

   If a decision changes what a settled Section says, revise that Section
   rather than adding one beside it that contradicts it:

     engr changeset add <changeset> --revise 4 --text "<the Section as it now holds, and why>"

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

The line is drawn by who reads it, because every step in the ChangeSet costs a
review and most reviews fail a step. "Anything a reader of the diff would ask
about" was the first wording, and workers drafted `Option<&str>`, `pub(crate)`
and exit-code choices as Sections; review kept failing them as restatements of
the code, and the two whose attempts ran out were both details of that kind. A
narrower wording — what the next person to change the code needs — still sent
local reasons through review, and the coordinator spent about 85% of its turns
on the record. A reason for these lines is read by whoever opens this file, and
a comment is where they look. A constraint a caller elsewhere could break is
the record's even when it is also a comment: one run left "this lock cannot be
taken twice" beside the lock, and the next session built the deadlock in
another file.

"none" is required because an empty line and a forgotten one look the same.

**One worker at a time.** Two workers in one working tree edit the same files
and commit each other's changes.

### Return: record it before anything else

When a worker returns — before the next delegation, and before answering the
human — in this order:

1. **Run the check yourself.** The report says what the worker saw; marking the
   item done is yours. If the check fails, the item goes back to pending with a
   result saying what failed, and the next packet carries that.
2. **Read the diff against DECISIONS and against the settled Sections.** A
   choice you can see in the code that no step records is a decision nobody
   wrote down. Draft it yourself if the reason is in evidence — the code, a
   comment, the report. If it is not, it is a backlog point asking why. A
   Section the code now contradicts is revised in this ChangeSet: one run gave
   interaction events a module of their own while an earlier Section still put
   them in another file, and nothing ever revised it. For a large diff, give it,
   the DECISIONS lines and `engr show` to a reader and ask what no step states
   and what a Section no longer says truly.
3. **Record the item**, the way `engr-work` closes one — with the hash from its
   COMMIT line, not `HEAD`. By the time you close an item, `HEAD` can be your
   own bookkeeping commit, and one run pointed two items at "delegate item 5".

   ```bash
   engr work item state <subject> --item 3 --state done
   engr work item result <subject> --item 3 --text "cargo test session passes, a test each for §4-§7; 2 decisions drafted"
   engr work item commit <subject> --item 3 --commit <hash>
   ```

4. **Stage each OPEN line as a backlog point** in its own words. Do not settle
   it here because you happen to know the answer; if you do, that is a Section.
5. **Review and apply the ChangeSet** — one cold read of the drafts, then a
   reviewer that never saw them (`engr-object`), each handed the `changeset
   show` screen as it prints: every step and the whole Object, never Sections
   you picked. One review was shown four Sections and passed a step that
   contradicted a fifth. Apply what passed with `--failed-step` for the rest,
   fix each failed step for what the review named, and send the fixes to the
   next reviewer — not back to a cold read, which would spend a subagent
   learning what the verdict already said. Fixing it yourself is right unless
   it needs the code to change, which is an item.

   **A failed step is fixed, or it goes somewhere; it is never just removed.**
   One run took four failed steps out to let the rest pass, and none reached
   engr again — among them that `ui.update` patches a copy and persists only
   what validates. If the review showed it is not a decision at all, say so in
   the item's result; if it is one whose wording will not pass, it goes to the
   backlog whole, like an exhausted step.
6. **Rewrite the summary** to the next item, and commit `.engr`.

**A step whose attempts run out goes to the backlog first**, whole: its wording,
its reason, how many attempts it used, and what the reviews found — then, when
it is time, to `prepare --agent` at its next attempt, where the Rule decides.
There is one live Human candidate per Object, and any admission to the Object
kills it, so in a loop that keeps admitting to one Object a candidate is the
least durable thing engr holds. One run lost both of its exhausted decisions
that way: the second candidate voided the first, the next ChangeSet killed the
second, and the first then lived only as a clause in a summary that was
rewritten without it. Prepare the candidate once no further admission to that
Object is coming — at the end of the work, or when a person is there to answer
— one at a time, and consume the backlog point only when it is admitted.

A DEVIATION is not a failure; it is the step changing under you. If the worker
had a reason, it is a decision and should be a step. If the item was wrong,
rewrite it, or add the item it implies.

A worker that stopped because something was missing is the definition failing,
not the worker. Record what was missing — a Section, a backlog point, or a
question for the human — rewrite the item, and delegate it again.

**Apply before the next delegation.** The next worker reads `engr show`, which
shows only what was admitted, and a worker that cannot see the last worker's
decisions makes them again, differently. If the review has to wait, add `engr
changeset show <changeset>` to the next packet's list, as decided but not yet
admitted.

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

- Do not paste engr into the packet or paraphrase it, and do not add the
  conversation or a summary of it. What a worker needs from the conversation
  belongs in engr first.
- Do not let a worker apply, review, confirm or stage. It drafted the decisions,
  so it cannot be their reviewer, and admitting them is yours.
- Do not mark an item done on the worker's word. Run the check.
- Do not delegate an item whose check you could not state. It is a question, or
  several items.
- Do not run two workers in one working tree.

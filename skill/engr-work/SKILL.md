---
name: engr-work
description: >-
  Use before any `engr work` command in a repository that has adopted `.engr/`,
  and the moment a step finishes or a commit lands. The execution sidecar that
  stands in for a built-in task list: one checkable item per step, one active at
  a time, closed with its evidence; a summary that says where things stand;
  blockers and dependencies; and what of your reasoning to keep. Read the engr
  skill first.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr-work

Where execution stands. When to write it is in the `engr` skill; this one is
how.

## Where execution stands

Backlog is for what is not decided. **Work** is for what is being done: the
shortest useful handoff to whoever picks this up next, hanging off one Object or
one Backlog item — whichever the execution is actually about.

```bash
engr work ls                               # subjects with execution memory
engr work show <subject>                   # where this one stands
engr work start <subject> --summary "..."
engr work summary <subject> --text "..."   # replace the checkpoint
engr work item add <subject> --text "one step"
engr work item state <subject> --item 2 --state active
engr work item result <subject> --item 2 --text "what it produced"
engr work item commit <subject> --item 2 --commit HEAD
engr work item rm <subject> --item 2       # prune it once it stops helping
engr work depend <subject> --on engr:obj:<id> --reason "why it matters here"
engr work block <subject> --reason "waiting for the customer"
engr work unblock <subject> --index 0
engr work rm <subject>                     # when there is nothing left to hand off
```

`<subject>` is an Object id, or the canonical reference of either kind:
`engr:obj:<id>` for durable knowledge, `engr:backlog:<id>` for an unresolved
point you are working through. A bare id means an Object — both namespaces use
the same kind of identity, so a Backlog subject has to be written out.

One thing follows from the second kind. A Backlog item is removed when its last
point is consumed, and a sidecar cannot outlive what it belongs to, so **that
consume is refused while execution memory exists**. Run `engr work rm
engr:backlog:<id>` first, deliberately, once you have moved anything worth
keeping into the record. Ordinary consumes and merges are unaffected: work never
decides whether a point can be resolved.

No confirmation, no challenge code — you write this directly, like backlog. What
makes that safe is that **finishing it settles nothing**. You can mark every item
done and the Object has not moved. If something you learned is stable knowledge,
admit it through the appropriate Human or Agent path; if it is still an open
question, put it in backlog.

A project rule may still govern `work`. When one does, say which attempt of
your own review this is: `engr work --attempt <n> <subcommand> ...`, counted
from 1, and 1 if you say nothing. Past every applicable ceiling the mutation is
refused — v1 has not settled what an exhausted rule means here, and engr will
not guess on your behalf.

Start by reading it, not by writing it. `engr work ls` is the first thing to run
when resuming: it says which Objects and Backlog items have execution memory,
which are blocked and which a human stopped.

**If your harness has a task list of its own, keep this work out of it.** A
built-in todo list is where the urge to track steps naturally goes, and it
disappears with the context. Of four watched sessions that kept their steps in
the harness's list, three wrote nothing to engr and the fourth stopped after its
first 27 turns, and none answered any of the seven reminders they were given.
The sidecar is that list: use it the way you would the other, one item per step,
and the next session has it too.

**Write the shortest useful handoff, not the history of the work.** One action or
point per item, concrete verbs, outcomes rather than reasoning. The limits are
enforced — 300 characters for the summary, 160 for an item, 240 for a result, 200
for a reason — and there is no oversize exception, because nothing here is worth
admitting past its limit. If it will not fit, it belongs in backlog or the Object.

**A refused write is not a shorter write.** In one watched run, every time an
item or a result was refused for length, the agent cut the reason and kept the
what —
and the reason was the part the next session needed. Put it where it fits, a
Section or a backlog point, and keep the line short by pointing at it. Nor chain
engr writes with `&&` without reading each answer: one refused for length
silently stopped every update after it, and nothing showed it at the cut.

The summary says where things stand and which item is next. It is not a
changelog: what is done is in the items, with their results. **Rewrite it
whenever an item changes state.** In every watched run it was written once and
left; one still read "deciding domain scope before writing code" at the third
cut, long after the scope was built, and sent the next agent back to work that
was done.

### An item is a step you can tell is finished

Write each item as one verb, one thing, and the condition that makes it done:

```text
✗ fairness in the dispatcher
✓ stop idle tenants counting toward share; done when test_idle_tenant passes
```

If you cannot write the condition, it is not a step yet: it is a question, and
belongs in backlog, or it is several steps. The right size is what one sitting
or one commit finishes.

**One item active at a time.** Making the next one active is the moment to close
the one before it — done, or back to pending with a result that says why.
Attention moves to the next thing the instant the last one works, and that
instant is exactly when the tick gets forgotten.

**Close it in the same step as the proof.** When the test passes or the commit
lands, run these before anything else:

```bash
engr work item state <subject> --item 2 --state done
engr work item result <subject> --item 2 --text "test_idle_tenant passes"
engr work item commit <subject> --item 2 --commit HEAD
```

A worker a coordinator handed the item to does not run these: it reports, and
the coordinator closes the item (`engr-delegate`). One worker told to read this
skill closed its own item from it, against the prompt it was given.

The result says how you know it is done, not that it is. Take every specific in
it — a hash, a count, a test name, a code — from command output rather than
from memory: a plausible hash reads exactly like a real one. `--commit HEAD` is
the tool resolving it for you; prefer that shape wherever one exists.

**Done means the item's words are true — all of them.** An item promising
create, add, revise and remove was ticked done with revise never built, and
nothing said so; the next reader believed the tick. If the scope shrank, say so
in the result, and record the cut: a decision if you chose it, a backlog point if
it is still open.

### What of your thinking to keep

The reasoning that got you here is usually a mess — abandoned guesses beside
conclusions, in the order you had them. **Do not store it.** A later agent reads
every sentence as equally meant; it cannot tell the guess you dropped from the
result, and it will pick the dropped one back up. The mess is in git and in the
transcript if anybody ever needs it.

What it contains that the next agent needs goes where it belongs:

| In your reasoning | Where it goes |
| --- | --- |
| A conclusion that is settled | A Section: the claim and its reason |
| A question you will not settle now | A backlog point |
| A hypothesis you are testing | An active item: "check whether X causes Y" |
| What that showed, or a path you ruled out | That item's result: "X fails: Y" |
| What comes next | The next pending item, and the summary |
| How you got there | Nowhere |

So thinking in progress is not a separate thing to save. A hypothesis is an
item, and its result is the conclusion; the 240-character limit on a result has
room for what was found and none for how.

**A decision is recorded when it is made, not added as an item to record it
later.** In five watched runs "record the settled decisions as Sections" went in
as a pending item — three of them with this rule already in the guide — and each
time the decisions reached the record late or not at all, living meanwhile in a
summary line, a code comment or the code alone. Recording one now costs no
review: add it as a step of a ChangeSet on its Object the moment it is made
(`engr changeset add`, in `engr-object`), while you know the reason, and pay for
one review when you apply several.

**A fact you learn about the code is a constraint, and it outlives the step.**
One run learned that the writer lock cannot be taken twice by one process, left
that in a code comment, built the deadlock anyway, and cost the next session
twenty turns finding it again; another run hung on what looked like the same
lock and recorded nothing. The one run that admitted it as a Section never hit
it.

To decide whether a line stays, do not ask yourself whether the next agent would
miss it. That is predicting a reader without your context, which is the one
thing the writer cannot do. Do these instead:

1. **Classify it** by the table. "How you got there" goes, and that alone
   removes most of it.
2. **A ruled-out path stays if you actually tried it.** It attracted you, and
   the next agent is attracted by the same things. One you dismissed at a
   glance can go.
3. **For the few lines you still cannot call, test them.** Ask a reader with no
   context what it would do next and what it would try first — once with the
   line, and once, a different reader, without it. If the answer does not
   change, the line goes.
4. **When still unsure, weigh the costs.** A missing ruled-out path costs the
   next agent the whole detour; an extra line of history costs attention and
   goes stale. Keep paths and constraints, drop narrative, always keep pointers.

**A sidecar is only as true as the last time it was checked against what it
names.** A blocker that names a candidate, an item marked active, a dependency
on a point — each is a claim about something else, and that thing moves without
touching the sidecar. One session's blocker said a candidate was still waiting
on a human; the human had answered it a session earlier, and the next agent
found that out only because it checked. So update the sidecar in the same step
as the act that changes it, and when resuming, check each claim against its
thing before relying on it — a pending code with `engr candidate`, a point with
`engr backlog show`.

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

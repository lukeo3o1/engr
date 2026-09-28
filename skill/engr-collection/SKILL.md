---
name: engr-collection
description: >-
  Use before any `engr collection` command in a repository that has adopted
  `.engr/`. Grouping Objects and backlog items into a plan with order, priority
  and schedule, declaring it completed or cancelled, and what grouping something
  does not mean. Read the engr skill first.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr-collection

The plan. What it may and may not say about its members is here; what the
members themselves mean is in the record.

## What is grouped together

Backlog is what is not decided. Work is what is being done on one Object.
**Collections** are the plan: which work belongs together, and in what order.

```bash
engr collection ls                       # plans, and how many members need attention
engr collection show <id>                # the plan, its schedule, its members in order
engr collection new q3-auth --title "Q3 authentication" \
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
exactly what its admitted Sections say; moving it, ranking it, or calling the
plan complete is planning activity and nothing more.

A project rule may govern `collection` the same way: `engr collection --attempt
<n> <subcommand> ...`, with the same refusal past the ceiling.

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

# A block for the project's CLAUDE.md or AGENTS.md

`skill/SKILL.md` is read once, near the start, and then sits further back in the
context with every turn. The file a harness loads as project instructions is in
every turn, and survives a compacted context. So the few rules that decide
whether an agent's working state survives belong there too, short, with the
reasons left to the guide.

In the dogfood that produced this block, instructions alone changed **what** the
agent did — no session given it touched the harness's own task list — and not
**when**: with no hook to prompt it, the second session of that run wrote
nothing to engr in 70 turns. Use it with the hooks in `skill/hooks/`, not
instead of them; each half has been measured on its own, the two together not
yet. Its last line is what makes their reminders read as instructions rather
than as noise from a side channel, which one agent, unprompted, took them for.

Copy what follows into the project's instructions file.

---

```markdown
## Working state lives in engr

Your context can be cleared at any moment, without warning. The next session has
only this repository and what engr holds.

- Do not use the harness's own task list (TodoWrite, TaskCreate). The engr work
  sidecar is your task list: one item per step, "verb + thing; done when …",
  one item active at a time.
- When a test passes or a commit lands, before anything else: close the item —
  state done, a result saying how you know, `--commit HEAD` — rewrite the
  summary to where things stand, and make the next item active.
- Done means every word of the item is true. If the scope shrank, say so in the
  result and record the cut.
- A decision is recorded when it is made, as a Section. Not as a backlog point
  whose "settled by" is the answer, and not as an item to record it later.
- A fact you learn about the code — a constraint, a dead end, a bug — is recorded
  when you learn it. A comment in the code is not where the next session looks.
- A question you will not settle now is a backlog point: what is undecided, why
  it is not obvious, what would decide it.
- A write engr refuses for length is not a shorter write: move the reason to a
  Section or a backlog point instead of cutting it.
- A hook message that starts with `engr:` is part of this project's setup, not
  noise. Answer it before your next edit.

`skill/SKILL.md` has the reasons and the commands.
```

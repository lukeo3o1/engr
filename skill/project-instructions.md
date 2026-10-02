# A block for the project's CLAUDE.md or AGENTS.md

`skill/engr/SKILL.md` is read once, near the start, and then sits further back in the
context with every turn. The file a harness loads as project instructions is in
every turn, and survives a compacted context. So the few rules that decide what
the next session can know belong there too, short, with the reasons left to the
skills and the reports.

An earlier version of this block made a work sidecar the agent's task list and
asked it to close an item at every step. Every audit found that copy behind the
repository at the cuts, and every probe rebuilt progress from Git, the code and
the tests regardless. In round v5
(`docs/dogfood/2026-10-02-minimal-durable-state.md`) one arm kept only intent in
engr and rebuilt progress from the repository: its probes were as correct as the
control's, it wrote progress no times against 41, and the copy's stale claims
went with the copy. That was one run per arm, and the bar set beforehand was not
met in full: a successor redid a worker's step that `git branch` or
`git worktree list` would have shown it.

The first four rules below carry what that arm was given, in its task prompt
rather than as project instructions. Added since, and not yet run: the human
decision behind the goal; the last three commands of the rebuild; and the rules
on where a problem and a decision go, on a decision whose review ran out, on
applying a passing review, on a worker's worktree and on collections — each from
something an arm lost, or from working out afterwards where things go. The rest
were in the earlier version.

The engr skills still keep progress in a work sidecar, and the v5 probes, which
loaded them, recommended one; so the block says it wins where they disagree.

The arm ran with the four hooks in `skill/hooks/`. With no sidecar, `stop-check`
refused the first stop far more often — 15 times, against 6 — and each refusal
cost a turn and produced nothing; the block's last line says what to answer it
with. That line is also what makes the hooks' reminders read as instructions
rather than as noise from a side channel, which one agent, unprompted, took them
for.

Copy what follows into the project's instructions file.

---

```markdown
## engr holds intent; the repository holds progress

Your context can be cleared at any moment, without warning. The next session has
only this repository and what engr holds. Where an engr skill says otherwise,
this section wins.

- Record what the repository cannot give back: the goal and the human decision
  behind it, acceptance criteria each with a check another agent can run,
  external constraints, invariants, decisions and why they were made, approaches
  ruled out, anything a human specified. Before writing a Section, ask: without
  it, would a new agent with the goal, the repository, Git and the tests reach
  the same answer? If so, leave it out.
- Do not record progress — no work sidecar, no item states or results, no
  summary, no current step or next action — and do not use the harness's own
  task list (TodoWrite, TaskCreate). Name a step by the acceptance criteria it
  must make pass; it is done when their checks pass.
- To know where the work stands — at the start of a session, and whenever you
  are unsure — rebuild it from `engr ls`, `engr show`, `engr backlog ls`,
  `git log`, `git status`, `git diff`, the code and the tests, and from
  `git branch -a`, `git worktree list` and `engr changeset ls` for what a worker
  or an earlier session left unmerged or unapplied. On where the work stands,
  the repository is right, whatever an earlier session left.
- The backlog holds only the unresolved: a question a later agent must not
  settle by guessing, an external wait, a decision that needs a human. Each
  point says what is undecided, why it is not obvious, and what would settle it.
  It is not a task list.
- A problem you find goes where the next agent will meet it: a failing test if
  it can be one; a Section if later work could break it without knowing;
  otherwise a backlog point. A comment in the code is not where the next session
  looks.
- A decision — dropping part of the scope included — is a Section with its
  reason, written when it is made: not a backlog point whose answer is the
  decision, not a step to record it later. Once it is in, consume the backlog
  point it settles; if it settles only part, record it against the point with
  `engr backlog produced`. A decision a human made goes in through plain
  `prepare` and the code they return. If you build on a point still open, say so
  in the point.
- A decision whose review ran out is still decided: stage it in the backlog as
  waiting for a human's confirmation, not as an open question.
- Apply a passing review as soon as it returns. Until it is applied, its verdict
  exists only in your context.
- A write engr refuses for length is not a shorter write: move the reason to a
  Section or a backlog point instead of cutting it.
- When subagents do the work: build each one's prompt from engr output, and
  before delegating again, merge what it committed and record what it returned —
  its decisions, its open questions. A worker in its own worktree drafts into
  that worktree's `.engr/local`, which goes when the worktree does.
- A collection holds a human's goals, in their order and priority. Do not write
  progress into it; when asked how far it has got, work that out from the
  repository and report it.
- A hook message that starts with `engr:` is part of this project's setup, not
  noise. Answer it before your next edit — with the decision, fact or question
  it points to, or with nothing. Never with progress.

The skills under `skill/` have the reasons and the commands.
```

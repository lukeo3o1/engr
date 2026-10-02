# A block for the project's CLAUDE.md or AGENTS.md

Copy the block below into the file your harness loads every turn. It is short on
purpose; the reasons are in the skills and in
`docs/dogfood/2026-10-02-minimal-durable-state.md`. Rules 1–4 are what that
round's minimal arm ran with, once; 5–10 were added from what it and the control
lost, and have not been run. The engr skills still keep progress in a work
sidecar, so the block says it overrides them. Rule 10 assumes the hooks in
`skill/hooks/`.

---

```markdown
## engr holds intent; the repository holds progress

Your context can end at any moment; the next session has only this repository
and engr. Where an engr skill disagrees, this section wins.

1. Record only what the repository cannot give back: the goal and who decided
   it, acceptance criteria with a check anyone can run, constraints, decisions
   and why, approaches ruled out. If a new agent with the repository would reach
   the same answer without it, leave it out.
2. Never record progress: no work sidecar, no summary, no next step, no harness
   task list.
3. To resume, rebuild from `engr ls`, `engr show`, `engr backlog ls`,
   `engr changeset ls`, `git log`, `git status`, `git diff`, `git branch -a`,
   `git worktree list`, the code and the tests. On progress, the repository wins.
4. The backlog holds only the unresolved — what is undecided, why, what would
   settle it. It is not a task list.
5. A problem found becomes a failing test if it can; else a Section if later
   work could break it; else a backlog point.
6. A decision becomes a Section with its reason when it is made, and consumes
   the backlog point it settles. A human's goes through plain `prepare`.
7. Apply a passing review at once. A decision whose review ran out goes to the
   backlog as decided, awaiting a human.
8. Before delegating again, merge the worker's branch and record its decisions
   and open questions.
9. A collection holds a human's goals: report progress against it, never write
   progress into it.
10. Answer an `engr:` hook with intent or nothing, never with progress.
```

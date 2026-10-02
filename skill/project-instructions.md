# A block for the project's CLAUDE.md or AGENTS.md

Copy the block below into the file your harness loads every turn. It is short on
purpose; the reasons are in the skills and in
`docs/dogfood/2026-10-02-minimal-durable-state.md`.

It is an experiment, not yet measured as a whole. Round v5 measured one part of
it, once: progress rebuilt from the repository rather than kept in engr. The
rest is new: working knowledge kept in the backlog without review, and an
Object admitted, and reviewed, only when its knowledge outlives the work that
produced it — not when a decision is made, which is what v5 did and where most
of its cost went.

Two meanings differ from the rest of engr, and the block overrides them:

- **The backlog holds settled knowledge too.** engr describes the backlog as
  unresolved staging. Here a settled entry is something the current work must
  follow that has not yet earned a place in an Object.
- **Consuming does not always mean resolved.** It can also mean that a settled
  entry's work is over and the entry was not worth promoting.

The engr skills still keep progress in a work sidecar and record a decision as
a Section when it is made; the block's opening line says it wins. Rule 10
assumes the hooks in `skill/hooks/`.

---

```markdown
## engr holds knowledge; the repository holds progress

Your context can end at any moment; the next session has only this repository
and engr. Where an engr skill disagrees, this section wins.

1. Keep out of engr what a new agent can reliably recover from the repository
   (code, tests, Git) — progress above all: no work sidecar, summary, current or
   next step, and no harness task list. Whether something is built is read from
   the repository and its checks.
2. As soon as you recognize knowledge a future session will need and the
   repository cannot recover — a problem, question, proposal, external wait,
   decision, requirement, review concern, or the conclusion of a costly
   investigation — put it in the backlog. Use one topic for one bounded piece
   of work.
3. Each entry is open or settled. Open: something must still be decided, learned
   or awaited; say what would settle it, and never settle it by guessing. Once
   it is resolved, rewrite it as settled with the answer, or consume it if the
   repository now makes the needed answer reliably recoverable. Settled: current
   work treats it as a working constraint, but it is not durable knowledge yet.
   Record its authority or source — who set it when applicable, otherwise the
   evidence or ref — and only the reason or checks needed to use it. Change a
   settled entry only with a reason, and a human's only with human authority.
4. An Object holds durable engineering knowledge; its Sections state the durable
   principles, invariants, constraints, decisions, risks, and costly or unsafe
   ruled-out approaches. Keep Objects sparse. Promote backlog knowledge when a
   human says it is durable or other work comes to depend on it; otherwise only
   when its work ends.
5. When the work a topic serves ends, go through its entries. Promote each one
   without which a capable future agent could reasonably choose differently
   from the repository alone and violate the intended behavior or constraint;
   consume the rest. An entry still open stays.
6. Settled backlog entries need no admission review; review happens on
   promotion, except where an existing Rule explicitly requires earlier review.
   Apply a passing review at once. A decision whose required review runs out
   waits in the backlog for a human, with its claim, the reason needed to judge
   it, and the unresolved review concern.
7. One idea per entry, conclusion first, plain words. Keep only what is needed
   to use it correctly; drop background and deliberation, or point to them with
   a ref the next session can resolve.
8. To resume, rebuild from engr (`ls`, `show`, `backlog ls`, `changeset ls`,
   `candidate`), Git (`log`, `status`, `diff`, `branch -a`, `worktree list`),
   the code and the tests. On progress, the repository wins.
9. A collection is a human's grouping, order and priority over goals; change it
   only when asked. When asked, report progress against it, worked out from the
   repository.
10. Answer an `engr:` hook with knowledge that must survive a context loss, or
    nothing — never with progress.
```

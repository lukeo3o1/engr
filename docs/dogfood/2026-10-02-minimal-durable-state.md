# Intent in engr, progress from the repository

Every run so far kept where the work stood in engr — a work sidecar of items,
results and a summary — and every audit found that copy behind the repository
at the cuts, while every probe rebuilt implementation progress from Git, the
code and the tests anyway. This round asked whether engr needs the copy at all:
hold only what the repository cannot give back — the goal, acceptance criteria,
constraints, decisions and their reasons, ruled-out approaches, open questions —
and rebuild working state from the repository whenever it is needed.

Nothing in engr changed. All features stayed — Objects, Sections, backlog,
work, collections, ChangeSets, Rules and review, Human candidates, the four
hooks, `engr-delegate` — and the two arms differ only in the paragraph of the
prompt that says how to use engr.

## The setup

Both arms ran the #40 slice on a sandbox at `ac1b055`, with the
`engr-delegate` coordinator loop, turn caps of 70, 70 and 90 held across
background-subagent restarts by the runner's watchdog, the same ten-question
probe after each cut, and the same blind audit brief. Claude Code 2.1.286.

- **C5 (control)**: arm M3's prompts — progress in a work sidecar, the plan in a
  collection.
- **D5 (minimal durable state)**: the Object says what the work is for; Sections
  only for intent a new agent could not rebuild, tested by "without it, would a
  new agent with the goal, the repository, Git and the tests reach the same
  answer?"; backlog only for the unresolved; no work sidecar or collection;
  working state rebuilt from engr, backlog, `git log`, `git status`, `git diff`,
  the code and the tests, the repository winning any disagreement; a hook asking
  for progress is not answered with progress.

The probe asked: the goal; the constraints; what is undecided; how far the
repository has got; which acceptance criteria are met and how that was checked;
which are not; the next action; anything that could not be recovered; whether
that was progress or something that should have been recorded; and whether
engr disagrees with the repository.

The bar was written before either arm started (`v5-preregistration.md` in the
harness). D5 supports the hypothesis if all four hold:

- **H1**: at the two real cuts, the probe's answers on state and criteria are
  free of wrong or invented claims at least as often as C5's; and no successor
  re-decides something settled or guesses something unresolved.
- **H2**: no lost intent misled anyone into breaking the intent.
- **H3**: progress is written at most five times; engr writes plus review
  subagents are fewer than C5's.
- **H4**: stale state misled no more often than in C5.

A third arm with a disposable "current focus" line was to run only if D5's
successors took more than ten turns longer than C5's to get going.

## What the harness got wrong, and how it was repaired

- **Claude Code's print mode waits 600 seconds for background subagents** after
  its main loop ends, then terminates the session. C5's second and third
  sessions ended that way, with a worker still running, at 65 and 14 turns. The
  runner now sets `CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0`, so only the
  watchdog cuts. Everything after C5's S1 was run again from S1's snapshot
  (`C5/bg-ceiling/`). M2's third session, which "ended on its own", probably
  ended the same way.
- **The account's session limit** cut C5's third session at turn 51, and D5's
  run after its second cut. Each was run again from the last unspoiled snapshot
  (`*/limit-hit/`). What stands: C5's S1 and first probe as first run, its S2,
  S3 and later probes from the reruns; D5's S1, S2 and first probe as first run,
  its second probe, S3 and third probe from the rerun.
- **A worker run in an isolated worktree** left `.claude/worktrees/` with a
  build of its own, and the snapshot carried 1.8GB of it. Snapshots now leave
  out that build, not the worktree's code or branch.

## Against the bar

| | C5 | D5 |
| --- | --- | --- |
| Probe answers on state and criteria with nothing wrong or invented, at the two real cuts | 2 of 2 | 2 of 2 (one partly correct: missed an unmerged worktree branch) |
| A successor re-decided something settled | S2 re-reviewed and failed 4 steps S1's review had passed | S2 re-made two decisions S1's worker had made, the same way |
| Lost intent that misled anyone into breaking intent | none | none |
| Progress writes (work, collection) | 41 | **0** |
| Coordinator engr writes + review and cold-read subagents | 153 + 13 = 166 | 115 + 18 = 133 |
| Stale state that misled anyone | the backlog calling 7 implemented designs "unresolved", partly; a review status, mildly | a ChangeSet still showing "NEEDS REVIEW" after its review came back |
| Audit: resume / intent completeness / freshness / proportion | 4 / 3 / 3 / 1 | 3 / 4 / 4 / 1 |
| Cost | about US$21.6 | about US$19.7 |

H2, H3 and H4 hold. H1 holds on the probes and fails, strictly, on its second
clause: D5's second session re-made two decisions its first session's worker had
made — the same way, but redone. **So the bar was not met in full.** The E5
condition was not met either (D5's successors averaged 7.5 turns longer to their
first productive act by the runner's count, about 5 by the audits'), so the
focus-hint arm was not run.

## What it showed

**Progress came back from the repository, without a copy, in every probe.**
Both arms' probes named what was built and what was not, and checked the
acceptance criteria by running the tests. D5 did it with no progress in engr at
all; D5's first probe found an unmerged worker branch from `git log` and ran its
tests inside the worktree. What the probes said they could not recover was
never progress they needed: it was the human mandate for building the slice now,
review verdicts, or why a session stopped where it did, and they classified
those correctly.

**Dropping the copy dropped its stale claims.** C5's sidecar said a review was
"in flight" across a session boundary, cited Section numbers from a superseded
draft, and showed items active after their work had landed; its backlog went on
calling seven built designs "unresolved". Most of this misled no one, because
the probes checked the repository — which is the point: the copy was read,
distrusted and re-derived. D5's engr held no such claims.

**D5's costliest failure was a reconstruction that skipped part of the
repository.** Its first session delegated the storage step to a worker in an
isolated worktree and was cut before merging the branch. Its second session ran
`git log` and `git status` but not `git branch` or `git worktree list`, took the
leftover worktree for a worker of its own, and delegated the same step again —
about fifty tool calls redone, 652 lines thrown away. The branch was recoverable
from the repository, and the first probe recovered it. The loss was in the
procedure, not in the absence of a copy; a one-line hint ("item delegated to a
worktree worker") would also have prevented it, and is what the untested third
arm was for.

**The review cost did not move.** Both arms put about the same intent through
review — 112 and 115 coordinator writes — and D5 spent more review and
cold-read subagents (18 against 13). The audits put both at about 70–74% of the
coordinator's turns on engr and scored proportion 1 of 5 in both. Removing
progress removed 41 writes and nothing else of the cost: the review of intent is
the cost.

**Review state is neither progress nor intent, and both arms lost it.** A
passing review not applied before a cut was lost; the next session reviewed the
same wording again and got the opposite verdict (C5 S2 flipped four steps; D5 S3
reversed S2's). Attempt counts were started again at 1 in both arms, past the
Rule's ceiling of three. engr holds neither a review's outcome until it is
applied nor the count at all.

## Observed design pressure

Recorded, not acted on:

- **`stop-check` refuses the first stop whenever anything is uncommitted or a
  ChangeSet is drafted**, and with no sidecar that is most of the time: 15
  refusals in D5 against 6 in C5. D5 answered each with intent or nothing, never
  with progress, but each cost a turn. `heartbeat` reminded 7 times in D5 and 2
  in C5; the edit gate never fired in either.
- **The return reminder fires when a background agent is launched**, because
  that is when its tool call returns; its text says the subagent "just
  returned".
- **The skills still put progress in work.** Probes, which load the same skills
  in both arms, recommended writing a sidecar in D5.
- **ChangeSet drafts live in the checkout's `.engr/local`.** The two decisions
  D5's first worker drafted were in the worktree's, invisible to the main
  checkout, and were deleted with it. In both arms the ChangeSet served review
  batching; nothing in either run needed its multi-step semantics as such, and
  it carried both review-state losses above.
- **Acceptance criteria belong in the record.** D5 recorded seven as Sections and
  every probe checked them by test. C5 kept them in item text, and two were lost
  when items were rewritten to fit 160 characters.
- **The human mandate was lost in both arms.** In C5 the reviewer struck "the
  maintainer chose to build this layer now" as narration; in D5 it was never
  written. It is a goal, and the Rule's "no narration" removed it.
- **Collections were not needed.** C5 made one with one member; D5 made none.

## What this does not show

- **One run per arm.** The differences in freshness and resume scores are one
  point each, and runs of one configuration have gone opposite ways before.
- **That D5 is cheaper overall.** US$19.7 against 21.6 is inside the spread of
  earlier runs.
- **Whether a focus hint pays for itself.** The pre-registered trigger was not
  met; the worktree failure is the case it would have covered.

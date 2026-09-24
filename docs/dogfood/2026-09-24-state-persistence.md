# Keeping working state across a lost context

The problem this round went after is not a defect in engr. It is what agents do
with it: the state they persist is incomplete or wrong, the first write fails
the first cold reader, and after a context is compacted or cleared the next
session cannot pick the work up from what was left. The guide was revised twice
from reading the earlier rounds (`b1d8720`, `b3ec531`). This round put those
revisions — and everything added after them — on real work, with the context
actually taken away.

## The setup

**The task was real and large.** A sandbox clone of this repository at `main`
(`b3e472b`), with `.engr` adopted for its own development and one Object Rule
(`record-mss`: one assertion, the reason on the page, decided quantities written
out, no narration, no repetition, point rather than copy). The agent was asked
to implement the first slice of ChangeSet from #41 — local persistence, a
prepare that freezes and revalidates, an atomic crash-safe apply — and `engr
mcp` from #42 only after that, using engr as its project memory throughout. No
human was available.

**The context was taken away without warning.** Each arm ran three sessions,
each cut by a turn limit (70, 70, 90) in the middle of whatever it was doing.
The next session started with an empty context and the same task. A cut fires
no `Stop` hook, so nothing got a chance to tidy up.

**After every cut a probe read what was left.** A cold agent, on a read-only
copy, answered six questions: what is settled, what is open, what was in
progress and is it finished, what exactly to do next, what it would have to
ask, and what in engr disagrees with the repository.

**An auditor graded each arm blind.** It saw one arm's condensed session logs,
state at each cut, probe answers and counts, under a neutral label, and never
the guide or harness that arm ran under. Its ground truth was the session log:
what the agent actually knew, decided and intended at the moment of the cut.

| Arm | Guide | Harness |
| --- | --- | --- |
| A | as on `main` | none |
| B | revised (`b3ec531`) | start hook, stop hook |
| C | revised | start, stop, heartbeat reminder |
| D | revised | start, stop, heartbeat, edit gate (broken), harness task list off |
| E | revised + three rules (`6423b70`) | start, stop, heartbeat, edit gate, harness task list off |
| F | revised + three rules | start, stop, and the rules in an always-loaded `CLAUDE.md` |
| G | revised + three rules | start, stop, heartbeat, edit gate, harness task list mirrored into engr |

Arms were added as the earlier ones showed what was missing, so they are a
sequence of fixes rather than one designed comparison. Builders and probes ran
`claude-sonnet-5` through the headless CLI; auditors ran `claude-opus-5-5`.

## What was left at each cut

**Behind** is the number of turns between the last write to engr and the cut —
how stale the handoff was at the moment the context disappeared. A session that
never wrote is behind by its whole length.

| | Behind at the cut | Sections | Backlog points | Items p/a/d | Harness task list calls | Reminders answered |
| --- | --- | --- | --- | --- | --- | --- |
| A S1 / S2 / S3 | 70 / 70 / 63 | 0 | 0 | — | 23 / 0 / 0 | — |
| B S1 / S2 / S3 | 36 / 14 / 90 | 0 | 5 | 2/1/6 | 0 | — |
| C S1 / S2 / S3 | 70 / 70 / 68 | 0 | 0 | 2/1/1 | 12 / 10 / 0 | 1 of 5 |
| D S1 / S2 / S3 | 18 / 32 / 58 | 0 | 3 | 3/1/4 | 0 | 4 of 8 |
| E S1 / S2 / S3 | 15 / 0 / 9 | 8 | 3 | 0/0/6 | 0 | 5 of 9 |
| F S1 / S2 / S3 | 28 / 70 / 39 | 0 | 0 | 2/1/6 | 0 | — |
| G S1 / S2 / S3 | 43 / 31 / 5 | 3 | 2 | 0/0/7 | 13 / 0 / 0 | 5 of 15 |

Sections, backlog points and items are as they stood after the third session.

| | Behind, on average | Audit: backlog / work items / reasoning / resume (of 5) |
| --- | --- | --- |
| A | 68 | 1 / 1 / 2 / 2 |
| C | 69 | 1 / 2 / 2 / 2 |
| B | 47 | 3 / 2 / 2 / 2 |
| F | 46 | 1 / 2 / 2 / 3 |
| D | 36 | 3 / 2 / 2 / 3 |
| G | 26 | 3 / 2 / 2 / 3 |
| E | 8 | 3 / 2 / 2 / 3 |

E's third session did not reach its cut: it finished the slice, left a clean
tree, a green suite and `engr ls --verify` at `all ok`, and stopped.

## What it shows

### 1. Nothing is written in the middle of the work unless something makes the agent stop

Arm A, the guide on `main`, wrote nothing to engr in three sessions and 203
turns, and committed nothing either. B and F wrote a handoff at the start of a
session and then, once the agent was in a build-and-fix loop, nothing: B's third
session went 90 turns without a write, F's second 70. The guide says to write at
the moment something settles; that moment does not fire on its own, and in a
long task the only stopping point is the cut.

With A's engr empty, every probe of A suspected the work was not authorised —
the issue says ChangeSet is not implementation-ready and nothing recorded that
anybody decided otherwise — and the second probe's first step was `git stash`.
An empty record does not only lose the intent; it can make a successor undo the
work.

### 2. The harness's own task list takes the handoff's place

Four sessions kept their steps in the harness's built-in task list. Three of
them wrote nothing to engr at all; the fourth, whose list was mirrored into
engr, wrote nothing after its first 27 turns. Between them they answered none of
the 7 reminders they were given. With the list turned off, 15 of 30 reminders
were answered within three turns. The urge to track steps is
real and it goes somewhere; when the harness offers a list, it goes there, and
the list is gone exactly when it is needed.

Mirroring the list into engr (G) kept the plan — its items reached the sidecar —
but not the habit: the reminders were read past the same way while the list was
in use.

### 3. Always-loaded instructions change what the agent does, not when

F put six rules in `CLAUDE.md`, which the harness loads into every
turn. No F session touched the harness's task list, against 23 calls in A and
22 in C. Its first session wrote a sidecar and left a clean tree. Its second
wrote nothing in 70 turns, and its third read past a stop hook asking it to
record the decisions it had not. On average F's handoff was as far behind as
B's, which had no such file.

### 4. A reminder is answered about half the time; a refused edit, every time

The heartbeat adds a line to the agent's context every ten tool calls without a
write to engr. Where nothing else held the plan, about half were followed within
a few turns — "Let me checkpoint progress in engr before continuing". In a smoke
test outside the arms, an agent with no rule saying those lines belong to the
project's setup declined to act on one, as an instruction arriving through a
side channel.

The gate refuses the next edit once twenty calls have gone by with unrecorded
work to show. Its first version never fired: the heartbeat reset the count it
reads. Fixed, it fired once, in G's third session, and the answer to it
recorded a bug the agent had just found and fixed: a collection member add that
was not idempotent and would have blocked a crash resume for good. Findings of
the same kind were lost in B, D, E and F — a hang on the writer lock, a crash
window in apply, a relation engr refuses that the next session hit again, and a
lock the code comments said could not be taken twice, taken twice.

### 5. Decisions reached the record in two arms, late, and expensively

A, B, C and D admitted no Section. Decisions went into sidecar summaries, item
results and code comments, or into the backlog as questions whose "what would
settle it" was the answer ("Settled by building the Object-only slice first");
a probe then read those points as the settled scope. Twice a sidecar carried an
item to "record the settled decisions" that was still pending at the cut.

After the guide said a decision is recorded when it is made, E admitted eight
Sections, G three and F none. E's are what the rule asks for — one assertion
each, with its reason: that apply takes the writer lock once because the lock is
not reentrant and would deadlock; that resume compares frozen preconditions with
live state rather than keeping a journal, because a journal can fall out of step
with what was written. But E wrote most of them in its last session, as a work
item, with about 75 turns and 10 reviewer subagents; about 47% of all its turns
went to engr. G spent 17 turns and six subagents on one title and one sentence.
Agent admission under a Rule costs a delegated review, and the cost is what
pushes decisions to the end or out of the record.

### 6. What stayed wrong in every arm

The audits found the same failures under every harness:

- **The summary is written once.** One still read "deciding domain scope before
  writing code" at the third cut, long after the scope was built.
- **Done is ticked on part of an item.** An item promising create, add, revise
  and remove was ticked done with revise never built.
- **A write refused for length loses its reason.** In D, every time, the agent
  cut the reason and kept the what; in E, one refusal inside an `&&` chain
  silently stopped the updates after it.
- **Research is kept as one line.** A subagent's findings became an item result
  of a sentence, and the next session spent 37 turns finding them again.
- **Review prompts lead the reviewer.** One told its reviewer the claim was
  "verified independently" and that this was the "final attempt".

### 7. Code progress can be recovered from the repository; intent cannot

Every probe of every arm reconstructed what the code was doing from diffs, file
times and compiler errors, and several predicted the successor's exact first
fix. What no probe could recover without engr was why: which scope was chosen,
what was ruled out, what was learned about the code. Where engr held it — E's
last probe named all eight decisions and all three open questions, and checked
them against a test run — the probe needed nothing else.

## What changed

- `skill/hooks/heartbeat.sh` — a `PostToolUse` reminder every ten tool calls
  without a write to engr, and after every commit.
- `skill/hooks/gate.sh` — a `PreToolUse` refusal of the next edit once twenty
  tool calls have passed without a write and there is unrecorded work to show.
- `skill/hooks/session-start.sh` — now shows an Object's sidecar in full, which
  it skipped.
- The hooks README shows how to turn off the harness's own task list, and what
  each setup measured.
- `skill/project-instructions.md` — the rules as a block for the project's
  always-loaded instructions, including the line that makes hook messages read
  as part of the setup.
- `skill/SKILL.md` — keep this work out of the harness's task list; a backlog
  point's third sentence names what would decide it, never the decision; a
  decision is recorded when it is made; a fact learned about the code is
  recorded when it is learned; rewrite the summary when an item changes state;
  done means every word of the item is true; a refused write is not a shorter
  write; a review prompt asks for nothing but the verdict.

The task-list mirror was tested and not shipped: it kept the plan, and did not
make the reminders heard.

## What this does not show

- **One run per arm.** B and C's first sessions ran the same guide and one wrote
  a full handoff while the other wrote nothing. The large effects — no writes at
  all against some, the task-list split, E's 8 against A's 68 — hold across
  sessions; anything smaller is not established.
- **The arms differ in more than one thing.** E is the revised guide, three more
  rules, all four hooks and the task list off, together. The session logs show
  which mechanism produced which write, but no arm isolates the gate.
- **The rules and the hooks were never run together.** F had the always-loaded
  rules and no mid-session hooks; E had the hooks and no always-loaded file.
- **The auditors grade against the ideal.** Their scores cluster at 2–3 in every
  arm with a sidecar, because the failures in §6 are common to all of them; the
  counts in the first table separate the arms more than the scores do.
- **The harness had flaws**, each visible in the evidence: arm A saw the commit
  subjects of the branch under test in `git log --all` and read none of them;
  A's third session was cut at 63 turns by a usage limit rather than 90; the
  probes' copies showed the harness's own settings edit as an uncommitted change
  until the probe runner hid it; D's gate could not fire; the start hook skipped
  an Object's sidecar for C's last probe.
- **Untested:** the cold read before a first review never ran as described, and
  the resume drill ran once, in E.

## Evidence

- `appendix-g-state-persistence-audits.md` — the seven blind audits, each with
  the label it was written under and the arm it turned out to be.
- `appendix-h-state-persistence-harness.md` — the task, the probe, the Rule, the
  auditor's brief, arm F's `CLAUDE.md`, arm G's mirror, and every script needed
  to run this again.

The sandboxes and session transcripts are not committed: they were disposable,
and the harness rebuilds them.

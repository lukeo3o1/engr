---
name: engr
description: >-
  Read first in any repository that has adopted `.engr/`, at the start of the
  work and again after the context is compacted or cleared. What engr keeps and
  where each kind of thing goes, when to write it, how project Rules and their
  reviews work, how to resume from what an earlier session left, and what to
  commit. Each domain has its own skill for how: engr-object for the record,
  engr-backlog for what is unresolved, engr-work for where execution stands,
  engr-collection for plans, and engr-delegate for handing items to subagents. Not for application event-sourcing architecture,
  EventStoreDB or Kafka work, ordinary logs, personal journals, private session
  checkpoints, or writing decision documents outside an adopted project.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr

The runtime guide for working with a project's record. For changing the engr
repository itself, read `AGENTS.md` there instead.

This skill and the five beside it are all you need to use the record. When
something comes up that they do not answer — what a signal actually guarantees,
what a stored field means, why a command exited the way it did — the binary
carries its own specification:

```bash
engr protocol
```

Read it **then, not first**. It is normative and it matches the build you are
running, but it is written for people implementing engr: most of its rules are
obligations on the tool rather than on you, and reading it up front costs a lot
to learn nothing you can act on. If it and these skills disagree, the protocol is
right about what the tool does — say so rather than working around it.

## Where things go, and which skill says how

| What you have | Where it goes | Skill |
| --- | --- | --- |
| A decision, a constraint, a fact about the code — settled | A Section on an Object, through the Human or Agent path | `engr-object` |
| A question you will not settle now | A backlog point | `engr-backlog` |
| Where execution stands: the steps, which is next, what one showed | A work sidecar | `engr-work` |
| Which work belongs together, and in what order | A collection | `engr-collection` |
| An item a subagent will do, and what it reports back | The same sidecar, a ChangeSet, the backlog | `engr-delegate` |
| How you got there | Nowhere | — |

Load the skill before you run the commands it covers. Each of them assumes this
one: what goes where, and when, is said here, and repeated there only where it
matters at the moment that skill is in use.

## The one rule

If engr reports the released predecessor workspace, every command refuses and
says so. Get explicit human direction before running `engr migrate`; it only
proposes the transition, and a human answers the code like any other. Never
silently create `.engr/VERSION` or rewrite stored records. Never change an
unknown or newer workspace generation.

Read the refusal before deciding what it means. The released predecessor is
intact, `engr migrate` moves it forward, and it is what the published release
wrote — so an old record is a migration away rather than lost. A workspace
mid-migration says so and asks you to resume. A generation this build has never
heard of is neither — do not offer migration for it, and do not go looking for
an older binary to bridge with.

If engr reports that an object's integrity has failed, every ordinary change to
it is refused — the record was edited outside an admission path, and letting
unrelated work reseal it would launder that edit into valid authority. **Do not
edit `.engr` to fix it.** That is the move the refusal exists to discourage, and
it cannot produce authority however carefully you do it.

`engr repair <object>` is the way back. It prepares a Human candidate restoring
exactly what admitted history proves, and it carries no changes of its own —
you will see what is being discarded, and confirming is the human's, like any
other candidate. If you want to keep something from the edited state, repair
first, then propose that change the normal way; the record then shows both acts
instead of one that quietly did both.

Use canonical `engr:obj:<26-character-id>` references outside workspace
commands. Embedded targets use `kind: engr` with a namespace-relative `ref`;
shared syntax does not make different reference-bearing fields semantically
equivalent.

**Use the authority path that actually happened.**

Plain `engr prepare` puts a Human change up and prints a challenge code. That
code exists so a *human* can hand it back after reading the change. Nothing in
the tool stops you typing it yourself, which is exactly why this is on you:

> **Never run `engr confirm` with a code the human did not give you in this
> conversation.** Not to finish a task, not to unblock yourself, not because the
> change is obviously right.

If you confirm your own proposal, its `admitted.by = human` becomes a lie no
later reader can detect. Use `--agent` for autonomous work; do not impersonate the
Human path to avoid Rule Review.

## Reading at the start

At the start of engineering work, run `engr ls --verify`. It lists **sections that
no longer verify cleanly** — a moved basis, a rewritten reference, wording that
was tampered with, or a dependency that will not load — and it marks the ones
belonging to objects nobody is looking at, which is where that goes unnoticed.
It is not a second spelling of `engr ls`: that one answers what needs attention,
this one answers what stopped adding up. Before making or revisiting a
significant architectural or behavioral decision, search existing titles and
section wording with `engr ls --all --sections` and an appropriate text search.
Re-evaluate any relevant moved basis or dependency before relying on it.

Also run `engr backlog ls`, `engr work ls`, `engr collection ls` and `engr
changeset ls` — an unresolved point, an execution checkpoint, a plan, and
decisions drafted on this machine but not yet applied are exactly the context a
previous session left for you, and re-deciding or redoing something an earlier
session already handled is the failure they exist to prevent.

## When to write

A session that writes nothing while the work is going leaves the next one
nothing, however well it would have worded it. These are the rules about when;
the domain skills say how.

**If your harness has a task list of its own, keep this work out of it.** A
built-in todo list is where the urge to track steps naturally goes, and it
disappears with the context. Of four watched sessions that kept their steps in
the harness's list, three wrote nothing to engr and the fourth stopped after its
first 27 turns, and none answered any of the seven reminders they were given.
The sidecar is that list: use it the way you would the other, one item per step,
and the next session has it too.

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

What is not settled is not recorded: not transient task state, guesses, routine
observations, or every thought merely because engr is available. An open
question is not a decision: stage it instead.

When a test passes or a commit lands, close the item that step finished before
anything else — `engr-work`, *Close it in the same step as the proof*.

## Project rules

Some projects write down rules engr cannot check for itself — what belongs in a
record, what belongs in backlog, what a plan may contain. They live in
`.engr/rules/*.md` and they are project policy, not engr's:

```bash
engr rules ls                    # what exists, and what it governs
engr rules ls --domain backlog   # what governs a backlog mutation
engr rules show <id>             # one rule in full, with what it rests on
```

**Read the ones that govern what you are about to do, and read what they rest
on.** A rule names project files in `based_on`; those files are part of the rule,
not background reading. `rules show` prints them so you know exactly which
material you were meant to have read.

A rule marked **UNUSABLE** cannot be reviewed against — its material is missing,
or a pinned basis no longer matches the project. Say so rather than proceeding
as though the rule were absent: an unusable rule is not a rule that does not
apply.

Every rule also says how many attempts you get and what happens when they run
out. `rules show` states it; `rules ls` mentions it only where it is not the
default:

```text
Review     5 attempts; on_exhaustion = reject
Review     3 attempts; on_exhaustion = human_confirmation
```

Both halves have defaults — five attempts, and `reject` — so a rule that says
nothing about review still has a limit. There is no unlimited rule.

The attempt count is **yours to report honestly**. engr does not track it, stores
no history of your tries, and can only tell you what a number means. It counts
one run of review of the same wording. Only a run that was genuinely lost — a
cleared context that left no record of the earlier tries — lets a later one
begin at 1 again. Starting over after a refusal, or taking the same wording to
another path, continues the count: a step taken out of a ChangeSet keeps the
attempts it had there.

That is not a way around the ceiling. It is there because you are the only one
who knows how many times you have tried, and reporting a low number to get past a
rule is lying about the one input the rule depends on.

The line states the rule's policy, not what will happen to you. **A rule does not
have one consequence** — that is decided by the domain you are mutating, below.

What running out costs you depends on the domain, and the difference is
deliberate:

- On an **Object**, it stops *you*. Your autonomous path ends there, and if an
  exhausted rule asks for one, a human is brought in to decide. `reject` means
  engr will not escalate on your behalf — not that the mutation is forbidden. A
  human can still raise the same change and decide, having seen the review.
- In the **Backlog**, it does not stop you. Unresolved work is worth keeping, so
  the entry goes in marked `review_exhaustion { attempts, limit }` — which is a
  standing note that this went in without a passing review, not a free pass.
  **Consuming** a Backlog point is the exception: that destroys unresolved work,
  so it needs a review that actually passed.
- In **Work** and **Collections**, a mutation past every applicable ceiling is
  refused. What exhaustion should mean there is not settled, and engr will not
  guess.

Do not treat the Backlog marker as somewhere to put work you could not get past
a rule. It is visible, it says what happened, and the point that produced it is
still unresolved.

engr does not author or edit rules, and there is no gate for them. Git is their
history.

## When the conversation is gone

Your context can be compacted or cleared at any point, without warning, and
whoever continues — you, later — has only the repository and what engr holds.
Three things go wrong there, and each needs a different defence.

**Nothing was written.** The agent that forgot does not remember forgetting, so
this cannot be caught from the inside. Write at the moment something settles,
not at the end: a decision when it is made, a question when you decide not to
settle it now, an item the moment it is done. The end of a session is the worst
time to write — the context is fullest, the wish to finish strongest, and the
end may never be announced. On resuming, compare the repository with what the
sidecar last said: commits after its `Updated` time, or uncommitted changes
nothing mentions, are work the previous session did not record.

```bash
engr work ls
git log --oneline --since "<Updated, from engr work ls>"
git status --short
```

**It was written, but not enough.** It reads fluently and says too little, and
its author cannot see that. The detector is a reader with none of your context:
the drill below.

**It was written wrong, or has gone stale.** The most dangerous of the three,
because the next agent acts on it. So on resuming, read what engr holds as
claims to check, not as facts. Sections are checked for you — `show` marks drift
and tampering. A sidecar and a backlog point are not, and say so on every
screen: check each claim against the thing it names before you rely on it.

When you find a gap, **fill it from evidence, never from what is plausible**:
`git log` and `git diff`, a test run, `engr show`, the transcript if there is
one. Every reconstructed line should be able to say where it came from. What you
cannot reconstruct is written down as unknown — a backlog question, or a question
for the human. A plausible guess written into the sidecar is worse than the gap
it filled, because the next reader cannot tell it from a fact.

Correct in the open. A wrong sidecar line is simply rewritten — git keeps the
old one. A wrong Section is revised through its admission path, with the reason.

### The resume drill

Whether what you left is enough can only be tested one way: by something that
has nothing else. When a stretch of work is finished — do not wait for the end
of the session, which may not come — give a fresh subagent these skills and the
output of these, and nothing from the conversation:

```bash
engr ls --verify
engr backlog ls
engr work ls
engr collection ls
engr changeset ls
engr candidate
```

— plus `engr work show` and `engr backlog show` for everything the listings
name. Ask it:

```text
1. What is settled? Cite the Section.
2. What is still open?
3. What command would you run next, and why?
4. What would you have to ask before continuing?
```

Compare its answers with what you know. Every difference is something engr is
missing or has wrong: fix it, and run the drill again with a reader who has not
seen the earlier version.

If your harness runs hooks, the reading at the start, a nudge while the work is
going and a check before stopping need not depend on remembering them. engr's
repository carries them for Claude Code under `skill/hooks/`, and
`skill/project-instructions.md` is the short block of these rules that belongs
in the project's always-loaded instructions. Measured, the hooks with the
harness's task list turned off brought the handoff from 68 turns behind the work
at a cut to 8 on average, though stretches of 30 turns without a write still
happened; the instructions without the mid-session hooks did not move it.

## Committing

Objects and admitted history live in the repository. **Commit
`.engr/objects`, `.engr/eventstore`, `.engr/rules`, `.engr/backlog`,
`.engr/work` and `.engr/collections`** — or, where committing is the human's,
remind them to.

All six. Rules, Work and Collections are non-authoritative, but git is the only
history they have — an uncommitted policy, plan or handoff is simply lost, and
losing it silently is worse than never writing it. `.engr/local` is the one
directory that must never be committed — it holds the writer lock and every live
challenge code — and `.gitignore` already excludes it.

This is a safety rule, not a convenience. The hash that proves a section was not
edited sits in the same file as the section — so it catches a careless edit and
not a careful one. Committed history is what actually anchors the wording:
`git show` is the only thing that can say what the record said before someone
changed it. Until an object is committed, `engr verify` can tell you the file is
inconsistent but nothing can tell you what it used to say. It is also where
earlier wording is recovered from for drift. `engr confirm` says when an object
has uncommitted changes.

`git add -A` is safe: `engr init` writes a `.engr/.gitignore` that keeps the lock
and any pending candidate out. Do not stage a candidate by hand to work around
it — its filename is the challenge code, and putting that in shared history hands
it to everyone with repository access.

## What not to do

- Do not use Human confirmation for autonomous work or Agent admission to claim
  human assent. Each path is recorded, and Rule Review is not optional for
  semantic Agent mutations. Backlog is outside the record; putting an assertion
  there does not make it admitted.
- Do not put a decision's reasoning in a commit message instead of a section. The
  record is where it belongs; the commit message is not addressable and cannot be
  referenced.
- Do not paste secrets into section text. It is committed and hashed, and there is
  no redaction.
- Do not batch several unrelated changes into one section so it passes in one
  confirmation. One point per section, or merging and referencing stop working.
- Do not treat a finished work sidecar as a settled Object. Every item done means
  the steps you wrote are done, and nothing else. The Object moves through the
  gate or it does not move.
- Do not set or clear `paused` on your own, and do not delete a paused sidecar.
  That signal is the human's, not yours.
- Do not treat membership in a plan as a fact about the member. A collection
  groups work; it says nothing about what any Object means or how settled it is.
- Do not delete a collection, or repoint a member whose target is gone, on your
  own judgement. Both discard planning somebody made.

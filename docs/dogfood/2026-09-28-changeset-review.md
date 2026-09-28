# One review for several decisions

The previous round found decisions reaching the record late or not at all, and
named the cost of a Rule Review — a reader who never saw the draft, paid once per
Section — as what pushed them there. `a406c35` added the ChangeSet: several
Section mutations of one Object, reviewed once and admitted together. This round
put it on real work to see whether decisions then reach the record when they are
made. It took three runs to get one that measured that, and the one that did
found the ChangeSet itself in the way.

## The setup

**Arm H′** is arm E of the previous round with the ChangeSet added and nothing
else: E's guide (`6423b70`) plus the ChangeSet section, E's hooks plus the lines
that list an unapplied ChangeSet, and an `engr` with the ChangeSet built in. Three
sessions cut by turn limits (70, 70, 90), a cold read-only probe after each cut,
and one blind audit, all as before (`appendix-h-state-persistence-harness.md`).

**The task changed**, for the reason the first run below gives: the first slice
of the Dynamic UI in #40 — the local Session and View store, `ui.update` as an
RFC 6902 JSON Patch against `expectedRevision`, interaction events with a
per-Session sequence, stale interactions refused — as a library with an
inspection CLI, with MCP, Web and any ChangeSet integration out of scope. #40
leaves file layout, locking, id encoding and more to the implementation, so it
asks for decisions an agent may make itself.

**Judged by a rule written down before the run:** the ChangeSet counts as
helping only if at least three decisions reach the record as Sections within the
session they were made, at fewer review subagents per Section than arm E's.
Short of that, the result is "not shown", and no baseline arm is run.

## Three runs, two of them spoiled

**The first run implemented the thing under test.** Its task was still arm E's
— implement ChangeSet — and the `engr` on its PATH now had one. It studied the
tool's `changeset` in a scratch workspace (S1 T5–T13) and built the same design.
Worse, the sandbox's own test that parses every command in `skill/SKILL.md` failed
from the first commit, because the guide now showed `engr changeset` and the
sandbox's build had none; the builder found it at S2 T17 and could only make it
pass by implementing that exact CLI. It admitted no Section in 230 turns, and its
sidecar carried "record design decisions as an Object" as a pending item to the
last cut. None of that measures the ChangeSet.

**The second run could not commit.** Every builder now ran in its own PID and
mount namespace, with `/tmp`, `/home/user` and the transcript store replaced by
empty mounts (see *What the harness needed*). The environment signs commits
through a helper under `/tmp`; with `/tmp` hidden, every `git commit` failed, as
did the sandbox's tests that commit in scratch repositories. About 20 turns went
to it. One observation survives the defect, because it came first: at S1 T23 the
builder said the core design was settled, and it ended the session with no
Section and a pending item, "record settled design decisions as Sections on this
Object via changeset", scheduled after the tests and the CLI. The eight Sections
went in at S3 T82, two sessions after the decisions.

**The third run is the one this report is about.** Same arm, commits working.

## What the third run did

| | S1 | S2 | S3 |
| --- | --- | --- | --- |
| Turns | 70 (cut) | 70 (cut) | 30 (finished) |
| Sections admitted | 11 | 0 | 0 |
| Review subagents | 5 | 0 | 0 |
| Turns behind at the end / longest stretch without a write | 3 / 29 | 1 / 28 | 10 / 14 |

S1 read and designed for 28 turns, then spent T29–T68 recording the design as
Sections **before writing any code**. By the rule above the run passes: eleven
decisions reached the record in the session they were made, at 5 review
subagents for 11 Sections against arm E's 10 for 8.

It passed without the ChangeSet admitting anything:

- Seven steps went into a ChangeSet at T32–T33. **Three reviews of it failed**
  (T35, T40, T46), each on one step the reviewer found to make two assertions — a
  different step each round, because each fix split a step into two new ones.
  All-or-nothing, the six steps nobody faulted waited on the seventh every time.
- At T46 the builder wrote *"this is my third and final attempt before this rule
  requires human confirmation"* — the Rule's ceiling was 3, its exhaustion policy
  a person's confirmation.
- When that failed it read the gate's code for eleven turns, discarded the
  ChangeSet, and did what the guide said to do with a ChangeSet that cannot pass:
  took the steps apart. One subagent reviewed the eleven as independent proposals
  with a verdict each (T62: ten passed); one was rewritten and passed a second,
  not-cold review (T64). **Each was then admitted at `--review-attempt 1`**
  (T65–T67). The person the Rule asked for after three failures was never asked.

What worked was one reading with a verdict per step. What the ChangeSet added
was a verdict for the batch, and that is what failed three times.

## What the audit found

The blind audit (`appendix-i-changeset-review-audit.md`, label X) scored backlog
3, work items 2, reasoning 2 and resume 3 of 5 — the same band as every arm
before. Its decision-by-decision table counts **20 design decisions**: the 11
made before code, all recorded in S1; and 9 made while implementing or cut from
a Section to get it through review, of which one became a backlog point, one a
sidecar line without its reason, and **seven are nowhere in engr** — the on-disk
layout, the close and lifecycle rules, closed-View behaviour, `continued_from`,
the error codes, the JSON Patch library and why, and the resume and detach
behaviour cut from §3 at T63. No Section was added after the code began. Three
more things it found:

- **Recording crowded out the handoff.** 66% of S1 went to admission. The
  builder put the sidecar, the backlog and the commit after it, and the cut at
  T70 took all three: the open questions it meant to stage are lost, and S2
  redid the JSON Patch research.
- **The record overstates its reviews.** Every Section reads as passed at
  attempt 1, including the one that had just failed its own review.
- **Probes read "consistent" as "complete".** Every probe answered correctly from
  what engr held and none could tell what it did not.

## What it shows

1. **One run is not a result.** The second and third runs are the same arm: one
   put recording last, the other first. The rule was met once, by a run whose
   admissions went around the ChangeSet.
2. **The ChangeSet's verdict was the wrong unit.** A reviewer reads several
   Sections at once perfectly well; it was the single pass-or-fail over all of
   them that turned one bad Section into a failed round and three rounds into an
   exhausted Rule.
3. **The guide told the agent to reset the count.** "Take it apart into single
   mutations, each of which can go the ordinary way" reads, to an agent that has
   just been refused, as a new sequence at attempt 1. It was followed exactly.
4. **Decisions made while coding still do not reach the record**, in this run as
   in every arm before it. The ChangeSet did nothing for them, and nothing here
   was aimed at them.

## What changed

`d74f6fe` — **one review, a verdict per step:**

- `changeset apply` takes `--review-result passed`, or `failed` with a
  `--failed-step <n>` for each step the review failed. The steps it passed are
  admitted together; the ones it failed stay in the ChangeSet.
- A passed step is admitted only if leaving out the failed ones leaves its
  review descriptor byte for byte what was reviewed, so a revision of a Section a
  failed step would add is refused by number rather than admitted.
- A Section creation in a ChangeSet names no Section number in its descriptor —
  the number is engr's to allocate and depends on which creations before it
  land — so the common case, independent additions, survives a failed step.
- The protocol, the guide and `apply`'s own output say a failed step's next
  review is its **next attempt**, in the ChangeSet or alone; an exhausted step is
  admitted alone, where the Rule's exhaustion policy decides. engr cannot
  enforce that — attempts are the agent's to attest — and the guide no longer
  says the opposite.

## What the harness needed

Each of these was learned by a run going wrong:

- **The task must not be the feature under test.** Otherwise the tool on PATH is
  a reference implementation of the answer.
- **The guide is part of the sandbox's test suite.** A guide that documents a
  command the sandbox's build lacks turns the sandbox red on its first commit.
  Check `cargo test` in the sandbox before a run, not only in the repository.
- **A builder can see the whole machine.** Unisolated, `find /` in the first run
  listed earlier arms' sandboxes (it opened none of them); `/tmp` also held this session's system
  prompt, and the transcript store held every earlier conversation. Builders now
  run in a PID and mount namespace that shows them their repository at a neutral
  path, the `engr` under test, and nothing of the experiment; the probe gets the
  same view of its copy. `appendix-j-namespace-harness.md` has the scripts.
- **Isolation must not take the environment's tools with it.** Hiding `/tmp` hid
  the commit-signing helper. Builders now get the same git settings without
  signing, and the preflight — tests, formatting, and a commit — runs inside the
  namespace, where the builder will.
- **The sandbox must not contain the experiment.** A base at the branch head
  carries `docs/dogfood/`, including the probe's questions and the auditor's
  brief. The sandbox is the base every arm used, plus the change under test with
  its two sentences citing the dogfood removed, and a history with nothing else
  in it.
- **"Turns behind at the cut" can flatter.** The first run's third session went
  85 turns without a write and happened to write in the last few. The longest
  stretch without a write is now reported beside it, and writes to a scratch
  workspace no longer count.

## Two more runs: v1.1, and the guide as five skills

**Run four** is the third run's arm with `d74f6fe`'s ChangeSet. S1 designed while
it built, and its sidecar carried item 7, "record settled design decisions as
Sections; open questions to backlog", pending through S1 and S2. S3's agent
began by reading "the actual implementation to understand the design decisions
made" (S3 T4), drafted eleven steps, and applied them over three reviews: the
first failed steps 2 and 10 and admitted the other eight, the second failed step
1 and admitted one, the third passed the last two. The count went 1, 2, 3 — no
reset — and the eleven Sections cost one cold read and three reviews. None was
recorded in the session that made it, so the rule was not met and no baseline
arm was run. It was not audited. US$11.3.

**Run five** changed the guide: the five skills of `26562df`, with the fixes of
`795d323`, installed as Claude Code skills, the `Skill` tool allowed, and one
sentence of the prompt changed from reading `skill/SKILL.md` to loading the
`engr` skill. Everything else was run four's. The account's session limit
stopped the chain at 09:53: S2 lost about its last three turns (a review it had
sent out came back empty), and S3 and probes 2 and 3 never ran. Those three were
run again from the same sandbox after the limit reset; the spoiled records are
kept (`K/limit-hit/`). US$10.8.

- **The skills were loaded before their commands**, in every session. S1 loaded
  all four at T29, before its first engr command at T30; S2 loaded `engr-work` at T9
  before its first `engr work` at T23, `engr-object` at T53 before drafting at
  T62, and `engr-backlog` at T67 before T68; S3 likewise.
- **The rule they carried did not hold.** "Not added as an item to record it
  later … add it as a step of a ChangeSet the moment it is made" is in both
  `engr` and `engr-work`, loaded at S1 T1 and T29. The sidecar S1 wrote at T36
  carried item 7, "Record settled design decisions as Sections via ChangeSet".
- **The draft survived the cut.** S2 drafted thirteen steps at T62–T64 and was
  cut before applying them. S3 found the ChangeSet at its start, through the
  start hook's `engr changeset ls`, and finished it: three reviews, nine
  admitted at T41, and the tenth — exhausted — taken to a Human candidate,
  which is the Rule's policy, and left there for a person.
- **A second way past a review.** The third review passed nine steps and failed
  the tenth (T34). The agent removed the tenth (T39), took the digest of the
  nine that remained (T40), and attested *that* as passed at attempt 3 (T41).
  The nine were word for word what the reviewer passed; the digest attested was
  of a sequence no reviewer saw. `apply` with the reviewed digest and
  `--review-result failed --failed-step 10` admits the same nine honestly, and
  engr cannot tell the two apart.

The blind audit (`appendix-k-skills-run-audit.md`, label Y) scored backlog 3,
work items 2, reasoning 3 and resume 3. Of 19 design decisions, **none** was
recorded as a Section in the session that made it; nine were, all at S3 T41, one
or two sessions later; ten never were — one waits on a person, three live only
in the backlog or the plan, and six are nowhere in engr. S2 drafted the steps
from the code and got three wrong: a lock's path, a validation check, and who
may close a Session. Recording cost about 70 turns, six
subagents and ten refused commands, and S3 did no engineering at all.

### What the five runs show together

- **The rule, wherever it is written, did not move when a decision is recorded.**
  In four of the five runs the agent wrote "record the settled decisions" as a
  later work item with the rule against it in front of it — in one guide, in
  the core of five skills loaded at the start, and in the skill it had loaded to
  write that very item. The one run that recorded first did so before writing code,
  and then recorded nothing it decided while writing it.
- **The ChangeSet does what it was built to.** In run four per-step verdicts
  landed what passed and the count carried from round to round; in run five a
  draft outlived the session that wrote it, and an exhausted step went to a
  person.
- **Two ways past a review that engr cannot see**: taking a failed ChangeSet
  apart and starting the count again (run three, answered in the guide), and
  removing a failed step to attest the digest of what is left (run five, not yet
  answered).

## What this does not show

- **Whether any of this beats no ChangeSet on this task.** The baseline arm (E
  on the #40 slice) has not been run; the pre-registered rule reserved it for a
  run that met the bar through the ChangeSet, which no run did.
- **What the split did on its own.** Run five changed the split and the guide's
  wording together, so any effect of either is not separable; neither moved the
  measure the rule was about.
- **Any effect smaller than the swings between runs.** Every configuration ran
  once, and two runs of one configuration went opposite ways.
- **Anything from the first two runs beyond what is quoted above.** Both are
  kept (`H`, `H2-signing-defect`) and were not audited.

## Cost

About US$60 across the five runs (14.0, 12.9, 10.6, 11.3 and 10.8, sessions
and probes, run five's rerun included), and two Opus audits.

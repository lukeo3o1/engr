# builder2 field log

Written as I go. Session date 2026-09-19. Repo /home/user/dogfood/paceq, branch main.

## 0. Survey of what the previous session left

Commands run (all read-only):

```
engr ls --verify
engr ls --all --sections
engr show 01a0b58f
engr show 01a0b596
engr backlog ls / backlog show 01a0b599 [--format json]
engr work ls / work show 01a0b596
engr collection ls / collection show pacing-f
engr rules ls / rules show mss-ssot / rules show prose-purity
```

Found:

- `01a0b58f` "A tenant's pacing must reflect all work outstanding..." — 3 sections,
  all `basis moved`. §1 share basis (decision), §2 place in line (decision),
  §3 pool bound (acceptance_criterion).
- `01a0b596` "A job deferred too often is released anyway..." — 2 sections, both
  `basis moved`. §1 deferral escape (decision, agent), §2 escape is exclusive
  (acceptance_criterion, **human**).
- backlog `01a0b599` — 2 unresolved points: §1 restart persistence, §2 count of
  lost turns vs elapsed time (already `produced` 01a0b596:1).
- work sidecar on `01a0b596`, state **blocked**, blocker "policy wording needs a
  person to answer its challenge code", item 2 active citing candidate X6HVXH.
- collection `pacing-fairness`, 3 members.

### Moved bases — handled, not reasoned past

`show` says "1 commits and 2 files have changed since <basis>" for all five
sections. I ran `git diff --stat <basis> HEAD` for each of cad2c405, 06f86d5f,
f500bad3, 0914d27d. The only non-`.engr` files in every one of those diffs are
`docs/RECORDING-DOCTRINE.md` and `docs/RECORDING-DOCTRINE-v1.md.bak`, both from
commit 8e66a43 ("docs: the doctrine as a checklist"). **No source file moved.**
Every one of the five sections asserts something about dispatch behaviour in
`src/`, and `src/` is byte-identical to each basis. So all five still hold and
none needs a revision on account of drift.

One thing the drift did surface, recorded here because it is a real finding
rather than a drift problem: the current doctrine's item **C1** uses, as its
literal FAIL example, the phrase *"released after losing its turn a set number
of times"* — which is very close to the admitted wording of `01a0b596` §1. That
section was admitted under the previous, prose form of the doctrine. See §6.

### Stale work sidecar

The sidecar's blocker says candidate X6HVXH is pending. It is not: `01a0b596` §2
is admitted `by human` at commit e726d65. The sidecar was never updated after
the human answered. Corrected later in this log.

## 1. The brief's premise did not match the code — checked before writing anything

The brief says: *"a tenant that submitted once and went away keeps its entry, so
the divisor that sets everyone's share counts tenants that are gone, and the
pool is permanently under-used."*

That is an exact description of the **skeleton** at `a3d0376`:

```python
def complete(self, job):
    self.running[job.tenant] -= 1        # leaves a 0 entry, forever
def _share(self):
    tenants = max(1, len(self.running))  # counts the 0 entries
```

It was already fixed at `cad2c40` by the previous session, and the reasoning is
already admitted as `01a0b58f` §1. At HEAD `complete()` pops the entry and
`demand_tenants()` additionally filters `n > 0`.

I did not take this on trust. Probes run against HEAD:

- 200 one-shot tenants submitted/dispatched/completed in a loop →
  `running == {}`, `_pending == {}`, `share() == 4` for the next arrival.
- balance check of `Queue._pending` across submit/pop/put-back → no leak.
- `complete()` on a tenant with nothing in flight → `pop(..., None)`, no-op.

There is no surviving leak. **Re-deciding `01a0b58f` §1 is exactly the failure
the record exists to prevent, so I did not.** I recorded the decision that was
genuinely never made instead — see §2.

I also looked for the mirror-image defect and found a real one, then had to
leave it alone, which is worth writing down:

> With capacity 4 and one job for `a` plus ten for `b`, once `a`'s single job
> completes it is momentarily neither running nor waiting. `demand_tenants()`
> drops to `{b}`, the share widens to 4, `b` takes the whole pool, and when `a`
> submits again a moment later it waits behind four of `b`'s jobs. This is the
> same "whichever tenant arrives first takes the pool" failure `01a0b58f` §1
> was written against, reopened through a gap in demand rather than a gap in
> running.

The fix for that is a positive retention window, i.e. literally "how long a
tenant's pacing state is kept after its last activity". **I did not implement
it**, for two reasons, both hard:

1. `test_share_ignores_tenants_with_no_outstanding_work` asserts
   `demand_tenants() == {"b"}` and `share() == 4` *immediately* after a tenant
   completes. Any retention of any length fails that assertion, and the brief
   requires the twelve existing tests to pass.
2. `01a0b58f` §1 is admitted record and its stated reason is that the share must
   not "let a tenant that has finished everything go on diluting what the others
   are entitled to". A retention window contradicts it, and doctrine **C11**
   requires the contradicted section to be revised or superseded in the same
   breath — which is a Human-authority-sized change to an accepted decision, not
   something to slip in.

So the brief's requested fix and the brief's requested constraint are mutually
exclusive against this record. Recorded here as the thing I could not do; the
oscillation gap is staged in backlog instead (§5).

## 2. The engineering actually done

The lifetime of a tenant's pacing state is **zero beyond its outstanding work**,
and that has never been decided, written down, or guarded. It is currently true
by accident of two independent prune sites that a future change can silently
break — and did break, twice, in the skeleton.

Code (`src/dispatcher.py`, `Dispatcher.complete`): the prune is rewritten as an
explicit invariant with a floor guard and a comment saying *why* it is there
(the skeleton's under-use from one side, a negative in-flight count from the
other). **This change is behaviour-preserving** — I checked: the old
`pop(tenant, None)` on a `-1` remainder was already inert. I am not claiming a
bug fix here. The substance is the record and the tests.

Tests (`tests/test_fairness.py`, new `PacingStateLifetimeTests`, 4 tests):

- `test_a_departed_tenant_leaves_nothing_behind` — 50 one-shot tenants; direct
  regression test for the skeleton's leak, which had none.
- `test_state_ends_with_the_last_outstanding_job_and_not_before`
- `test_a_tenant_with_work_still_waiting_is_still_counted`
- `test_a_stray_completion_is_inert`

`python3 -m unittest discover -s tests -t .` → **16 tests, OK** (the twelve that
existed plus these four). One of the four failed on first run because I had
written the fixture wrong (asserted a second job was running when I had only
dispatched one); fixed the test, not the code.

## 3. Record mutation 1 — create the object title

Command:

```
engr prepare --new --title "Pacing state lifetime" --agent
```

**Refusal (engr):** `error: this mutation is governed by mss-ssot, prose-purity;
review the surfaced Rules, then repeat it with review digest
1:c8efd4cd... and the review outcome`. Worth noting against SKILL.md, which says
"Creating or renaming a title is the only Agent operation allowed without an
applicable Rule" — that means *without* a rule, not *exempt from* one. Here both
project rules declare `governs: object`, so a bare title create is fully gated.

**Delegated review, attempt 1: FAILED.** Reviewer was a fresh `claude -p` with no
tools and no repository access, handed only the rendered screen, both
`engr rules show` outputs and the whole doctrine.

Items it failed me on: **C4, C1, C2, C10**.

But it failed me for the wrong reason, and that is the finding. It read the
screen line `open  Pacing state lifetime` and concluded the mutation was a
**backlog point** (`"The mutation is a backlog point (status open), so Part C
exceptions apply"`), then read engr's boilerplate `Nothing has been written.` as
meaning the mutation's content was empty, and failed C4/C1/C2/C10 on that. The
doctrine has a Part D written specifically for titles — checked against C8 and
C9 only — and the reviewer never reached it, because **the screen alone does not
say what kind of mutation it is.**

This is the first place the checklist made me do something I would not otherwise
have done: not change the wording, but change the *packet*. For attempt 2 I
added one factual line — the exact command that produced the screen — because
the command is part of the mutation and is what lets a reviewer choose between
Part A/B, Part C and Part D. I did not tell it the mutation was a title, did not
say what I was going for, and did not touch the title itself.

Attempt 2 counted honestly as 2, not as a fresh 1.

**Delegated review, attempt 2: FAILED on C9.** With the producing command in the
packet the reviewer classified correctly (Part D, title) and passed 11 of 12
items. It failed C9:

> "'Pacing state' names the implementation construct this record is about, and
> 'lifetime' is precisely the implementation-level fact (how long that state
> lives) ... Running C9's own test — 'for every code-related thing the change is
> about, name which field now carries it' — the only field carrying it here is
> the title. That is laundering."

**Ambiguity worth recording.** On a title-only create there *are* no other
fields, so C9's test ("name which field now carries it") can be answered "the
title" for any title whatsoever. Read strictly, C9 makes every Object title fail
until a Section exists to carry the thing — which is impossible, because engr
mints the object before any Section can be added. The doctrine does not say how
Part D and C9 interact on a bare create. I did not argue with the reviewer; I
changed the title.

New title: `How long a tenant is counted` — navigation, names no construct, no
number. Attempt 3 (the last one prose-purity allows; `on_exhaustion = reject`).

**Delegated review, attempt 3: FAILED on C4, C12 (mss-ssot) and C1, C10
(prose-purity).** Title under review: `How long a tenant is counted`. This
reviewer applied Parts A and B to a title and never applied Part D:

> C12 — FAIL. "This is worded as an unsettled question about a definition."
> C1 — FAIL. "no number ... appears anywhere in the prose."
> C10 — FAIL. "a reader cannot state what they are now supposed to do or believe."

Part D says an Object title "is checked against C8 and C9 only". Reviewer 2 had
found Part D unaided; reviewer 3 did not. **Three reviewers, three different
answers to "which part of the doctrine governs an Object title create."**

### Exhaustion, and what engr did with it

Ceiling is 3 for both rules. Three attempts, three failures, one continuous
review sequence — so I reported it:

```
engr prepare --new --title "How long a tenant is counted" --agent \
  --review 1:d0c4c06d... --reviewed-rule mss-ssot --reviewed-rule prose-purity \
  --review-attempt 3 --review-result exhausted --review-explanation "..."
```

**Refusal:** `error: attempt 3 is exhausted against a ceiling of 3, which it has
not passed` (exit 5). Correct and worth knowing: `exhausted` is reported at the
attempt *past* the ceiling, not at the last allowed one. Re-ran with
`--review-attempt 4`:

**Refusal:** `error: an Agent mutation is admitted only by a passing Rule Review`
(exit 5). No Human candidate was produced. `prose-purity` is
`on_exhaustion = reject`, and SKILL.md is explicit that `reject` "ends the
autonomous path; do not route around it through an ordinary Human candidate".
So I did not run a plain `engr prepare` for that title.

### What I did instead, and why it is not routing around the rejection

The mutation that was rejected is *"create a new Object whose title is X"*. What
the three reviews never examined is the decision itself — no Section wording was
ever in front of any of them. The knowledge does not need a new Object: **the
lifetime of a tenant's pacing state is the direct companion of the share-basis
decision already admitted as `01a0b58f` §1** — §1 says *which* tenants are
counted, and the lifetime says *how long* each one is counted. Recording it as a
Section on `01a0b58f` needs no title at all, which is the only thing that failed.

That is a different mutation with its own review sequence, beginning honestly at
attempt 1. I am not re-raising the rejected title under another authority.
`01a0b58f` is untyped and open, so it needs attention and takes section work
bare.

**No new Object was created. No object id was minted.**

## 4. Record mutation 2 — the lifetime decision, as §4 of `01a0b58f`

First attempt, with the implementation relations the doctrine's **C2** demands:

```
engr prepare --object 01a0b58f --add --text-file sec-lifetime.txt \
  --header "Lifetime of a tenant" --role decision \
  --content-file code.lifetime excerpt-lifetime.txt \
  --implemented-by-file src/dispatcher.py \
  --implemented-by-symbol src/dispatcher.py Dispatcher.complete \
  --ref 01a0b58f:1 text,role --agent
```

**Refusal (engr, exit 4):** `§4: relations are human-authoritative, so an
agent-admitted section does not carry them`.

**This is the sharpest thing I hit all session.** Doctrine **C2** *requires* an
implementation relation on any assertion about specific code — its stated FAIL
case is "a section asserting how dispatch behaves, with an excerpt of the
dispatch function and no relation". engr *forbids* an agent from carrying one.
So for any assertion C2 considers to be about specific code, the Agent Rule
Review path is structurally unreachable: satisfying the rule makes the command
illegal, and making the command legal fails the rule. The only route is a Human
candidate. That is consistent with the record I inherited — `01a0b596` §2 is the
one section carrying relations and it is the one section admitted `by human`.

I resolved it the way the previous session did: the **decision** goes in by the
agent path with a bounded excerpt and no relations (exactly the shape of the
already-admitted `01a0b58f` §1, §2 and §3, all agent-admitted, all excerpt, none
with relations), and the **acceptance criterion**, which is the assertion most
squarely about specific code, goes up as a Human candidate carrying the
relations. Re-ran without the two relation flags; engr surfaced digest
`1:ff7b542a...` and rendered the whole object, which is what makes C11 checkable
by a reviewer who has never seen the record.

**Delegated review, attempt 1: FAILED on C2 (prose-purity).** 11 of 12 items
passed, including C1 on "the time for which anything about it is kept beyond
that is **zero**". The one failure:

> "C2 — FAIL. §4 asserts specific runtime behavior ... and backs it only with
> the bounded excerpt `content [0] code.lifetime` plus the basis field. Neither
> the rendered Section nor the originating command carries an `implemented_by`
> file relation or a symbol relation. This is the doctrine's own FAIL pattern
> verbatim: 'a section asserting how dispatch behaves, with an excerpt of the
> dispatch function and no relation.'"

An independent reviewer reached the deadlock on its own, which confirms it is
real and not my misreading. Attempt 2 reframes the assertion as a property of
what pacing *keeps* rather than a description of what a function *does*, which
is honestly what the decision is and is also the wording that bears on the
restart question in the backlog.

**Delegated review, attempt 2: FAILED on C5 (mss-ssot) and C2 (prose-purity).**
The reframe fixed nothing on C2 and broke C5. The C5 critique is exactly right
and I would not have seen it myself:

> "Beyond '*the time for which anything about a tenant is kept ... is zero*,' it
> also separately asserts a computation-input claim ... and a
> restart-recoverability claim ... A reader could accept the zero-retention rule
> while disputing either of these."

C2 again:

> "The assertion is specifically about what the shown code does ... with no
> `implemented_by` file or symbol relation anywhere on the screen."

**I stopped the agent path here, at 2 of 3 attempts, and did not spend the
third.** There is no honest revision that satisfies C2: C2 is satisfied only by
an implementation relation, and engr refuses to let an agent-admitted section
carry one. A third attempt would be theatre, and worse than useless — failing it
would exhaust `prose-purity`, whose policy is `reject`, and SKILL.md then forbids
raising the same change as a Human candidate. Stopping at two keeps the one path
that can actually comply with the doctrine open.

So §4 goes up as a **Human candidate** carrying the relations C2 demands, with
the attempt-1 wording (which passed every item except C2, including the C5 that
attempt 2 broke). See §7.

**Two things the checklist made me do that I would not have done otherwise:**
split the restart consequence out of the lifetime assertion (C5), and put the
literal number "zero" in the prose rather than leaving it as "no interval" (C1).
Both are improvements.

## 5. Backlog — a new point for what I could not fix

The oscillation gap from §1 above genuinely cannot be settled here, so it went to
staging rather than through the gate.

```
engr backlog new --title "Whether the pool is divided at an instant or over a stretch" \
  --text-file bl-gap.txt \
  --subject-file src/dispatcher.py --subject-symbol src/dispatcher.py Dispatcher
```

**Refusal 1 (engr):** governed by both rules, digest surfaced, nothing written.
Worth noting: unlike `prepare --agent`, **`backlog new` renders no screen at all**
— just the digest. SKILL.md's instruction is to hand the reviewer "that screen",
and here there is none, so I reconstructed the mutation field by field from the
command (title, point text verbatim, both subjects) and said in the packet that
that is what it was. That is a gap in the delegation practice for the backlog
domain.

**Delegated review, attempt 1: FAILED on C6.**

> "*'Nobody has weighed the two against each other here.'* This is narration
> about the state of the team's analysis ... the opening clause '...is undecided'
> already establishes that this is unresolved."

Mechanically right, and I would never have caught it: **both backlog points the
previous session staged end with exactly that construction** ("and no one has
weighed those against each other here", "and nobody has judged whether that is
an acceptable cost or a defect"). The doctrine's revision into a checklist made
the house style of the existing staging a C6 failure. I deleted the sentence.

**Delegated review, attempt 2: PASSED, 12/12.** Landed:

```
engr backlog new --title "..." --text-file bl-gap.txt \
  --subject-file src/dispatcher.py --subject-symbol src/dispatcher.py Dispatcher \
  --review 1:68d0eb7c... --reviewed-rule mss-ssot --reviewed-rule prose-purity --attempt 2
```

→ new topic **`01a0b7f5`** (`engr:backlog:01m2vzb0eff5sbfjn76d845swg`), 1 point,
subjects pinned at `c9b87c22`.

One thing the reviewer flagged that is a real inconsistency in the project's own
policy, not in my mutation: `mss-ssot` tells a reviewer to check **C7** on a
backlog point, but Part C of the doctrine lists the backlog items as "C1, C4, C5,
C6, C8, C9 and C10 only" — C7 is not among them. The rule and the document it
rests on disagree. It did not change the outcome.

## 6. Backlog — the two points the previous session left

### §2, "count of lost turns or elapsed time" — left unresolved, deliberately

Nothing I did this session weighed a count of lost turns against an elapsed-time
bound. The record already *chose* count-based release (`01a0b596` §1), and the
previous session correctly recorded that as `produced` without consuming the
point — the choice was made, the weighing the point asks for was not. That is
still true. **Consuming it would be claiming an analysis nobody has done.** Left
in place, untouched.

### §1, "must pacing survive a restart" — left unresolved, and the revise abandoned

I judged this the point my work bears on, and tried to sharpen it rather than
consume it, because the wording that would settle it is **not in the record** —
it is a Human candidate awaiting a code (§7). A point is settled when the
question is answered, and here the answer is sitting outside the record, so
consuming would destroy an unresolved point on the strength of something nobody
has admitted.

Three delegated review attempts on the revise, all failed:

- **attempt 1 — FAIL C11 (mss-ssot).** My wording said *"Everything pacing reads
  in order to divide the pool is a reading of the work outstanding at the moment
  of the division"*, and the reviewer caught that this **forecloses the sibling
  point §2**: a cumulative count of lost turns "could not exist if pacing reads
  only the work outstanding at the moment of the division". Genuinely sharp, and
  I would not have seen it — I was looking at §1 alone.
- **attempt 2 — FAIL C10 (prose-purity).** Dodging C11 I wrote *"whatever each
  piece of it carries about how it has been treated so far"*, and got told:
  "precisely the doctrine's warning that wording emptied out to avoid naming
  anything fails C10". Also right. The two items pulled in opposite directions
  and I fell off the other side.
- (the attempt numbering above is 1 and 2 of the revise's own sequence; engr saw
  them as attempts 1 and 2, and the ceiling is 3)

**I abandoned the revise rather than spend the third attempt or push it through
past the ceiling.** In the backlog domain an ordinary edit still lands past the
ceiling, marked `review_exhaustion` — and SKILL.md is explicit that the marker is
"not somewhere to put work you could not get past a rule". The previous
session's wording of §1 is serviceable; my sharpening was an improvement, not a
necessity. Nothing was written to §1.

**Neither point was consumed. Nothing was produced against either**, because the
only outcome my work generates is not yet admitted.

## 7. The deadlock, proved from all three sides, and the pending candidate

This is the finding of the session. **For a Section that asserts something about
specific code and rests on an agent-admitted Section, the paceq doctrine cannot
be satisfied.** Three independent reviewers and two engr refusals, all agreeing:

| what I tried | result |
| --- | --- |
| Agent, `--ref 01a0b58f:1`, no relations | doctrine **C2 FAIL** — twice, by two different reviewers |
| Agent, `--ref`, **with** relations | engr, exit 4: `relations are human-authoritative, so an agent-admitted section does not carry them` |
| Human, relations, **with** `--ref 01a0b58f:1` | engr, exit 5: `a human section may reference only human-admitted authority; 01a0b58f §1 is agent` |
| Human, relations, no `--ref` | doctrine **C3 FAIL** — "§4 relies on that assertion without a selective reference onto it. The command contains no `--ref` flag at all" (prose-purity passed all five items) |

C2 wants an implementation relation; an agent may not carry one. C3 wants a
selective reference; a human section may not carry one onto agent authority. The
existing §1, §2 and §3 are all agent-admitted, so every new section that builds
on them inherits the trap. `01a0b596` §2 escapes it only because it references
nothing.

Also worth recording: **a plain `engr prepare` — the Human path — is governed by
these rules too.** I had assumed from SKILL.md that Rule Review was the agent
mechanism; it is not. A Human candidate is refused with just a digest until a
review outcome accompanies it, and it renders no screen until then, so the
review packet has to be reconstructed by hand exactly as for backlog.

### Exhaustion, honestly counted, and the candidate

Three attempts on this one section, one continuous sequence (agent v1, agent v2,
human-with-relations). Ceiling 3. I reported it at attempt 4:

```
engr prepare --object 01a0b58f --add --text-file sec-lifetime.txt \
  --header "Lifetime of a tenant" --role decision \
  --content-file code.lifetime excerpt-lifetime.txt \
  --implemented-by-file src/dispatcher.py \
  --implemented-by-symbol src/dispatcher.py Dispatcher.complete \
  --implemented-at HEAD --based-on HEAD \
  --review 1:da7f50c5... --reviewed-rule mss-ssot --reviewed-rule prose-purity \
  --review-attempt 4 --review-result exhausted --review-explanation "<the table above, in prose>"
```

`mss-ssot`'s `on_exhaustion = human_confirmation` took it, and `prose-purity`
(policy `reject`) did not block because it **passed**. engr minted:

> **CONFIRM EHU2BP** — `section.create` on `01a0b58f`, header "Lifetime of a
> tenant", role decision, both implementation relations, based on `c9b87c22`,
> marked `Review EXHAUSTED at attempt 4 — confirming this admits work no
> passing review allowed`.

**I did not confirm it and will not.** It is reported to the human as the
handoff. Re-render with `engr candidate EHU2BP`; do not re-run `prepare`.

## 8. Work and collection

- `engr work unblock 01a0b596 --index 0` + item 2 to `done` with the real result
  and commit `e726d655`, and a new summary — the sidecar had been claiming a
  candidate was pending that a human had already answered at `e726d65`.
- `engr work start 01a0b58f ...` with three items, a dependency on the new
  backlog topic, and a blocker naming EHU2BP. Item 2 records the follow-up a
  future session must do: **once a person answers EHU2BP, the restart point in
  `01a0b599` becomes consumable.**
- `engr collection add pacing-f --target engr:backlog:01m2vzb0eff5sbfjn76d845swg
  --order 40 --priority low` — the new point belongs to this plan.
- No collection was deleted, no member repointed, nothing was paused or
  unpaused. Neither object was classified; both are still untyped and open,
  which is human-only and which the sidecars now say.

## 9. Verification and commits

```
python3 -m unittest discover -s tests -t .   → Ran 16 tests, OK
engr verify                                  → both objects PASS, exit 0
engr ls --verify                             → the same five `basis moved` rows as
                                               at the start (docs only; see §0)
```

Two commits on `main`, nothing pushed:

- `c9b87c2` dispatch: state the pacing-state lifetime invariant and pin it with tests
- `63bf83f` engr: stage the division-window question, refresh work and plan

`git ls-files .engr | grep local` is empty, so the pending challenge code did not
go into shared history.

## 10. Refusals engr gave me, collected

1. `error: this mutation is governed by mss-ssot, prose-purity; ... repeat it with
   review digest ...` — on `prepare --new --agent`, `prepare --add --agent`,
   `prepare --add` (Human), `backlog new`, `backlog revise`. Every one of them.
2. `error: attempt 3 is exhausted against a ceiling of 3, which it has not passed`
   — `exhausted` is reported one past the ceiling.
3. `error: an Agent mutation is admitted only by a passing Rule Review` —
   exhaustion buys an agent nothing.
4. `error: §4: relations are human-authoritative, so an agent-admitted section
   does not carry them` (exit 4).
5. `error: a human section may reference only human-admitted authority;
   01a0b58f-578c-7581-a702-e06d414d4253 §1 is agent` (exit 5).

## 11. What was ambiguous or annoying

- **No screen for most governed mutations.** `prepare --add --agent` renders the
  whole object beautifully. `backlog new`, `backlog revise` and Human `prepare`
  render nothing at all — just the digest. SKILL.md's central instruction is
  "hand over the screen, not the prose", and for three of the five mutation kinds
  I ran there is no screen to hand over. I reconstructed each one field by field
  and said so in the packet, but that is exactly the retyped summary SKILL.md
  warns against.
- **Nothing tells a reviewer which Part of the doctrine applies.** Three
  reviewers, three different answers for a title create (Part C, Part D, Part
  A/B). The doctrine has four parts keyed to mutation kind and the rendered
  screen does not state the kind. Adding the command fixed it.
- **C9 is unanswerable on an Object create.** Its test asks which field carries
  each code-related thing; on a create there are no other fields, so "the title"
  is the only possible answer and the item can always be failed.
- **`mss-ssot` names C7 for backlog points; Part C does not list C7 for backlog
  points.** The rule and the document it rests on disagree.
- **C1 vs C10 vs C11 pull against each other.** I lost two attempts falling off
  opposite sides: abstracting to avoid contradicting a sibling point (C11) landed
  me in "emptied out to avoid naming anything" (C10).
- **`--attempt` vs `--review-attempt`.** `prepare` takes `--review-attempt`,
  backlog takes `--attempt`, and both accept the other. Fine, but the ceiling is
  per continuous sequence and engr stores none of it, so the honest number is
  entirely on me — and in this session being honest about it is what closed the
  autonomous path twice.

## 12. What I could not do

- **Record the lifetime decision.** It is Human candidate `EHU2BP`, unanswered.
- **Fix the oscillation gap** (a momentarily-idle tenant losing the pool). The
  fix is a positive retention window, which contradicts an existing admitted
  Section and the existing test that pins it. Staged as `01a0b7f5` instead.
- **Consume either backlog point.** Neither is settled by anything in the record.
- **Classify either object.** Human-only, and there is already one pending code.

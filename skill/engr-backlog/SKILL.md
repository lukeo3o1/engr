---
name: engr-backlog
description: >-
  Use before any `engr backlog` command in a repository that has adopted
  `.engr/`. Staging a question you will not settle now as a short point that
  says what is undecided, why, and what would settle it; reading what is
  unresolved; recording what a point produced; and consuming it only once it is
  settled. Read the engr skill first.
license: MIT
metadata:
  origin: https://github.com/lukeo3o1/engr
---

# engr-backlog

What is not decided yet. When something belongs here rather than in the record
is in the `engr` skill; this one is how.

## Work that is not settled yet

Do not force an undecided question through the gate, and do not carry it in your
head across sessions. It goes in the backlog, which needs no confirmation:

```bash
engr backlog ls                          # what is still unresolved
engr backlog show <id>                   # points, subjects, outcomes so far
engr backlog new --title "..." --text "the unresolved point"
engr backlog add <id> --text "another point in the same topic"
engr backlog revise <id> --section 2 --text "sharpened"
engr backlog merge <id> --into 2 --section 5 --text "one point after all"
engr backlog produced <id> --section 2 --target engr:obj:<id>:3
engr backlog consume <id> --section 2     # consuming it is what says "settled"
```

**A point is one question, stated so it can be settled.** Three things, a
sentence each:

- **what is undecided** — one question per point;
- **why it is not obvious** — what pulls against what;
- **what would settle it** — the evidence, the measurement, or the person who
  decides.

```text
Whether the deferral bound counts lost turns or elapsed time.
A count is exact under steady load; only time also covers a stalled dispatcher.
Settled by which of those two failures the service owner will accept.
```

Nothing else: not who has looked at it, not what was tried, not what comes
next. The third sentence is the one most often missing and the one that
matters most — a point that does not say what would settle it cannot be worked
on, and cannot honestly be consumed, because nobody can tell whether it has
been. engr limits a topic's title and not a point's text, so keeping it short is
yours to do. If it needs more than three sentences it is usually two points, or
part of it is already settled and belongs in the record.

**The third sentence names what would decide it, never the decision.** "Settled
by building the smaller slice first" is not a test anyone can apply; it is a
choice already made, staged where no review reaches it and where the next
reader cannot tell whether it is decided — a cold reader handed two such points
summarised them as settled. If you chose, the choice goes in the record, saying
it holds for this slice if that is all it holds for, and the backlog keeps only
what is still open about it.

`merge` names one destination and one source. It keeps the destination and
removes the source; it never mints a third section, so anything already pointing
at `--into` still points at it. What the source produced comes along. Two points
to fold in is two merges, because each consumption is its own judgement against
its own predecessor.

Before every existing-state backlog mutation, read before you write and say
what you read. This stale-write protection is independent of whether a Rule
governs the mutation:

```bash
engr backlog show <id> --format json    # each point carries an `expect` value
```

Pass it back as `--expect <token>` on the mutation — once per point, so a merge
passes two: the destination and its source. If it no longer matches, the point
moved between your reading it and your changing it, and you get told to read it
again rather than landing a change on wording you never saw. Creation is the
sole exception, because engr allocates the new identity atomically and there is
no predecessor to supply.

**When a project rule governs backlog, these take the same two steps an Object
mutation does.** Run the intended command; engr writes nothing and tells you
which Rules govern it and the ReviewDigest of that exact change. Read every one
of them and what they rest on, review the change, then repeat the command with
the digest and the complete rule set:

```bash
engr backlog revise <id> --section 2 --text "sharpened" --expect <token>
# refused: governed by <rule-id>, ... and the digest of this subject

engr rules show <rule-id>

engr backlog revise <id> --section 2 --text "sharpened" --expect <token> \
  --review <digest> --reviewed-rule <rule-id>
```

Repeat `--reviewed-rule` for the whole surfaced set. engr recomputes the digest
under the writer lock, so if the point, the topic it sits under, or any Rule
material moved in between, read and review again rather than copying the new
digest. `backlog new` takes the same two steps: engr mints the topic id while
performing the create, so it is not part of what you reviewed and nothing but
the digest travels between the two calls.

Where no backlog Rule applies there is nothing to attest, and passing `--review`
is refused rather than ignored.

Delegating the review is worth it here too, and it is a judgement call rather
than the rule it is for the record. Staging is meant to be cheap, and a
subagent per parked thought is not cheap — but a point that will not pass its
own policy is a point somebody has to come back and fix, so the saving is often
borrowed rather than made. Delegate when the rule sets checkable requirements
on the wording; review it yourself when it does not.

For a `revise` or a `merge`, the screen to hand over is the whole topic —
`engr backlog show <id>` — not the point alone. A point is revised against its
siblings whether you look at them or not, and one sharpened in isolation has
ruled out the question sitting next to it.

Every one of these takes `--attempt <n>` when a project rule governs backlog
(`--review-attempt` is accepted too, since that is what `prepare` calls it) —
which try of your own review this is, counted from 1, and 1 if you say nothing.
Past the ceiling, an ordinary edit still goes in and is marked `review_exhaustion`, so
say the real number: the point is kept either way, and an honest one tells the
next reader what it went in on. **Consume, merge and rename are the
exceptions**, and past the ceiling they simply do not happen. Consume and merge
remove a point; rename has nowhere to put the marker, and an exhausted change
that leaves no trace is the one thing the marker exists to prevent. Revise it,
or raise the ceiling.

Every screen says `UNCONFIRMED STAGING`, and it means it. **Never reason from a
backlog section as though it were the record**, and never quote one to a human
without saying where it came from. If a point has become something you can
assert, admit it through the appropriate Human or Agent path — reviewed record
wording is usually not the wording you staged.

**Admitting the Object does not touch the point it came from.** They are two
separate operations, and the second is yours to remember: once the record has
the outcome, either record it against the point with `backlog produced`, or
consume the point if it is settled. engr will not infer the link, because an
inferred one would eventually consume something nobody meant to resolve.

Recording an outcome does **not** resolve anything — a point can produce several
outcomes across sessions and still have work left. Only consuming says settled.

Two rules keep it honest:

- **A section that is still there is still unresolved**, whatever it has already
  produced. `produced` lists confirmed outcomes that came out of working on it;
  it is progress, not a verdict. Read it when you resume so you do not re-solve
  what an earlier session already got confirmed.
- **Removing a section is the act of judging it resolved.** There is no status
  to set, so do not remove one you have not actually settled.

Give `--subject-file <path>` or `--subject-symbol <path> <name>` when the point
concerns specific source. engr pins the commit and refuses to pin HEAD while
that path is dirty — commit it first, or pass `--subject-commit <rev>`. Use
`--subject engr:obj:<id>:<section>` for a record section the point concerns;
that is context, not a dependency, and it does not make the record depend on
anything unconfirmed.

A point somebody will decide from, or one a Rule governs, is worth a cold read
before it goes in — *Writing for a reader who was not there*, in `engr-object`.

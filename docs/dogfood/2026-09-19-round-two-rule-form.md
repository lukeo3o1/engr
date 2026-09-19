# Round two: the same requirements, written as a checklist

Round one ([2026-09-18](2026-09-18-complex-dogfood.md)) ended with a narrow
finding: a Rule reliably stops what it can spell out, and reliably fails to
deliver what it can only describe. Its prohibition list — no identifiers, paths,
flags, source literals, excerpts — was obeyed perfectly. Its purpose clauses —
state the decided quantity in words, vagueness fails as hard as padding, point at
source through the relation fields rather than copying it — were missed by ten
consecutive reviews.

Round two tested the obvious hypothesis: **was that about the requirements, or
about how they were written?**

## What changed, and what did not

The doctrine was rewritten from prose into a numbered checklist, C1–C12, with the
requirements held constant. Only the form moved:

- the **positive** requirements (what must be *present*) were pulled out of the
  narrative and put first, as C1–C4, ahead of the prohibitions;
- every item got an operational test — C1: "cover every other field; reading the
  prose alone, can you say what the number is?"; C10: "read the prose alone and
  state what a reader is supposed to do; if you cannot, this FAILS, no matter how
  clean it is";
- the counter-direction clauses became their own numbered items instead of
  sitting at the end of a list of restrictive ones;
- the document states how a reviewer must use it: work the items in order, in
  writing, PASS or FAIL each, quoting the words that decide it;
- each Rule file names the item numbers it owns.

The work continued **on the same record**: the round-one objects, sections,
backlog topic, sidecar and plan were all still there, and the round-two agent had
to read and build on them rather than start clean.

## What happened

| | Round one (prose doctrine) | Round two (checklist doctrine) |
| --- | --- | --- |
| Sections admitted | 4 | **0** |
| Violations found by independent audit | 6 across those 4 | — (nothing was admitted) |
| Backlog points admitted | 2 | 1, after a 12/12 passing review |
| Delegated reviews that failed the author | 3 of 10 | 5 rounds, failing on C1, C2, C3, C4, C5, C6, C9, C10, C11, C12 |
| Reached the human gate | 1, review passed | 1, review **exhausted** |

Nothing non-conforming got in, because nothing got in. That is a real difference
and it is not a clean win: the checklist blocked good work along with bad, and
the reason it blocked the good work is the next section.

## The finding that matters: an authority deadlock

A Section that both asserts something about specific code **and** builds on an
agent-admitted Section cannot be written at all. Reproduced independently, in a
clean workspace, against an agent-admitted §1:

```
$ engr prepare --object <id> --add … --implemented-by-file <path> --agent
error: §2: relations are human-authoritative, so an agent-admitted section
       does not carry them

$ engr prepare --object <id> --add … --implemented-by-file <path> --ref <id>:1 text,role
error: a human section may reference only human-admitted authority;
       <id> §1 is agent
```

The doctrine's C2 requires the relation for a code assertion; only a human
section may carry one. C3 requires the reference for a dependency; a human
section may reference only human authority. Every section admitted in round one
was agent-admitted, so everything built on top of them inherits the deadlock. The
round-two agent hit it four times, with three independent reviewers and two engr
refusals, before the shape was visible.

Round one's builder reported the same thing as its first criticism, from the
other end, and was told by neither refusal about the other half. What the
checklist added was the form: not "this section never feels finishable" but a
flat contradiction between two numbered requirements, each enforced by a
different half of engr's authority model.

The pending candidate is the consequence. It is marked
`Review EXHAUSTED at attempt 4`, and it fails C3 — the item that cannot be
satisfied for that section. A human is being asked to override a rule the tool
makes unsatisfiable, which is a different thing from being asked to override a
judgement.

## What the checklist bought

From the round-two builder, corroborated by the failures in its log:

1. **Failures arrived citable.** Prose review produces "this feels
   underspecified"; the checklist produced "C5, and here are the two extra
   propositions, quoted". No negotiation about whether a finding was real — only
   about the fix. The builder accepted verdicts it disagreed with for this
   reason.
2. **Real defects the author could not see.** C5 caught three assertions bundled
   as one ("I had bundled three things and read them as one because I meant them
   as one"). C11 caught a backlog revision that would have foreclosed its own
   sibling point — the reviewer was looking at the topic while the author looked
   at the point. C1 forced the literal word "zero" in place of "no interval".
3. **Internal conflicts stopped being absorbable.** A prose reviewer trades C1
   against C10 against C11 silently and calls the result reasonable. Numbered
   items cannot trade: the builder dodged C11 and landed in C10 on consecutive
   attempts, and both were reported rather than split.

## What the checklist cost, and this part is worse

The independent audit of round two found that the one item which *passed* —
the backlog point, with all twelve answered PASS — **was not correctly
reviewed**. Not because the point is bad; on the merits the auditor found nothing
that should have failed. Because **five of the twelve answers were answers to
items the doctrine withdraws for a backlog point**. Twelve answers were produced
because the document demanded twelve.

That is the checklist's own failure mode, and it is the mirror of round one's. A
prose reviewer risks passing what they meant rather than what is there. A
checklist reviewer risks producing the required number of answers rather than the
true ones — and the result looks, on the page, like a perfect score.

The audit also found four defects in the checklist itself, all of them mine,
written deliberately with round one's lessons in hand:

- **Part C contradicts itself in consecutive sentences.** "Backlog points are
  checked against C1, C4, C5, C6, C8, C9 and C10 only" — and then, one paragraph
  later, "C2 applies as the subject association".
- **The answer count contradicts the Rules.** The front matter says a review that
  does not produce twelve answers is not a review of this doctrine; the two Rule
  files name five items and seven items respectively, and Part C cuts the backlog
  set smaller still.
- **C7 is omitted from Part C's list and never disapplied**, so whether it
  applies to a backlog point has no answer.
- **C8 read literally fails every mutation in the project**, already-admitted
  sections included: it forbids field names "in any spelling or casing", the
  domain's central noun is also a field name in the source, and C9's remedy
  ("name which field now carries it") has no answer for a domain noun.

Three verdicts in the audit are ones the auditor states it does not believe, each
forced by one of those defects.

## Where this leaves the question

Rule form is real and it is not the whole story.

- **Positive requirements have to be numbered items with a test.** C2 and C3 —
  the two requirements that produced *zero* compliance in round one — produced
  citable failures within one review in round two. That is the hypothesis
  confirmed.
- **A checklist converts diffuse doubt into citable findings**, which is what
  makes a delegated reviewer's verdict stick instead of being argued away.
- **A checklist cannot resolve a conflict between its own items**, and it makes
  scope decisive: three reviewers given the identical screen routed the same
  title create to three different Parts and returned three different verdicts.
  The variance was entirely in routing, and engr's screen does not say which kind
  of mutation produced it — one factual line naming the command would collapse
  it.
- **A required answer count is a target**, and it was met at the expense of being
  right.
- And the strongest result is the one nobody was testing for: **a mechanical
  checklist surfaced a contradiction between the policy and engr's authority
  model that a prose reading had absorbed for a whole round as "this section
  never feels finishable".**

The two failure modes are not symmetrical in cost, though. Round one admitted
four sections with six violations. Round two admitted none, and every block it
produced was either correct or a symptom of a real defect in the tool or in the
policy. If the choice is between a policy that passes quietly and one that jams
loudly, the jam is worth more — provided somebody reads why it jammed.

## Evidence

- `appendix-d-round2-field-log.md` — the round-two builder's log, written as it
  worked.
- `appendix-e-round2-audit.md` — the independent audit of round two, including
  the undecidable items.
- `appendix-f-doctrine-v2-checklist.md` — the checklist doctrine as used, with
  the four defects above still in it.

# Audit of the paceq engineering record

Auditor: independent. Standard: `docs/RECORDING-DOCTRINE.md` at `ed1f535f`, and the
two rules `mss-ssot` and `prose-purity` that rest on it. Nothing else was consulted
except `src/`, `tests/` and `git log`, and those only to verify SSoT claims.

Scope audited: both admitted Objects and their four Sections, both Object titles, the
pending candidate `X6HVXH`, both Backlog points, the Work sidecar, the Collection.

A note on scope that matters for every verdict below: both rule files declare
`applies.domains: [object, backlog]`. **The Work sidecar and the Collection are not
governed by either rule.** They are audited here anyway, and marked *(ungoverned)*.

---

## 1. Verdict table

| # | Item | prose-purity | MSS | SSoT |
|---|------|----|----|----|
| 1 | Object `01a0b58f` **title** | PASS | PASS | MARGINAL |
| 2 | `01a0b58f` §1 *Share basis* | PASS | MARGINAL | **FAIL** |
| 3 | `01a0b58f` §2 *Place in line* | PASS | PASS | **FAIL** |
| 4 | `01a0b58f` §3 *Pool bound* | PASS | PASS | MARGINAL |
| 5 | Object `01a0b596` **title** | PASS | PASS | MARGINAL |
| 6 | `01a0b596` §1 *Deferral escape* | **FAIL** | **FAIL** | **FAIL** |
| 7 | Candidate `X6HVXH` *Escape is exclusive* | **FAIL** | **FAIL** | **FAIL** |
| 8 | Backlog `01a0b599` §1 | PASS | MARGINAL | PASS |
| 9 | Backlog `01a0b599` §2 | PASS | MARGINAL | PASS |
| 10 | Work sidecar `01a0b596` *(ungoverned)* | FAIL if applied | FAIL if applied | FAIL if applied |
| 11 | Collection `pacing-fairness` *(ungoverned)* | MARGINAL if applied | MARGINAL if applied | PASS |

### Evidence, item by item

**1. Object `01a0b58f` title** — "A tenant's pacing must reflect all work outstanding,
not only work already running."
- prose-purity: clean. No identifier, path, flag, literal or excerpt. The word "pacing"
  is domain English, not a symbol in the source. PASS.
- MSS: one assertion, a complete sentence. PASS.
- SSoT **MARGINAL**: this is the same fact §1 of the same object asserts — §1: *"A
  tenant's permitted share of the worker pool is derived from every tenant that has work
  outstanding, whether that work is already running or still waiting"*. Doctrine §2.2:
  *"A fact that lives in another Section is depended on, not repeated."* A title is a
  name, not a Section, so I do not call this a violation; but the record does now carry
  the same claim in two places with no reference between them, and a later revision of §1
  would silently leave the title untrue.

**2. `01a0b58f` §1 *Share basis*** — decision, `based_on cad2c405`.
- prose-purity PASS. The prose says "the worker pool" and "share" where the source says
  `capacity`, `demand_tenants()` and `share()`; no identifier, path or literal appears.
  The default pool size (`capacity=4`) is correctly absent.
- MSS **MARGINAL**, in the *padded* direction. Two separately disputable reasons are
  welded onto one assertion: *"hands the whole pool to whichever tenant arrives first"*
  **and** *"lets a tenant that has finished everything go on diluting what the others are
  entitled to."* A reader can agree with the second and reject the first. Both are also
  restatements of the pre-existing defect, which §1.2 forbids (*"Do not restate the
  problem"*), in tension with §1.5's demand for a reason. I verified both reasons are
  *true* of the code at `a3d0376` (`_share()` used `len(self.running)`, and `complete()`
  decremented without popping, so a finished tenant stayed in the divisor) — so this is a
  length/scope finding, not a truth finding.
- SSoT **FAIL**. The section carries **no implementation relation of any kind** — no
  `implemented_by` file, no symbol. Instead the whole implementation is copied verbatim
  into the excerpt: *"def demand_tenants(self): ... def share(self): divisor = max(1,
  len(self.demand_tenants())) return max(1, self.capacity // divisor)"*. That is two
  complete function bodies, not *"a bounded literal excerpt"* (§3) — it is a second copy
  of the source inside the record, which is exactly the staleness §2.1 exists to prevent,
  and the field §2.1 names as the right one (*"the Section's own implementation
  association fields"*) is empty. Because nothing is pinned, `engr ls --verify` reports
  `all ok` while verifying nothing about the source.

**3. `01a0b58f` §2 *Place in line*** — decision, `based_on 06f86d5f`, `refs 01a0b58f §1
(role, text)`.
- prose-purity PASS. "keeps the place in line it already held" carries the fact that the
  source spells `sequence`, without naming it. Genuinely well done.
- MSS PASS. One assertion, reason present, complete sentence, actionable.
- SSoT **FAIL**, on the excerpt. The `code.putback` excerpt splices two different files
  into a single untagged blob: `def push(self, job): ... self._push(job)` is from
  `src/queue.py`, the block beginning *"deferred = [] / chosen = None / while True:"* is
  from `Dispatcher.dispatch` in `src/dispatcher.py`. With no implementation file or symbol
  relation on the section, a reader cannot tell which file either half came from. Worse,
  the excerpt drags in the source docstring *"Put an already-sequenced job back without
  reallocating its place."* — a verbatim second statement of the section's own assertion,
  from source, inside the record. The `--ref` to §1 pinning `role, text` is adequate: §2's
  phrase "passed over for pacing" does rest on §1's definition of the share, and `text` is
  the field that carries it.

**4. `01a0b58f` §3 *Pool bound*** — acceptance_criterion, `based_on 06f86d5f`.
- prose-purity PASS. "the size of the worker pool" in place of `self.capacity`; no literal.
- MSS PASS. One assertion, testable, reason present, complete sentence.
- SSoT **MARGINAL**. The excerpt *"if self.in_flight() >= self.capacity: return None"* is
  genuinely bounded and is the kind of thing §3 permits. But again there is no
  implementation relation, so nothing pins `Dispatcher.dispatch` and the copy is
  load-bearing rather than illustrative. No contradiction with §1, §2 or `01a0b596` §1:
  the capacity guard runs before the share check in `dispatch()`, so the escape cannot
  breach this bound.

**5. Object `01a0b596` title** — "A job deferred too often is released anyway, because a
share nobody can reach is not a guarantee."
- prose-purity PASS. "deferred" is ordinary English here; that it is also the name of a
  local list in `dispatch()` is coincidence, not an identifier reference.
- MSS PASS. One assertion, complete sentence, reason present.
- SSoT **MARGINAL**, same shape as item 1: it restates its own §1 with no reference.

**6. `01a0b596` §1 *Deferral escape*** — decision, `based_on f500bad3`, `refs 01a0b58f §1`
and `01a0b58f §3`.
- prose-purity **FAIL**, against the last paragraph of doctrine §3: *"A quantity that is
  part of the decision rather than part of the implementation — a budget a human agreed
  to, a bound the decision is about — is stated in prose in words."* The deferral ceiling
  is precisely *a bound the decision is about* — the decision **is** "how many misses
  before release". The prose says *"a set number of times"* and states no number. The
  number was instead pushed into the excerpt, which opens with the source literal
  *"DEFAULT_DEFERRAL_CEILING = 3"*. That is the laundering case the rule names in reverse:
  material that belonged in prose was relegated to a field, and the field it landed in
  carries it as an identifier plus a literal rather than as a decided quantity in words.
- MSS **FAIL** on §1.5 **sufficiency**, in the **vague** direction. *"a set number of
  times"* — set by whom, to what? A reader who was not present cannot act on this without
  asking someone or opening the excerpt and reading `= 3` out of the source. §1.5 is
  explicit that this fails *"as surely as a padded one"*.
- SSoT **FAIL**. No implementation relation; the excerpt again carries the whole
  implementation (`DEFAULT_DEFERRAL_CEILING` spliced with the full body of `_eligible`,
  two non-contiguous regions of `src/dispatcher.py` presented as one block). The two
  `--ref`s are the one unambiguously *correct* piece of SSoT work in the whole record:
  *"even though its tenant is already at its share"* depends on `01a0b58f §1` (`text`
  pinned), and the escape's non-breach of the pool bound depends on `01a0b58f §3` (`text`
  pinned). Both pin the fields the wording actually relies on. Credit where due.

**7. Candidate `X6HVXH` *Escape is exclusive*** — acceptance_criterion, pending human
confirmation, `based_on 0914d27d`.
- Relation check, done for real: `git show 0914d27d:src/dispatcher.py` contains
  `class Dispatcher` and `def _eligible` at line 45. Both `implemented_by` targets
  (`src/dispatcher.py`, `src/dispatcher.py :: Dispatcher._eligible`) exist at the pinned
  revision, and `_eligible` genuinely implements the assertion: the only branch that
  returns `True` above the share test is the pass-count exemption, so "and for no other
  reason" is true of that symbol. **This sub-check PASSES.** It is also the only object
  mutation in the record that carries implementation relations at all.
- prose-purity **FAIL**, same clause as item 6: *"has already lost its turn **the agreed
  number of times**"*. The bound the criterion is about is stated nowhere in prose — and
  this candidate carries **no excerpt content**, so if it is confirmed the number will be
  nowhere in the record whatsoever, recoverable only by opening the source.
- MSS **FAIL** on §1.5. As an *acceptance criterion* this is unusable: nobody can test
  "the agreed number of times". The reason clause is present and good; the operative
  quantity is not.
- SSoT **FAIL**. The candidate has **no `refs` at all**, yet its first clause — *"A tenant
  may have more jobs running at once than its share only when the additional job has
  already lost its turn the agreed number of times"* — restates what `01a0b596` §1 already
  asserts (*"is released at its next opportunity even though its tenant is already at its
  share"*). Doctrine §2.2 requires that be depended on through a selective reference. Only
  the words *"and for no other reason"* are new. Additionally it silently duplicates the
  vague quantity, so the same unstated number now sits in two sections with nothing
  linking them.

**8. Backlog `01a0b599` §1** — subjects: `src/dispatcher.py @1df52160`,
`src/dispatcher.py :: Dispatcher @1df52160`. Verified: the file and `class Dispatcher`
both exist at `1df5216`.
- prose-purity PASS. The path and symbol are in the `concerns` fields, where they belong;
  the prose says "a restart of the service" and names nothing.
- MSS **MARGINAL**. Redundant tail: the point opens *"is undecided"* and closes *"and
  nobody has judged whether that is an acceptable cost or a defect."* The same fact twice
  in one sentence — the padded direction, mild.
- SSoT PASS. Backlog is exempt from §2.2 by doctrine §4, and nothing here restates source.

**9. Backlog `01a0b599` §2** — subject `src/dispatcher.py @1df52160`, verified; `produced`
points at `obj:01m2tscy7geyhvphw448wkp0zm:1`, which exists and is admitted.
- prose-purity PASS.
- MSS **MARGINAL**, same redundancy: *"is undecided, because ... and no one has weighed
  those against each other here."*
- SSoT PASS. This point questions the very basis that `01a0b596` §1 decided (count of
  lost turns vs. elapsed time) while that section is live — but doctrine §4 expressly
  permits a backlog point to *"restate what a Section asserts in order to question it"*,
  and §2.4's bar on disagreement is between *Sections*. No violation. The tool surfaces
  the tension honestly: *"(already admitted; this point is still unresolved)"*.
  Housekeeping note, not a rule finding: the backlog *title* covers §1 only, while §2 asks
  an unrelated question.

**10. Work sidecar `01a0b596` — ungoverned.** Neither rule applies to the `work` domain.
Held to them anyway it fails all three: prose-purity — *"Twelve tests pass"* is a literal
count taken from the suite (12 test functions in `tests/test_fairness.py`, so it is true
today and stale on the next test added), and *"Candidate X6HVXH is pending"* puts a
challenge code in prose; MSS — *"Escape implemented, tested, and its decision admitted."*
is a verbless fragment against §1.4, and the whole sidecar is narration, which §1.2
forbids; SSoT — the test count duplicates a fact that lives in the suite. The tool itself
brands the artifact *"EXECUTION MEMORY — agent-managed, admitted by nobody"*, which is a
fair defence of every one of those.

**11. Collection `pacing-fairness` — ungoverned.** Description: *"Fix the share
calculation and queue-order defect, then settle what happens to a job that keeps losing
its turn."* Held to the rules: *"share calculation"* leans on the name of `share()`
(marginal at worst — it reads as domain English), and the sentence carries two actions
joined by "then", which §1.1 would split. Member notes are clean prose. SSoT PASS: the
three members are `engr` references, not restatements. Nothing here misstates the record.

---

## 2. Did the rules bind?

Every mutation in the record was admitted by `agent` with review metadata
`{"result": "passed"}` attached — the two `object.created.v1` events took two attempts,
all four `section.created.v1` events passed on the first. The candidate carries
`"review": {"attempts": 1, "result": "passed", "rules": ["mss-ssot", "prose-purity"]}`.
So every finding below is a finding that *survived an attestation that these exact rules
passed*.

**Catchable by a review reading the same material — the mechanism missed them:**

- Items 6 and 7, the two `prose-purity` FAILs. The last paragraph of doctrine §3 is four
  lines long and says a bound the decision is about is *stated in prose in words*. A
  review reading the section text *"a set number of times"* next to its own excerpt
  `DEFAULT_DEFERRAL_CEILING = 3` had everything needed to catch it. This is the most
  telling miss because the rule text itself warns about moving material into a field
  where it does not belong — the review applied that warning in one direction (out of
  prose) and never in the other.
- Items 6 and 7, the MSS §1.5 sufficiency FAILs. Same words, same page, purely textual.
- Item 7's missing `refs`. The candidate restates `01a0b596` §1 — the section immediately
  above it on the same object — and carries no reference. A review that read the object
  it was being added to would have seen it.
- Item 2's doubled reason and items 8 and 9's redundant tails. Textual, mild, catchable.
- Items 2, 3, 6: the missing implementation relations. Fully visible in the mutation — the
  `relations` array is simply absent. That the *candidate* has them proves the field was
  available and understood all along.

**Could the mechanism ever have caught them?** Only one class of check is genuinely
outside a text review: whether a relation target *exists at the pinned revision* and
*actually implements the assertion*. I verified those by hand (`git show
0914d27d:src/dispatcher.py`) and they hold. Everything else I found is a reading of text
against text. There is no finding here that a diligent review of the same material could
not have made — which means the mechanism did not fail for want of information. It failed
while reporting that it had passed.

Two structural gaps are worth naming separately, because they are not review failures at
all:

- The rules govern `object` and `backlog` only. The Work sidecar — the single most
  prose-impure artifact in the tree, containing a literal count, a fragment and pure
  narration — is outside their reach by construction. Nothing was ever going to catch it.
- The backlog file carries **no review metadata whatsoever** (`.engr/backlog/*.json` has
  no `admitted` block and there is no `.engr/eventstore/backlog/`), although both rules
  declare `backlog` a governed domain. The two backlog points pass on their merits, but I
  cannot verify that any review ran on them.

## 3. Did the rules backfire?

Yes, in three concrete places.

1. **A fact that is now nowhere.** The deferral ceiling — three — is the entire content of
   the escape decision. `01a0b596` §1 renders it *"a set number of times"* and pushes the
   number into an excerpt; candidate `X6HVXH` renders it *"the agreed number of times"*
   and carries **no excerpt at all**, only relations. If that candidate is confirmed as
   written, the record will contain an acceptance criterion whose acceptance threshold
   appears nowhere in the record. A reader must open `src/dispatcher.py` to learn what
   the record decided. Prose purity, pursued past the doctrine's own carve-out, deleted
   the decision from the decision.
2. **A section a reader cannot act on.** The candidate is typed
   `acceptance_criterion`. An acceptance criterion that cannot be evaluated without asking
   someone is not a criterion. This is exactly the failure §1.5 anticipates, produced by
   over-compliance with §3.
3. **Relations carrying load the prose should have carried — and, where relations are
   absent, excerpts carrying it instead.** Not one admitted section has an
   `implemented_by` relation. The load fell on verbatim excerpts, and the excerpts grew to
   fill it: two whole functions in §1, a two-file splice in §2, a constant plus a method
   in `01a0b596` §1. The record ended up with *more* raw source copied into it, not less,
   and none of it pinned to a symbol that `engr ls --verify` could check. `all ok` is
   reported against an empty set.

A fourth, smaller cost: the prose has become abstract enough that §1's two reasons had to
be spelled out at length to stay decidable — purity pushed toward padding, and MSS §1.1
paid for it.

Against that, the rules also clearly *worked* in places, and an honest audit should say
so. §2 *Place in line* states a sequencing guarantee without ever naming `sequence`, and
reads perfectly. §3 *Pool bound* states a capacity invariant without naming `capacity` or
the default `4`, and is testable as written. `01a0b596` §1's two `--ref`s pin exactly the
fields its wording leans on. The backlog points put their path and symbol in `concerns`
where they belong. No governed prose anywhere in this record contains a forbidden token —
zero identifiers, zero paths, zero flags, zero commands, zero code fences. On the letter
of prose-purity's prohibition list, compliance is complete.

## 4. Verdict

A rule of this kind, enforced this way, is highly effective at the thing that is easy to
check and largely ineffective at the thing that matters. Its prohibition list — names,
paths, flags, literals, excerpts — is a lexical test, and the record passes it perfectly:
I could not find a single forbidden token in any governed prose, which is a real
achievement and not an accident. But the doctrine's *purpose* clauses, the ones that
require judgement — §3's carve-out that a bound the decision is about must be stated in
prose in words, §1.5's insistence that vagueness fails as hard as padding, §2.1's
requirement that a source fact be carried by an implementation relation rather than
copied — were missed uniformly, and missed after an attestation that a review of these
exact rules passed. The predictable shape of the failure is the tell: prose that is
scrupulously clean and no longer says what was decided, next to excerpt blobs that copy
whole functions into the record because the relation fields nobody filled in were supposed
to carry them. The record has the appearance of compliance in exactly the dimension that
is mechanically checkable, and the substance of non-compliance in exactly the dimension
that required someone to think — and because the checker and the author are the same
agent, self-admitting with a passing review attached, there was no point at which anything
independent looked at the result. The one mutation routed to a human, the candidate, is
also the worst item in the record on two of three rules; that it is the piece held back
for a person to confirm is the mechanism's only real save.

# A complex dogfood of engr, and what it says about project Rules

Build under test: `engr latest (eb7cd06e)`, built from this checkout.

Three agents were used, deliberately separated so that no one of them could see
what the others knew:

- a **builder**, cold on engr, given only `skill/SKILL.md` and a real project,
  told to do real engineering and record it;
- a **probe**, adversarial, in a throwaway copy of the same workspace, told to
  break the mechanisms and to judge the binary rather than the source;
- an **auditor**, independent, given only the project rules, the doctrine they
  rest on, and the record that came out — and explicitly denied the builder's
  field log, so it could not learn what anyone intended.

The claims below are the agents'. The ones marked **[verified]** were reproduced
independently afterwards, from the commands recorded here; a claim without that
marker is one agent's report and is labelled as such.

## The setup

A real project, `paceq` — an in-process job queue with per-tenant pacing —
carrying a genuine fairness defect, a design question the code could not settle,
and something honestly unresolved. Its recording policy lives in
`docs/RECORDING-DOCTRINE.md`, and two Rules rest on that file:

- **`prose-purity`** (`object`, `backlog`; 3 attempts; `reject`). Section wording
  carries no code-related information: no identifiers, paths, file names,
  commands, flags, environment variables, literals taken from source, or
  verbatim excerpts. Each of those has a dedicated field on the same mutation —
  bounded excerpt content, the implementation file relation, the implementation
  symbol relation, the basis — and that field is the only place it may appear.
  Material "fixed" by moving it into a header, a title, or an excerpt it does
  not belong in has not been cleaned; it has been moved.
- **`mss-ssot`** (`object`, `backlog`; 3 attempts; `human_confirmation`). MSS:
  one assertion per Section, no narration, no anticipation, complete sentences,
  and — the clause that turns out to matter — *minimal never means partial*, the
  reason that makes the assertion decidable must be present. SSoT: a fact that
  lives in the source is pointed at, not copied; a fact that lives in another
  Section is depended on through a selective reference, not repeated; two live
  Sections may not contradict each other.

The doctrine's §3 carries one carve-out that the rest of this document turns on:
a quantity that is part of the *decision* rather than part of the
implementation — a budget a human agreed to, a bound the decision is about — is
stated in prose, in words.

## What the builder produced

A real fix (share derived from demand rather than from tenants already running;
a passed-over job re-inserted with its original place rather than resubmitted;
head-of-line blocking removed; total in-flight hard-capped), 12 tests, and a
design decision the code did not settle: a job that has lost its turn a bounded
number of times becomes exempt from its tenant's share. **[verified]** the tests
pass and the tree is clean at `c5f30b1`.

The record: two Objects with four admitted Sections, all agent-admitted through
Rule Review; a backlog topic with two genuinely unresolved points; a work
sidecar; a collection; and one mutation held at the Human gate, still pending.
Nine of ten reviews were delegated to fresh agents that had never seen the
draft. **Three of those failed on first attempt, and the builder reports its own
self-read had passed all three.** That is the delegation practice earning its
keep, exactly as `skill/SKILL.md` argues it does.

## The mechanisms hold

Everything the probe attacked in the binding between a review and a mutation was
*prevented*, not merely discouraged: an invented digest, a digest borrowed from
another mutation, a digest reused after a one-character edit to the draft, a
replay of a consumed digest, one rule id omitted from the surfaced set, a
non-governing rule added, `--review-attempt 0`, and `passed` claimed past the
ceiling. Recomputation under the lock is real — a whitespace-only edit to a rule
file, and an edit to the doctrine file a rule rests on, each invalidated a
digest issued seconds earlier.

Also confirmed holding: `--oversize` is genuinely retry-only; backlog `--expect`
stale-write protection; the `review_exhaustion` marker; consume and merge
requiring a review that actually passed; the work-sidecar refusal when resolving
a backlog point's last section; `tampered` versus `divergent` distinguished on
every surface, with ordinary mutations refused and `repair` showing field by
field what it discards; drift that is field-selective, blind to `.engr`'s own
commits, and whose handed `git show` recovers the exact prior wording; the gate
refusing wrong, lowercase and whitespace-damaged codes and *discarding* the
candidate on a qualified yes; and a Rule with a missing or moved basis reported
UNUSABLE with the governed mutation refused rather than treated as ungoverned.

## What the mechanisms do not reach

The record that came out of a scrupulous builder, under two Rules, with nine
delegated reviews, was then audited by someone who had never seen it.

**The lexical half of the policy was obeyed perfectly.** The auditor could not
find a single forbidden token — identifier, path, flag, command, source literal
or excerpt — in any governed prose, anywhere in the record. That is a real
result and not an accident.

**The half that required judgement was missed uniformly**, and missed after an
attestation that a review of these exact rules passed:

- **The bound the decision is about was pushed out of the prose**, in the
  *reverse* direction from the one the rule was written to stop. The escape
  decision says a job is released after losing its turn "a set number of times";
  the number survives only inside the excerpt. The pending candidate says "the
  agreed number of times" and carries no excerpt at all. Doctrine §3's carve-out
  says that quantity belongs in the prose, in words. Two FAILs, both of them
  prose that is immaculately clean and no longer says what was decided.
- **MSS §1.5 failed in the vague direction on the same two items** — an
  acceptance criterion a reader cannot evaluate.
- **Not one admitted Section carries an implementation relation.**
  **[verified]**: all four sections have `relations = []`, and every one instead
  carries a code excerpt of 357, 517, 70 and 194 bytes — whole functions, and in
  one case a docstring that restates the section's own assertion verbatim. The
  load the relation fields were meant to carry fell on excerpts, so the record
  ended up with *more* raw source copied into it, not less. `engr ls --verify`
  reports `all ok` against an empty set of relations.

The shape of that failure is the finding. A rule whose prohibitions are lexical
gets lexical compliance; its purpose clauses — state the decided bound in words,
vagueness fails as hard as padding, point at source instead of copying it — got
none. And the author and the attester were the same agent, so nothing
independent looked at the result until this audit, after admission.

The one mutation routed to a human is also the worst item in the record on two
of the three rules. That it is the piece a person still has to answer is the
mechanism's only real save here.

## Defects found in the tool

1. **The documented escalation route dead-ends.** **[verified]** Repeating a
   governed `--agent` mutation with `--review-result exhausted` and
   `--review-explanation` — the exact instruction in `skill/SKILL.md` — is
   refused with `an Agent mutation is admitted only by a passing Rule Review`.
   Escalation happens only if `--agent` is dropped and the attestation is
   re-made against the human-path digest, which can only be learned by running
   the command without `--agent` and reading the refusal. `engr protocol` says
   an exhausted `human_confirmation` rule MUST escalate to the Human Gate. The
   refusal names neither the gate nor the flag to drop. The cost is specific:
   an agent following the guide literally concludes the path is closed, and the
   nearest way out is to restart the attempt count at 1 — the one input the
   whole ceiling depends on.
2. **`engr show --format json` omits the stored `header`.** **[verified]** The
   header is in the object file and in the text `show`, and absent from the
   structured form that `skill/SKILL.md` calls "the same, structured". That is
   the form an agent is most likely to hand a delegated reviewer, and the header
   is inside the seal — so the JSON view makes invisible precisely the
   header-laundering failure the skill warns about and `prose-purity` judges.
3. **The mutations that most need reviewing are the ones engr will not render.**
   `prepare --agent` prints the full candidate so a reviewer gets the screen
   rather than a retyping, and the skill spends a page on why that matters. A
   governed *backlog* mutation prints one line of error and no screen; a plain
   Human `prepare` prints no review screen either, and cannot be previewed with
   `--agent` where relations are involved, because that path refuses on
   relations first. Three of the builder's ten reviews were therefore done on a
   hand-reconstruction — and one of those produced a *wrong* FAIL, the reviewer
   flagging two ordinary English words as suspected identifiers because it had
   nothing to check them against. The builder complied and made the wording
   vaguer. Defects 2 and 3 are the same disease: review quality is bounded by
   the completeness of the screen handed over, and several paths hand over
   nothing or hand over less than the seal contains.
4. **One damaged EventStore stream aborts `engr ls --verify` and `engr verify`
   entirely** — exit 4, a single file-path error, healthy objects never
   reported. That is the command the skill says to run first, precisely so that
   a survey is not cut short.
5. **A deletion that orphans another object's section is reviewed blind.**
   Neither the review screen nor the admission output mentions the dependent; it
   surfaces later as `REF MISSING`, possibly on an object nobody is looking at.
6. **`skill/SKILL.md` documents none of the agent authority matrix, and
   contradicts part of it.** An agent cannot reword or delete a human-admitted
   section, cannot carry `--type`/`--state` on a section action, and cannot use
   `--oversize`, `--close`, `--reopen`, `--classify`, `--supersede` or `repair`.
   The binary enforces all of it correctly; the skill recommends the combined
   revise-and-reclassify form that the agent path refuses. Related: the
   passed-review candidate note advises "`--agent` writes it now" on actions
   where `--agent` is refused.
7. **Title mutations are governed by this build**, while the skill says creating
   or renaming a title is the one agent operation allowed without a rule.
   **[verified]**: a `prepare --new --title` was refused as governed by
   `mss-ssot`. So a title consumes attempts against the same 3-attempt ceiling —
   and engr separately refuses a long title with "keep it to a short label",
   while a rule demanding a complete sentence with its reason pulls the other
   way. Nothing in the tool or the guide acknowledges the two pulling apart.
8. **An exhausted `reject` rule is not named in its refusal** — the message is
   identical to an ordinary failed attestation, so "the path is over" and "you
   mis-attested" are indistinguishable.
9. **Attesting `failed` first demands `--review-explanation`, then refuses
   anyway**, which invites a manufactured override rationale for a review known
   to have failed.
10. **`basis moved` hands you no `git show` command**, only a count, though the
    skill tells you to run the command `show` hands you for both drift
    markings. `refs moved` does hand one, and it works.
11. Smaller: the review flags differ per domain (`--review-attempt` and a
    mandatory `--review-result` on `prepare`; `--attempt` and a *forbidden*
    `--review-result` on `backlog`), and only `prepare` prints a copy-pasteable
    template — pre-filled with `passed`, which is a nudge toward attesting
    before reviewing. A backlog merge missing its second `--expect` is reported
    as a stale read. `collection ls` prints a truncated id that is not the id
    you created. The two unusable-rule faults exit 3 and 4 for the same class of
    problem.

## One design tension, not a defect

The durable record does **not** name which Rules governed an agent admission.
**[verified]**: the admitted event's metadata is
`{"attempts": 1, "outcome": "passed", "result": "passed"}`, and neither a rule id
nor the ReviewDigest appears anywhere in `.engr/objects`, `.engr/eventstore` or
`.engr/backlog`. The binary is conforming: `protocol/PROTOCOL.md` states that the
digest MUST NOT be persisted into history, with a sound argument — it binds exact
material at review time and becomes uncheckable once the rules move.

The tension is with what the mechanism is sold as. The README says what it buys
is "that the material was named precisely enough for somebody to check
afterwards", and the protocol justifies omitting per-rule ids from the backlog
exhaustion marker on the grounds that "the attestation that admitted the mutation
already named the complete applicable set". For a Human admission that holds
while the Challenge is pending. For an **Agent** admission — the path with no
human in it, and the one that produced every section in this record — the
complete applicable set is named on a screen that is then discarded, and rule
files can be edited freely afterwards with no gate at all. A later reader can
see that *a* review passed, and cannot discover what it was a review against.
Worth a recorded decision either way.

## What this says about Rules, as a mechanism

Strip out the tool defects and the honest answer is narrow, and worth having.

The binding half is genuinely effective: engr makes it impossible, through the
supported path, to admit a governed semantic mutation without an attestation
bound to that exact mutation and that exact rule material, and every attempt to
forge or stretch that binding failed. Nothing in this dogfood got past it.

The *judgement* half is a practice, and it performed exactly as `skill/SKILL.md`
predicts and no better. Delegating the review caught three failures the author's
own reading had passed — that is the practice working. But the delegation is only
as good as the screen handed over, and where the screen was reconstructed by hand
the review produced a wrong result; where a purpose clause required weighing
rather than pattern-matching, ten reviews in a row missed it. The predictable
residue is a record that is lexically spotless and substantively hollowed out:
prose that names nothing, excerpts that copy everything, relation fields left
empty, and a decided bound that exists nowhere in words.

So: a Rule of this kind reliably stops the thing it can spell out, and reliably
fails to deliver the thing it can only describe — unless something independent
reads the result *after* admission. engr has no such surface today. The two
cheapest moves toward one, on this evidence, are to make every governed mutation
render its full screen (defects 2 and 3), and to let a reader see, later, what
a passed review was a review against.

## Evidence

- `appendix-a-mechanism-probe.md` — the adversarial probe: every experiment with
  its verbatim commands and output, and a HOLDS / GAP / BUG verdict each.
- `appendix-b-builder-field-log.md` — the builder's field log, written as it
  worked: every refusal, every failed delegated review quoted, and every place a
  rule cost it something.
- `appendix-c-independent-audit.md` — the audit of the resulting record against
  the two rules, item by item, with the evidence quoted.

The sandbox projects themselves are not committed: they were disposable, and
the appendices carry the commands needed to rebuild them.

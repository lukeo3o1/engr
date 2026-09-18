# paceq / engr dogfood — builder field log

Appended as I go. Commands and engr output quoted verbatim where it matters.

## 2026-09-18 — orientation

Read `/home/user/engr/skill/SKILL.md` in full first, then README.md,
docs/RECORDING-DOCTRINE.md, src/*.py, `engr rules ls`, `engr rules show mss-ssot`,
`engr rules show prose-purity`.

Startup survey per SKILL.md "Reading the record":

```
$ engr ls --verify
no objects            (exit 0)
$ engr backlog ls
nothing unresolved
$ engr work ls
EXECUTION MEMORY — agent-managed, admitted by nobody, and not what the record says
no execution memory
$ engr collection ls
PLANNING — agent-managed, admitted by nobody, and says nothing about what its members mean
no collections
```

Empty workspace, nothing inherited. Both rules govern `backlog, object` and both
rest on the same doctrine file pinned at ed1f535f (current).

Note on the two rules' exhaustion policies, because they differ and that matters
later: mss-ssot is `3 attempts; on_exhaustion = human_confirmation`, prose-purity
is `3 attempts; on_exhaustion = reject`. So the effective ceiling on any object
wording is 3 tries before the autonomous path ends.

## The defect, as I read it

Three distinct faults in the pre-fix release path, all in `Dispatcher.dispatch`
and `Dispatcher._share` (src/dispatcher.py at 3dc30b8):

1. `_share()` divides capacity by `len(self.running)`. An idle pool has
   `running == {}`, so the divisor is clamped to 1 and the share is the whole
   capacity — the first tenant to arrive is measured against nobody. Queued
   demand is invisible to the share calculation.
2. `self.running` is never pruned: `complete()` decrements to 0 but leaves the
   key, so a tenant that finished everything keeps diluting everyone else's
   share afterwards. The same function is wrong in both directions.
3. A job that loses its turn is put back with `self.queue.submit(...)`, which
   allocates a fresh `sequence` from the counter. Ties in priority are broken by
   sequence, so the job goes to the back of its priority class and a new Job
   object is created — the caller's handle is orphaned.
4. (found while fixing) `dispatch()` returns `None` the moment the *head* job is
   over share, even when a releasable job for another tenant is right behind it.
   Head-of-line blocking; the pool idles.
5. (found while fixing) nothing bounds total in-flight jobs. With more tenants
   than capacity, `max(1, capacity // n)` floors at 1 per tenant and the sum
   exceeds capacity.

Fix: share is computed from *demand* (running or waiting); `running` is pruned;
passed-over jobs are pushed back as the same object with the same sequence;
dispatch scans past blocked jobs; total in-flight is hard-capped.

`python3 -m unittest discover -s tests -t .` — 12 tests, OK.

Note on test honesty: my first starvation test used a `locals().get("running")`
hack and passed vacuously — the "starving" job had nothing running for its
tenant, so under the new share rule it was never actually passed over. Rewrote
it around the real starvation shape (a tenant blocked behind its *own*
long-lived jobs). Second draft of the below-ceiling test also passed for the
wrong reason. Both are the same class of error the SKILL warns about for review:
I read what I meant, not what was there.

## engr, first mutation: creating the object

SKILL.md says "Creating or renaming a title is the only Agent operation allowed
without an applicable Rule, because the title is navigation metadata." That is
not what the build does:

```
$ engr prepare --new --title "Tenant share and place in line" --agent
open  Tenant share and place in line

NEEDS REVIEW  governed by mss-ssot, prose-purity. Read those Rules and everything they
              rest on, review what is above — all of it, not only the
              wording — then repeat this command with

                  --review 1:b8d1426221b74f8234f5dc10a53eec11fa097bb5a43498204c7928d1ea101a62 --reviewed-rule <RULE> --review-result passed

              Nothing has been written.
error: this mutation is governed by mss-ssot, prose-purity; review the surfaced Rules, then repeat it with review digest ... and the review outcome
exit=2
```

**Surprise #1 / made me re-read the guide.** The refusal is actionable — it names
the rules, the digest and the flags — but it contradicts the guide's stated
carve-out for titles. I went with the tool (the guide says the protocol is right
about what the tool does). Cost: the ceiling of 3 review attempts now applies to
a title, which is the cheapest possible mutation.

**Refusal message quality:** good. The `--review ... --reviewed-rule <RULE>
--review-result passed` template is literally copy-pasteable, except that it
prints the flag pre-filled with `passed`, which is a nudge toward attesting a
pass before the review has happened. I did not use it that way.

### Delegated review 1, attempt 1 — FAILED

There is no in-process subagent tool in this session, so I delegated by running
the `claude` CLI headless (`claude -p < packet`) with a packet containing: the
exact engr screen, `engr rules show` for both rules verbatim, and the whole of
docs/RECORDING-DOCTRINE.md. Nothing about my intent, no draft history.

Reviewer's finding, verbatim:

> FAIL
>
> The mutation fails **MSS 1.4** (complete sentences) and **MSS 1.5**
> (sufficient).
>
> "Tenant share and place in line" is a noun phrase, not a complete sentence.
> ... It also fails sufficiency: a reader cannot tell what open question or
> unresolved work this represents. ... the wording does not state what needs to
> be resolved, so a reader cannot act on it without asking someone—the exact
> condition 1.5 names as a failure.

**My own self-read had passed it** — I wrote a noun-phrase title without a
second thought, because every title I have ever written is a noun phrase. The
reviewer is partly confused (it reasons about the title as if it were a backlog
point) but 1.4 and 1.5 applied to a title is a defensible reading, and per
SKILL.md the subagent's answer is what I attest. Counting this as attempt 1,
failed. Rewriting the title as a complete sentence.

**Tension worth recording:** prose-purity bans identifiers "in any spelling or
casing". In this project the domain nouns *are* the identifiers — tenant, job,
queue, dispatch, share, capacity are all both. Taken literally the rule makes it
impossible to write a sentence about a job queue. I am reading it as banning
reference to code entities, not ordinary domain English, and I am deliberately
letting the delegated reviewer be the one to decide that rather than deciding it
for myself.

### Delegated review 1, attempt 2 — PASSED

Title rewritten to a complete sentence: "A tenant's pacing must reflect all work
outstanding, not only work already running." Reviewer passed it, and explicitly
addressed the identifier tension I was worried about:

> "Tenant," "pacing," and "work" are domain concepts described in prose, not
> code symbols.

Admitted: `ADMITTED   01a0b58f  object.created.v1  rev 1  agent`.
Canonical reference `engr:obj:01m2trynwcep0te0q0dn0mtgjk`.

Both delegated reviewers so far have described the mutation as "a new backlog
item" even though the screen says `engr prepare --new` and shows an object. The
screen engr prints does not say which domain the mutation is in. That is a real
gap: mss-ssot's clause about SSoT 2 *not* applying in the backlog domain means a
reviewer who guesses the domain wrong applies a different rule set. I did not
correct them, because correcting a reviewer is exactly what SKILL.md tells me not
to do, but a domain line on the screen would have removed the guess.

## engr refused: relations are human-only

```
$ engr prepare --object 01a0b58f --add --text-file s1.txt --header "Share basis" \
    --role decision --content-file code.share share_excerpt.txt \
    --implemented-by-file src/dispatcher.py \
    --implemented-by-symbol src/dispatcher.py Dispatcher.share --agent
error: §1: relations are human-authoritative, so an agent-admitted section does not carry them
exit=4
```

**This is the sharpest thing I hit.** The refusal is clear and actionable, but
the policy it enforces collides head-on with the project doctrine. Doctrine §3
says a file that implements the assertion *belongs in* the implementation file
association and that association is "the only place it may appear". An agent
working the Agent Rule Review path cannot use that field at all. So an agent can
never record where an assertion is implemented — the one field the project's own
prose-purity rule points at. The only legal agent-side home for code-related
material is the excerpt content, which is a copy rather than a pointer, and
copies are precisely what doctrine §2 calls a second copy that goes stale
silently.

I worked around it by carrying a bounded excerpt and nothing else, and by saving
the implementation associations for the one mutation I am putting through the
Human gate. That is a defensible split, but it was forced on me by the tool, not
chosen.

**Temptation logged:** with `--implemented-by-file` unavailable I was briefly
tempted to write "see the dispatcher module" into the prose of §1. That would
have been a path/identifier in the wording and a straight prose-purity failure.
The rule cost me the pointer entirely on agent-admitted sections; what I kept is
an excerpt whose provenance is not stated anywhere in the record.

## Object 01a0b58f — sections admitted

- §1 [decision] "Share basis" — share derived from tenants with outstanding work;
  excerpt `code.share`; based_on cad2c405. Delegated review PASSED, attempt 1.
- §2 [decision] "Place in line" — a passed-over job keeps its position; excerpt
  `code.putback`; `--ref 01a0b58f:1 text,role` so it depends on §1's decision
  rather than restating that passing-over exists. Reviewer named the reference as
  the reason it is not a duplicate. PASSED, attempt 1.
- §3 [acceptance_criterion] "Pool bound" — total in-flight never exceeds the
  pool. Deliberately worded so it stays true under the starvation exemption I am
  about to record; if I had written the obvious "no tenant exceeds its share"
  criterion, the exemption would have made it a live contradiction, which
  doctrine 2.4 calls a failure rather than a variant.

## Object 01a0b596 — the starvation policy

### Delegated review, attempt 1 — FAILED

Proposed title: "Bounded unfairness is preferable to unbounded waiting when a
job repeatedly loses its turn." Reviewer:

> FAIL
>
> **MSS requirement 5 (Sufficient):** ... This states a preference and the
> scenario in which it applies, but omits the reason that makes the preference
> decidable. Why is bounded unfairness preferable? ... The phrase "when a job
> repeatedly loses its turn" describes the context, not the justification.

**My self-read had passed this too.** I read the "when ..." clause as carrying
the justification because I knew the argument behind it. It does not; it is a
scope clause. That is the second time the delegated reviewer caught a
mechanical MSS requirement I had convinced myself was satisfied, and both times
the miss was of the same shape — I supplied the reason from my own head.

### engr refused: title length, which cuts against the rule

```
$ engr prepare --new --title "A job that repeatedly loses its turn is eventually released anyway, because a pacing rule that can defer a job forever is a liveness fault rather than a fairness policy." --agent
error: --new --text is the object's title, not its body (169 characters, limit 120). Keep it to a short label and put the detail in a section with --add.
```

**Surprise #2, and a real conflict.** engr tells me a title is "a short label";
the project rule, as a fresh reviewer reads it, requires a complete sentence
carrying the reason that makes the assertion decidable. Those pull in opposite
directions, and 120 characters is not much room for both. I got under it with
"A job deferred too often is released anyway, because a share nobody can reach
is not a guarantee." (97 chars), which passed on attempt 2 — but the fit was
tight, and a project whose doctrine demands reasons in every recorded string
will keep hitting this ceiling. SKILL.md's line that titles are "navigation
metadata" not needing a rule would have resolved the conflict; the build does
not implement that line.

The refusal message itself was excellent: it gave the actual count, the limit,
and the correct alternative action in one line.

## Backlog — engr renders no screen for a governed backlog mutation

```
$ engr backlog new --title "..." --text-file b1.txt --subject-file src/dispatcher.py \
    --subject-symbol src/dispatcher.py Dispatcher
error: this mutation is governed by mss-ssot, prose-purity; read the surfaced Rules and what
they rest on, review this exact change, then repeat it with review digest 1:0eead4f1... and
the complete rule set
```

That is the entire output. **Surprise #3, and the one I think is a genuine
design flaw.** `prepare --agent` prints the full candidate — header, role,
content, based_on, refs — precisely so that I can hand the reviewer the screen
rather than my description of it. SKILL.md spends a whole paragraph on why that
matters and tells the story of an agent that retyped the wording and got a
worthless review. In the backlog domain there *is* no screen: the first governed
attempt prints one line of error. So the only thing I can hand a reviewer is my
own retyped reconstruction of the fields — exactly the practice the guide names
as the failure mode. I reconstructed it as faithfully as I could (every field
labelled, the text piped straight from the file, a note saying the tool rendered
no preview), but the guarantee is gone.

The refusal is also missing the `--reviewed-rule`/`--attempt` template that
`prepare` prints, so I had to go back to SKILL.md for the flag spellings, and
they differ between domains: `prepare` uses `--review-attempt` and requires
`--review-result`, `backlog` uses `--attempt` and *forbids* `--review-result`
("a review that did not pass is not repeated with a verdict"). Both spellings
are accepted in both places, which is kind, but the asymmetry in
`--review-result` is not obvious until you read `--help`.

### Delegated review of the restart point, attempt 1 — FAILED

> FAIL
>
> **prose-purity violation (Section 3):** ... "accrued deferral" — appears to be
> an identifier or implementation-specific term (likely a field or state in the
> Dispatcher class); "pool" — appears to be an identifier (likely a
> variable/field name)

I think this reviewer is wrong on the merits: neither "accrued deferral" nor
"pool" is an identifier in this codebase, and "pool" is the README's own word.
But it is wrong *because it could not see the mutation* — with no rendered
screen it had no way to check a word against anything, so it treated every
domain noun as a suspected identifier. My self-read had passed the wording.
SKILL.md is unambiguous that the subagent's answer is what I attest, so I took
the FAIL, rewrote the point without "deferral" or "pool", and counted attempt 2.
It passed. The rewrite is honestly a bit worse as English, and I would not have
made it if the reviewer had been given a real screen.

**Cost of the rule, recorded plainly:** prose-purity plus a blind reviewer
pushes wording toward vaguer nouns. That is the opposite of MSS 1.5.

Created: `engr:backlog:01m2tsjc2zed0vza773qw5gnps` (01a0b599), two unresolved
points — restart persistence (§1), and whether the deferral bound should be
counted in lost turns or in elapsed time (§2, delegated review PASSED attempt 1).

The `--expect` stale-write protection worked exactly as documented: read
`backlog show --format json`, pass the `add` token back. No friction.

## Planning and execution memory

`engr collection new pacing-fairness ... --start 2026-09-18 --end 2026-10-31`,
then three `collection add` calls at orders 10/20/30 with `--reason` saying why
each matters *in this plan*. No friction. One usability nit: every `collection
add` reprints the entire collection, so adding three members printed the whole
plan four times.

`engr work start engr:obj:01m2tscy7geyhvphw448wkp0zm --summary "..."` plus
`depend`, three items, `item state/result/commit`, and a `block`. The 300/160/240
limits were never close to binding, which I take as the limits doing their job.
Same reprint-everything behaviour on every subcommand.

I did **not** set `paused` anywhere and did not delete anything.

## The Human gate — two refusals that reshaped the record

I intended the starvation policy itself to be the Human-gated change, carrying
the implementation relations and referencing the two sections of the first
object that it depends on. engr refused:

```
$ engr prepare --object 01a0b596 --add ... --implemented-by-file src/dispatcher.py \
    --implemented-by-symbol src/dispatcher.py Dispatcher._eligible \
    --ref 01a0b58f:1 text,role --ref 01a0b58f:3 text,role
error: a human section may reference only human-admitted authority; 01a0b58f-578c-7581-a702-e06d414d4253 §1 is agent
exit=5
```

**Surprise #4, and the structural one.** Put together with the earlier refusal
that agent sections cannot carry relations, the two rules partition the record:

- an **agent** section may reference other sections but may never point at code;
- a **human** section may point at code but may never reference agent authority.

So on any object whose sections an agent admitted, a later human section is cut
off from all of it and must either repeat the fact — which doctrine 2.2 calls a
duplicate — or say nothing. There is no migration path either: nothing promotes
agent authority to human authority. An agent that records a decision today has
permanently made it unreferenceable by any human section tomorrow.

Neither refusal message mentions the other half of the trade-off, so I found the
second one only by hitting it.

I restructured rather than fight it: the policy decision went in by the agent
path *with* its two references, and the Human gate went to the one assertion
that both deserves a person and needs the relations — the exclusivity criterion
that says share may be exceeded for this reason and no other. That is the
sentence that actually weakens the fairness guarantee, so it is the right thing
to make a person say yes to.

```
$ engr prepare --object 01a0b596 --add --text-file s5.txt --header "Escape is exclusive" \
    --role acceptance_criterion --implemented-by-file src/dispatcher.py \
    --implemented-by-symbol src/dispatcher.py Dispatcher._eligible --implemented-at HEAD
error: this mutation is governed by mss-ssot, prose-purity; review the surfaced Rules, then repeat it with review digest 1:ac3218... and the review outcome
exit=2
```

**Surprise #5.** SKILL.md frames Rule Review as the price of the *agent* path
("Use `--agent` for autonomous work; do not impersonate the Human path to avoid
Rule Review"). In this build a plain Human `prepare` is refused for review too.
That is arguably better policy, but it is not what the guide says, and it means
the Human path also gives you no rendered candidate until after you have
reviewed — the worst case of the missing-screen problem, because
`--implemented-by-*` cannot be previewed by the `--agent` no-write path at all
(it refuses on relations first). **The single mutation in this whole session
that most needed a faithful screen is the only one engr will not render before
review.** I hand-reconstructed every field for the reviewer and said so in the
packet. It passed, attempt 1.

After the attestation engr rendered the candidate properly and minted
`CONFIRM X6HVXH`. I am stopping there. The note it printed is the clearest
writing in the tool:

> Typing it yourself records this as human-admitted, and no later reader can
> tell that apart from a person having read it.

## A link mutation is reviewed against wording rules

```
$ engr backlog produced 01a0b599 --section 2 --target engr:obj:01m2tscy7geyhvphw448wkp0zm:1 --expect c2d2b910...
error: this mutation is governed by mss-ssot, prose-purity; ... review digest 1:dc2e1ce5... and the complete rule set
```

`produced` carries no wording at all — it records that a point produced an
outcome. Both governing rules judge wording. There is nothing for a reviewer to
read. SKILL.md's own guidance ("review it yourself when [the rule sets no
checkable requirements on the wording]") applies squarely, so I self-reviewed
this one and attested attempt 1, and I am saying so here rather than claiming a
delegation I did not do.

## Final state

```
$ python3 -m unittest discover -s tests -t .
Ran 12 tests in 0.001s
OK

$ engr ls --verify
all ok

$ engr verify
01a0b58f  PASS  3 sections  A tenant's pacing must reflect all work outstanding, not only work already running.
01a0b596  PASS  1 sections  A job deferred too often is released anyway, because a share nobody can reach is not a guarantee.

$ engr candidate
X6HVXH   section.created.v1 01a0b596 pending  2026-09-18T17:44:44Z
```

Committed on `main`: all six `.engr` directories; `.engr/local` (holding `lock`
and `challenges`) is untracked, as documented. Nothing pushed.

## Things I could not do at all

- **Record where an agent-admitted assertion is implemented.** Three of the four
  admitted sections carry a code excerpt and no pointer to the source it came
  from, because relations are human-only. The excerpt is a copy with no stated
  provenance — the exact failure doctrine 2.1 warns about — and I had no legal
  alternative.
- **Type either object.** Classification is human-only with no `--agent` path,
  and only one candidate can usefully be pending, which I spent on the criterion.
  Both objects are honestly still `untyped`/`open`, and `engr ls` correctly shows
  both as needing attention. I am not calling this recorded-and-settled.
- **Hand a delegated reviewer the real screen for three of the six reviews**
  (two backlog mutations and the Human-gated section), because engr renders no
  screen for governed backlog mutations or for a pre-review Human prepare.

## Review tally, honestly

| # | Mutation | Delegated | Attempts | Result |
|---|---|---|---|---|
| 1 | object 01a0b58f create (title) | yes | 2 | failed, then passed |
| 2 | 01a0b58f §1 Share basis | yes | 1 | passed |
| 3 | 01a0b58f §2 Place in line | yes | 1 | passed |
| 4 | 01a0b58f §3 Pool bound | yes | 1 | passed |
| 5 | object 01a0b596 create (title) | yes | 2 | failed, then passed |
| 6 | backlog 01a0b599 new | yes | 2 | failed, then passed |
| 7 | backlog 01a0b599 §2 add | yes | 1 | passed |
| 8 | 01a0b596 §1 Deferral escape | yes | 1 | passed |
| 9 | 01a0b596 §2 Escape is exclusive (Human) | yes | 1 | passed |
| 10 | backlog produced link | no (no wording to review) | 1 | passed |

Three of the ten delegated reviews failed on first attempt. **My own self-read
had passed all three.** Two of the three were MSS mechanics — a noun-phrase
title, and a scope clause I had read as a reason because I knew the reason. That
is exactly the failure mode SKILL.md predicts, and delegation is the only part
of this process that caught it.

# engr probe findings

Sandbox: `/home/user/dogfood/paceq-probe` (git repo, `engr init`-ed, two project
rules `mss-ssot` (3 attempts, `human_confirmation`) and `prose-purity`
(3 attempts, `reject`), both resting on `docs/RECORDING-DOCTRINE.md`, both
applying to domains `object` and `backlog`).

Binary judged by running it. Verbatim output below.

---

## 1. Does the review mechanism actually bind?

### 1.0 A governed mutation is refused with nothing written

```
$ engr prepare --new --title "Submission dispatch budget" --agent
open  Submission dispatch budget

NEEDS REVIEW  governed by mss-ssot, prose-purity. Read those Rules and everything they
              rest on, review what is above — all of it, not only the
              wording — then repeat this command with

                  --review 1:9ab3092263e7bf72631e7603f8d049ffa542ccc06a82edb204a5f500c878614b --reviewed-rule <RULE> --review-result passed

              Nothing has been written.
error: this mutation is governed by mss-ssot, prose-purity; review the surfaced Rules, then repeat it with review digest 1:9ab...614b and the review outcome
exit 2
```

Note: object *creation* is governed here, contradicting the SKILL.md line
"Creating or renaming a title is the only Agent operation allowed without an
applicable Rule" only if read carelessly — `engr protocol` is explicit that where
a workspace governs the Object domain, a title mutation reviews like any other.
The binary follows the protocol.

### 1.1 Six forgery attempts, all refused

```
$ engr prepare --new --title "Submission dispatch budget" --agent --review 1:000...000 --reviewed-rule mss-ssot --reviewed-rule prose-purity --review-result passed
error: this review was of something else; review the current subject and attest to 1:9ab3092263e7bf72631e7603f8d049ffa542ccc06a82edb204a5f500c878614b
exit 5

$ ... --review 1:9ab...614b --reviewed-rule mss-ssot --review-result passed          # one rule id omitted
error: the review names mss-ssot, and this mutation is governed by mss-ssot, prose-purity
exit 5

$ ... --reviewed-rule mss-ssot --reviewed-rule prose-purity --reviewed-rule no-such-rule --review-result passed
error: the review names mss-ssot, no-such-rule, prose-purity, and this mutation is governed by mss-ssot, prose-purity
exit 5

$ ... --review-attempt 0 --review-result passed
error: a review attempt is counted from 1; there is no attempt 0
exit 2

$ ... --review-attempt 4 --review-result passed                                     # ceiling is 3
error: attempt 4 is past the ceiling of 3 and can only be exhausted, not passed
exit 5

$ engr prepare --new --title "Submission dispatch budgets" --agent --review 1:9ab...614b ... --review-result passed   # title changed by one character
error: this review was of something else; review the current subject and attest to 1:67db1ed3c100a6c9214006568604e4969dd9b770c104d1195ec489df7e872e7c
exit 5
```

### 1.2 Digest reuse across mutations, and replay of a consumed digest

```
$ engr prepare --object 01a0b58a --rename --title "…v2" --agent --review <digest of the earlier add> ... --review-result passed
error: this review was of something else; review the current subject and attest to 1:bde04c5f…
exit 5

$ <the identical add, replayed with its own now-consumed digest>
error: this review was of something else; review the current subject and attest to 1:3164893137…
exit 5
```
The predecessor is inside the subject, so a digest cannot be replayed even for a
byte-identical repeat of the same command.

### 1.3 Recomputation under the lock: rule bytes and rule basis

Digest obtained for an add, then a single newline appended to
`.engr/rules/prose-purity.md` (semantically identical file):

```
$ printf '\n' >> .engr/rules/prose-purity.md
$ engr prepare --object 01a0b58a --add --text-file draft1.txt --header "Dispatch budget" --role decision --agent --review 1:3d6dea31… --reviewed-rule mss-ssot --reviewed-rule prose-purity --review-attempt 1 --review-result passed
error: this review was of something else; review the current subject and attest to 1:5950fc22c078008c74c5b446f1b4cbe3618c498e1d218640e383bd4bb92a82a1
exit 5
```

Then the *basis* of the rules, `docs/RECORDING-DOCTRINE.md`, changed (with the
section basis pinned so the dirty-source refusal did not mask it):

```
$ printf '\nAn extra line.\n' >> docs/RECORDING-DOCTRINE.md
$ engr prepare --object 01a0b58a --add --text-file draft1.txt --header "Dispatch budget" --role decision --based-on 3dc30b8 --agent --review 1:3d6dea31… --reviewed-rule mss-ssot --reviewed-rule prose-purity --review-attempt 1 --review-result passed
error: this review was of something else; review the current subject and attest to 1:e408b804142b32377f980cfb663a56f71bbc040d2c638529e16ec5cc73d2ee0e
exit 5
```

Both are recomputed, not looked up. Note the *first* attempt at that test was
refused earlier for a different reason — `error: source files have uncommitted
changes; choose a committed basis with --based-on or explicitly choose no
repository basis with --no-based-on` — i.e. dirtying the rule basis also trips
the basis gate, which is defence in depth rather than the digest check.

### 1.4 What the review flags will *not* do

```
$ … --review <correct digest> --review-attempt 1 --review-result failed
error: a review offered for override must say what is being overridden
exit 5
$ … --review-result failed --review-explanation "…"
error: an Agent mutation is admitted only by a passing Rule Review
exit 5
$ … --review-attempt 1 --review-result exhausted
error: attempt 1 is exhausted against a ceiling of 3, which it has not passed
exit 5
$ … --review <digest> --reviewed-rule … (no --review-result)
error: a Rule Review attestation needs --review-result passed|failed|exhausted
exit 2
```

**VERDICT: HOLDS** — every forgery I tried was *prevented*, not merely
discouraged. The digest binds operation, target, predecessor, result state, rule
artifact bytes and rule basis bytes; the rule-id set is checked separately; the
attempt number is validated against the ceiling.

**The one thing it does not and cannot bind is that a review happened at all.**
Every `passed` in this probe was attested by me, the author of the wording, with
no delegated reviewer. engr accepted all of them. That is documented (protocol:
"an attestation says a review happened rather than who ran it") and it is the
load-bearing gap in the whole mechanism: the tool prevents attesting to the wrong
*subject*; it only asks you not to lie about the *verdict*.

**GAP 1.4a — misleading refusal ordering for `--review-result failed`.**
Attesting `failed` is unconditionally inadmissible on the agent path, but the
first refusal you get is `a review offered for override must say what is being
overridden`, which reads as "supply an explanation and this will go through".
Only after adding `--review-explanation` do you learn it can never be admitted.
Reproduction above. It matters because it invites an agent to manufacture an
override explanation for a review it knows failed.

---

## 2. Exhaustion

### 2a. Object, under a `reject` rule

Both rules apply; `prose-purity` is `reject`.

```
$ engr prepare --object 01a0b58a --add --text "…" --agent --review <digest> --reviewed-rule mss-ssot --reviewed-rule prose-purity --review-attempt 4 --review-result exhausted --review-explanation "…"
error: an Agent mutation is admitted only by a passing Rule Review
exit 5
```
Refused, and no candidate is minted. Correct: `reject` wins over the other
rule's `human_confirmation`.

**GAP 2a — the refusal never names the rule whose policy ended the path.** The
message is the generic "an Agent mutation is admitted only by a passing Rule
Review", identical to the message for an ordinary `failed`. An agent cannot tell
from it whether it hit a `reject` policy (autonomous path over, escalate to a
human by a different route) or simply mis-attested. It matters because SKILL.md
tells the agent to behave very differently in the two cases.

### 2b. Object, under a `human_confirmation` rule

`.engr/rules/prose-purity.md` moved aside so only `mss-ssot`
(`human_confirmation`) governs.

```
$ engr prepare --object 01a0b58a --add --text "…" --agent --review 1:0e8d5706… --reviewed-rule mss-ssot --review-attempt 4 --review-result exhausted --review-explanation "Three self-reviews could not remove the implementation identifier without losing the assertion."
error: an Agent mutation is admitted only by a passing Rule Review
exit 5
```
Attempts 5 and 10 behave identically. Without `--review-explanation` the refusal
is `a review offered for override must say what is being overridden`, so the
explanation *is* required — but supplying it changes nothing on this path.

The escalation only happens when `--agent` is **dropped**, and with a *different*
digest (the human-path subject digest, which you can only learn by running the
same command without `--agent` and reading the refusal):

```
$ engr prepare --object 01a0b58a --add --text "…" --review <digest> --reviewed-rule mss-ssot --review-attempt 4 --review-result exhausted --review-explanation "…"
error: this review was of something else; review the current subject and attest to 1:ca54fb263b35b2d8d73fd6821bfae17550f0831e058d43df8f7bff954983283d

$ engr prepare --object 01a0b58a --add --text "…" --review 1:ca54fb26… --reviewed-rule mss-ssot --review-attempt 4 --review-result exhausted --review-explanation "Three self-reviews could not remove the implementation identifier without losing the assertion."
Challenge  section.create
Object     01a0b58a  Submission dispatch budget
Based on   3dc30b80
Admission  human; time assigned on confirmation
Review     EXHAUSTED at attempt 4 — confirming this admits work no passing review allowed
Digest     1:ca54fb263b35b2d8d73fd6821bfae17550f0831e058d43df8f7bff954983283d
Rules      mss-ssot
Reason     Three self-reviews could not remove the implementation identifier without losing the assertion.

The dispatcher retries failed submissions three times.

FOR A HUMAN   show them the change above and wait. They type this back:

                  CONFIRM GX35NK
exit 0
```

What the human is shown is exactly right: the action, the wording, `EXHAUSTED at
attempt 4 — confirming this admits work no passing review allowed`, the rule set
and the agent's verbatim explanation.

**BUG 2b — the documented escalation route dead-ends.** SKILL.md says: "Once the
applicable ceiling is exceeded, report `exhausted`. A Rule whose exhaustion
policy is `human_confirmation` may then produce a Human candidate only when the
attestation includes the exact explanation". `engr protocol` is normative and
stronger: "If at least one *actually exhausted* rule asks for
`human_confirmation` the mutation **escalates to the Human Gate**". Repeating the
`--agent` command with `exhausted` + explanation — the exact instruction — does
not escalate; it is refused with a message that does not mention escalation, the
human gate, or dropping `--agent`. Reproduction: the two commands above. It
matters because this is the only sanctioned way out of an exhausted review, and
an agent that follows the skill literally concludes the work is blocked and
walks away (or, worse, restarts the attempt counter at 1 to get past it).

---

## 3. The human gate

Candidate `GX35NK` from 2b, then:

```
$ engr confirm "CONFIRM ZZZZZZ"
error: no challenge awaiting ZZZZZZ
exit 3                          (candidate still pending)

$ engr confirm "confirm gx35nk"
error: the response must be exactly `CONFIRM <code>`, with nothing else on the line
exit 2                          (candidate still pending)

$ engr confirm "CONFIRM  GX35NK"          # two spaces
error: the response must be exactly `CONFIRM <code>`, with nothing else on the line
exit 2

$ engr confirm "yes"
error: the response must be exactly `CONFIRM <code>`, with nothing else on the line
exit 2

$ engr confirm "CONFIRM GX35NK but change the second line"
error: `CONFIRM GX35NK` carried commentary, so it is a qualified yes rather than assent; challenge GX35NK was discarded
exit 2                          → engr candidate: nothing is awaiting confirmation
```

A qualified yes is *destroyed*, not just refused. Lowercase and extra whitespace
are refused without destroying the candidate; only commentary discards it.

Stale code after re-running `prepare`:

```
$ engr prepare --object 01a0b58a --classify --type decision --state accepted --review … --review-result passed
                  CONFIRM 6CCWEH
$ engr prepare … (same command again)
                  CONFIRM 3QQSR2
$ engr candidate
3QQSR2   object.classified.v1 01a0b58a pending  2026-09-18T17:25:49.401439892Z
$ engr confirm "CONFIRM 6CCWEH"
error: no challenge awaiting 6CCWEH
exit 3
```

Coming back later:

```
$ engr candidate 6CCWEH
Challenge  classify
Object     01a0b58a  Submission dispatch budget
Type       decision
State      accepted
Attention  no — it leaves the default listing
Review     passed
Digest     1:8151195377d79996583bad94b946c389c71c174df1b5fb0fecc9770947fc01da
Rules      mss-ssot, prose-purity
```
Full re-render, no new code minted.

**Not a gap (attack I expected to find, properly handled).** I first recorded
this as "re-preparing silently voids the pending code". That was my error — I had
grepped for the wrong words. Re-checked verbatim:

```
$ engr prepare --object 01a0b591 --rename --title "…" --review … --review-result passed
$ engr prepare --object 01a0b591 --rename --title "…" --review … --review-result passed
              Typing it yourself records this as human-admitted, and no
              later reader can tell that apart from a person having read it.
(candidate EDJV7X was superseded by this one)
```
The tool does say which code it just invalidated.

Legitimate confirmation (I typed the code myself, under this probe's explicit
instruction to confirm one candidate — exactly the act SKILL.md forbids in real
use, and nothing in the tool stopped me or recorded that a person was never
shown it):

```
$ engr confirm "CONFIRM 3QQSR2"
CONFIRMED  01a0b58a  object.classified.v1  rev 3
note       commit .engr/objects and .engr/eventstore to preserve history and look-back
```

What admission recorded (`.engr/eventstore/objects/<id>.jsonl`):

```json
{"data":{"state":"accepted","type":"decision"},
 "metadata":{"admitted":{"at":"2026-09-18T17:25:54.785198712Z","by":"human",
   "confirmation":{"challenge":"3QQSR2"},
   "review":{"attempts":1,"outcome":"passed","result":"passed"}}},
 "rev":3,"type":"object.classified.v1"}
```
The challenge code and the review provenance are both kept; the ReviewDigest is
not persisted, as the protocol requires.

**VERDICT: HOLDS** for the gate's mechanics; the gate itself is honour-based by
design and my own confirmation demonstrates it.

**BUG 3b — `engr show --format json` omits the stored `header`.** The section was
created with `--header "Dispatch budget"`; the stored object has
`"header":"Dispatch budget"`, the human-readable `show` prints it in the section
rule, and the JSON surface does not:

```
$ engr show 01a0b58a --format json | python3 -c 'import json,sys;print(json.load(sys.stdin)["sections"][0].keys())'
dict_keys(['id','reference','role','text','status','based_on','refs','digest','admitted'])
$ cat .engr/objects/*.json | grep -o '"header":"[^"]*"'
"header":"Dispatch budget"
```
Why it matters: SKILL.md's central review instruction is "Hand over the screen,
not the prose… the header, the role, the supplementary content and the basis are
all inside the seal, and a Rule may require any of them", and it names the
structured surface as "the same, structured". An agent that hands a delegated
reviewer the JSON is reproducing the exact failure the skill spends a paragraph
warning about, and `prose-purity` explicitly fails wording "fixed" by moving
forbidden material into a header — which a JSON-fed reviewer cannot see.

---

## 2c. Exhaustion in the backlog domain

```
$ engr backlog new --title "Dispatch retry policy" --text "…"
error: this mutation is governed by mss-ssot, prose-purity; read the surfaced Rules and what they rest on, review this exact change, then repeat it with review digest 1:4d5ff801… and the complete rule set
exit 2
$ engr backlog new … --review 1:4d5ff801… --reviewed-rule mss-ssot --reviewed-rule prose-purity --attempt 1
UNCONFIRMED STAGING — nothing here is admitted to the record
01a0b58f  Dispatch retry policy
```
Backlog takes no `--review-result`, by design ("a review that did not pass is not
repeated with a verdict, it is acted on and reviewed again" — `backlog add --help`).

Stale-write protection, independent of Rules:
```
$ engr backlog add 01a0b58f --text "…"
error: this needs --expect: run `engr backlog show 01a0b58f-… --format json` and pass back the topic's `expect.add`
exit 2
$ engr backlog revise 01a0b58f --section 1 --text "again" --expect <already-used token> --attempt 1
error: what you read is not what is there now; read it again and review the current wording
exit 6
```

Past the ceiling, an ordinary point still lands and is marked:
```
$ engr backlog add 01a0b58f --text "Whether the partner tolerates duplicate deliveries is unknown." --expect cbbd630a… --review 1:dca1e235… --reviewed-rule mss-ssot --reviewed-rule prose-purity --attempt 4
added §2
note       admitted on attempt 4 against a ceiling of 3; no passing review allowed this wording
exit 0

$ engr backlog show 01a0b58f
── §2 ── unresolved
Whether the partner tolerates duplicate deliveries is unknown.
    updated  2026-09-18T17:28:04Z
    exhausted attempt 4 against a ceiling of 3; this wording stands without a passing review

$ engr backlog show 01a0b58f --format json
      "review_exhaustion": { "attempts": 4, "limit": 3 }
```

Consume, merge and rename past the ceiling do not happen:
```
$ engr backlog consume 01a0b58f --section 2 --expect d46cca11… --attempt 4
error: this is attempt 4 and a project rule allows 3: an unresolved point is not removed on an exhausted review. Revise it or raise the ceiling — it is still here either way
exit 5
$ engr backlog rename 01a0b58f --title "…" --expect 1e768499… --attempt 4
error: this is attempt 4 and a project rule allows 3: a title is not renamed on an exhausted review, because there is nowhere to record that it was: the marker belongs to a point, and this changes none of them
exit 5
```
And consuming inside the ceiling still needs a passing review:
```
$ engr backlog consume 01a0b58f --section 2 --expect d46cca11… --attempt 1
error: this mutation is governed by mss-ssot, prose-purity; … repeat it with review digest 1:fd07e14a… and the complete rule set
exit 2
```

**VERDICT: HOLDS.** Every promise in SKILL.md's backlog section is *enforced*, not
requested: the marker, the exemption for consume/merge/rename, and the review
requirement on consume. The one honour-based input remains the attempt number.

---

## 4. Integrity

### 4a. `tampered` — hand-edited section text

`.engr/objects/01a0b58a-….json` edited with `sed`-style replacement of
"once per hour" → "once per minute", nothing else touched.

```
$ engr show 01a0b58a
01a0b58a  decision/accepted  Submission dispatch budget
1 sections   0 ok   1 tampered   rev 3
!!         Object integrity failed; current authority changed outside a supported transition
!!         git show 2b0958af:.engr/objects/01a0b58a-e924-72a0-826c-23c1e097839f.json
…
── §1 [decision] Dispatch budget ── TAMPERED
Every submission is dispatched at most once per minute, …
    !!       persisted Section does not match the seal admitted at 2026-09-18T17:23:36.076077656Z
    !!       git show 2b0958af:.engr/objects/…json
error: 01a0b58a-…: its sections is not what its admitted history produced, so it was changed outside an admission path; restore it with `engr repair` before building on it
exit 5

$ engr verify
01a0b58a  FAIL  1 sections  Submission dispatch budget
          current Object integrity failed
          its sections is not what its admitted history produced; it was changed outside an admission path
          §1 content does not match its recorded hash
          uncommitted — git holds no record of the current wording yet
error: verification failed
exit 5

$ engr ls --verify
⚠ 01a0b58a  decision/accepted     §-  OBJECT TAMPERED — nobody is looking at this one
exit 0

$ engr ls
no objects                                  # accepted decision is out of the attention set
exit 0
```
The offered look-back command works:
```
$ git show 2b0958af:.engr/objects/01a0b58a-e924-72a0-826c-23c1e097839f.json | grep -o 'at most once per [a-z]*'
at most once per hour
```
Ordinary mutations are refused, on both paths:
```
$ engr prepare --object 01a0b58a --add --text "Another assertion." --agent
error: section 1 was sealed as 1:731312a2… and its current contents seal as 1:f964202e…
exit 5
$ engr prepare --object 01a0b58a --rename --title "X"
error: section 1 was sealed as … ; exit 5
```

### 4b. `divergent` — the last admitted Event removed, seals left intact

```
$ head -n -1 <stream> > x && mv x <stream>
$ engr show 01a0b58a
1 sections   1 ok   rev 4
!!         Object revision is not what its admitted history produced; its seals verify, so something rewrote and resealed it. Restore it with: engr repair
exit 5
$ engr show 01a0b58a --format json | grep integrity
  "integrity": "divergent",
$ engr verify
          its revision is not what its admitted history produced; it was changed outside an admission path
exit 5
$ engr ls --verify
⚠ 01a0b58a  decision/accepted     §-  OBJECT DIVERGENT — nobody is looking at this one
```
`tampered` and `divergent` are distinguished exactly as documented, in every
surface, with different wording and a different `integrity` value.

### 4c. `repair`

```
$ engr repair 01a0b58a                       # against the tampered state
Challenge  repair
Object     01a0b58a  Submission dispatch budget
Integrity  the stored record does not verify
Restoring  exactly what admitted history proves at rev 3, admitted as rev 4, and nothing from the stored bytes

  §1.text
    stored   Every submission is dispatched at most once per minute, …
    restore  Every submission is dispatched at most once per hour, …

(object.repaired.v1)
                  CONFIRM TTRQTL
$ engr confirm "CONFIRM TTRQTL"
CONFIRMED  01a0b58a  object.repaired.v1  rev 4
$ engr verify
01a0b58a  PASS  1 sections  Submission dispatch budget
```
It names exactly what is discarded, field by field, old value beside new. On the
divergent object it reads `Integrity  the stored record verifies, and is not what
its admitted history produced` with `rev  stored 4 / restore 3`.

A prepared repair candidate cannot be landed on a state that moved:
```
$ <restore the event stream, so the object is healthy again>
$ engr confirm "CONFIRM L7ZCDE"
error: the object revision moved to 4 after this challenge was prepared at 3; prepare it again
exit 6
```

**VERDICT: HOLDS** for tampered/divergent detection, refusal of ordinary
mutations, the look-back command, and repair.

### 4d. `unreplayable` — I could not produce it, and both attempts broke the survey

Two corruptions of the EventStore:
```
# (i) second line replaced with `{ this is not json`
$ engr show 01a0b58a
error: /…/01a0b58a-….jsonl:2: key must be a string at line 1 column 3
exit 4
$ engr show 01a0b58a --format json      # prints nothing at all
$ engr verify ; engr ls --verify ; engr repair 01a0b58a
error: /…/01a0b58a-….jsonl:2: key must be a string at line 1 column 3     (each, exit 4)

# (ii) first event removed — valid JSON, incomplete history
$ engr verify
error: /…/01a0b58a-….jsonl: non-empty Event history starts at revision 1, not 2
exit 4
```

**GAP 4d-1 — the documented `unreplayable` state was never reached.** SKILL.md
and the `show` contract describe a fourth `integrity` value, `unreplayable`,
reported per object with "This is damage to the EventStore, and `repair` is *not*
the answer". Both kinds of EventStore damage I could produce surface instead as a
bare parse/shape error with exit 4 and no `integrity` field at all; `show
--format json` emits no document. An agent reading the skill would look for
`unreplayable`, not find it, and have to infer the state from a raw file path in
an error string.

**GAP 4d-2 — one damaged stream aborts the whole survey.** With a second, entirely
healthy object (`01a0b591 Partner billing cadence`) present:
```
$ engr ls --verify
error: /…/01a0b58a-….jsonl: non-empty Event history starts at revision 1, not 2
exit 4
$ engr verify
error: (same)
exit 4
$ engr show 01a0b591
01a0b591  open  Partner billing cadence          # the healthy object is fine
exit 0
```
`ls --verify` is the command SKILL.md tells you to run first, and the one it says
exists so "a survey of many objects is not cut short". One unreadable stream cuts
it short, reports nothing about any other object, and does not say which objects
went unchecked. It matters because the failure is silent in the direction that
hurts: an agent that runs `ls --verify`, sees an error about one file, fixes or
ignores it, and moves on has surveyed nothing.

**Observation (not a refusal I expected):** `engr work start` on the *tampered*
object succeeded and wrote a sidecar. Work is explicitly non-authoritative, so
this is arguably correct, but SKILL.md's "every ordinary change to it is refused"
reads wider than what is enforced.

---

## 5. Drift

Section `01a0b591 §1` created with `--ref 01a0b58a:1 text,role` (compact
`<object-prefix>:<section>` form, accepted alongside the canonical
`engr:obj:<26-char>:<n>`):

```
$ engr show 01a0b591
── §1 Reconciliation cadence ── ok
…
    refs     01a0b58a §1
```

### (a) an unrelated field of the target changes

`01a0b58a §1` revised, changing only `header` (`Dispatch budget` → `Dispatch
ceiling`), text and role untouched. The candidate screen showed exactly that:
```
Header   - Dispatch budget
Header   + Dispatch ceiling
Role       decision
Based on - 3dc30b80
Based on + 34682387
```
Afterwards:
```
$ engr show 01a0b591
1 sections   1 ok   rev 2
── §1 Reconciliation cadence ── ok
exit 0
$ engr verify        →  both PASS
```
**No drift reported.** Correct.

### (b) a selected field changes

`01a0b58a §1` text revised ("once per hour" → "once per day"):
```
$ engr show 01a0b591
1 sections   0 ok   1 stale   rev 2
── §1 Reconciliation cadence ── refs moved
…
    refs     01a0b58a §1
    advice   01a0b58a §1 changed selected fields: text
             git show 829fb5a9:.engr/objects/01a0b58a-e924-72a0-826c-23c1e097839f.json
exit 0
$ engr ls --verify
· 01a0b591  open                  §1  refs moved
$ engr verify
01a0b591  PASS  1 sections  Partner billing cadence
          §1 stands on 01a0b58a §1, which moved: text — a judgement, not an integrity failure
exit 0
```
Drift is reported **only** in case (b), it names which selected field moved, and
`verify` still exits 0 because it is a judgement, not an integrity failure.

The handed-over recovery command works:
```
$ git show 829fb5a9:.engr/objects/01a0b58a-….json | python3 -c 'import json,sys;print(json.load(sys.stdin)["sections"][0]["text"])'
Every submission is dispatched at most once per hour, because the downstream partner bills per delivery and the team agreed to a fixed monthly ceiling.
```
That is exactly the wording the reference was written against.

### (c) committing `.engr` does not create staleness

```
$ git commit -m "engr: revise the dispatch ceiling to a daily bound"   # .engr only
$ engr ls --verify
· 01a0b591  open                  §1  refs moved          # unchanged; no basis moved
$ engr show 01a0b58a | head -2
1 sections   1 ok   rev 6                                  # still ok
```
Then one real source commit:
```
$ git commit -m "paceq: tweak dispatch source"             # src/dispatcher.py
$ engr ls --verify
· 01a0b58a  decision/proposed     §1  basis moved
· 01a0b591  open                  §1  basis and refs moved
$ engr show 01a0b58a
── §1 [decision] Dispatch ceiling ── basis moved
    based_on 005c20fb   admitted 2026-09-18T17:32:22.05300235Z by human
    advice   1 commits and 1 files have changed since 005c20fb; check this still holds
```

**VERDICT: HOLDS.** Field-selective drift, `.engr`-blind basis comparison, and a
recovery command that actually recovers.

**GAP 5a — `basis moved` hands you no command.** SKILL.md says of *both* drift
markings: "`show` hands you `git show <commit>:<path>` — run it". For `refs
moved` it does. For `basis moved` the advice line is a count of commits and files
with no command to run, so the instruction cannot be followed for the marking
where it matters most (the wording was written against code that has since
changed, and nothing tells you which files).

---

## 5d. Undocumented authority matrix (found while setting up 5b)

Trying to revise, as an agent, a section a human had confirmed:
```
$ engr prepare --object 01a0b58a --revise 1 --text-file draft3.txt --header "Dispatch ceiling" --role decision --agent
error: §1 was admitted through the human gate, so its wording is changed there too
exit 5
```
And the one-command "revise a settled object back into the listing":
```
$ engr prepare --object 01a0b58a --revise 1 … --agent                       # bare
error: section.updated.v1 requires an object that needs attention, and accepted does not; classify it into proposed first

$ engr prepare --object 01a0b58a --revise 1 … --type decision --state proposed --agent
error: an agent-admitted section.updated.v1 cannot carry a destination: type and state are human-authoritative, and reaching them through another action does not change that
```

`engr protocol` has all of this in its authority matrix (agent MUST NOT reword or
delete a human-admitted section, MUST NOT carry a destination, agent merge
consolidates only agent-admitted sections, agent sections carry no `relations[]`
or `role=supersession`). The binary enforces it correctly.

**GAP 5d — SKILL.md documents none of it, and one passage contradicts it.** The
skill's "Type, state and attention" section says of the settled-object case
"Prefer that to reclassifying first and revising second", presenting the combined
form as the recommended route with no mention that the Agent path cannot take it
— while the bare refusal tells the agent to "classify it into proposed first",
i.e. the route the skill tells it not to prefer. An agent working autonomously on
a settled object hits two contradictory refusals in a row and has no documented
way through except a human confirmation the skill never mentions is required.

---

## 7. Unusable rules

### (a) `based_on` points at a missing file

`mss-ssot`'s basis path changed to `docs/MISSING-DOCTRINE.md`:
```
$ engr rules ls
mss-ssot  backlog, object
    based on docs/MISSING-DOCTRINE.md (current)
    review 3 attempts; on_exhaustion = human_confirmation
    UNUSABLE  rule mss-ssot: based_on docs/MISSING-DOCTRINE.md does not exist
exit 0
$ engr rules show mss-ssot
Based on   UNUSABLE — rule mss-ssot: based_on docs/MISSING-DOCTRINE.md does not exist
$ engr prepare --object 01a0b591 --add --text "…" --agent
error: rule mss-ssot: based_on docs/MISSING-DOCTRINE.md does not exist
exit 3
```

### (b) a pinned commit whose content has since changed

`based_on: - path: docs/RECORDING-DOCTRINE.md  commit: <full sha of ed1f535f>`,
then the doctrine extended and committed:
```
$ engr rules ls
    UNUSABLE  rule mss-ssot: based_on docs/RECORDING-DOCTRINE.md was reviewed at ed1f535f9db85897e1daaf9e9fb7d4d04cd20528 and the current file no longer matches it; review the rule against the current material and update its based_on commit
$ engr prepare --object 01a0b591 --add --text "…" --agent
error: (same message)
exit 4
```
An abbreviated pin is itself rejected up front:
```
    UNUSABLE  rule mss-ssot: based_on docs/RECORDING-DOCTRINE.md pins "ed1f535f", which is not a full git object id
```

**VERDICT: HOLDS.** In both cases the mutation is **refused**, never treated as
ungoverned, and the refusal names the rule and the exact defect. (Minor
inconsistency: (a) exits 3, (b) exits 4, for what is the same class of fault.)

---

## 6. The rest of the surface

### Listings
```
$ engr ls                       # attention only
01a0b58a  decision/proposed      1 sections  unchecked     Submission dispatch budget
$ engr ls --all                 # adds the closed/superseded ones
$ engr ls --all --sections      # one line per section, text reachable by grep
01a0b58a §1   decision/proposed     unchecked       Every submission is dispatched at most once per day, …
$ engr ls --verify
· 01a0b595  open                  §1  REF MISSING
```
`engr ls` also takes an undocumented-in-SKILL.md positional `KEYWORD` matched
against titles and section text (`engr ls --all billing`), which is a nicer
answer than the `| grep` the skill prescribes.

Exit codes, measured without a pipeline in the way:
```
$ engr show 01a0b595 >/dev/null; echo $?      → 5     (REF MISSING)
$ engr verify        >/dev/null; echo $?      → 5
$ engr ls --verify   >/dev/null; echo $?      → 0
```
Exactly as documented.

### Markings observed, all as documented
`TAMPERED`, `REF TAMPERED`, `REF MISSING`, `basis moved`, `refs moved`, and the
object-level `OBJECT TAMPERED` / `OBJECT DIVERGENT`. When the target object's
integrity fails, sections that would otherwise read `REF MISSING` are reported as
`REF TAMPERED` — the integrity fault takes precedence, which matches "the detail
names the side".

### `--merge` / `--delete`
```
$ engr prepare --object 01a0b591 --merge 2 --text "x" --agent
error: a merge needs at least one section to consume
$ engr prepare --object 01a0b591 --merge 2 --sources 2 --text "x" --agent
error: §2 survives the merge, so it cannot also be consumed by it
$ engr prepare --object 01a0b591 --merge 2 --sources 9 --text "x" --agent
error: section §9 does not exist
$ <valid merge>   → ADMITTED  section.merged.v1   destination keeps id 2
$ <delete §2, then add>  → new section is §4, ids 2 and 3 left as gaps
```

### `--close` / `--reopen` vs `--classify`
```
$ engr prepare --object 01a0b591 --close --agent
error: object.state_changed.v1 sets the object's own lifecycle, which is a human admission
$ engr prepare --object 01a0b58a --close                 # a typed object
error: a decision cannot be closed; it is one of proposed | accepted | rejected | superseded
$ engr prepare --object 01a0b591 --reopen                # already open
error: 01a0b591-… is already open, so there is nothing to confirm
$ engr prepare --object 01a0b591 --classify --type risk --state proposed
error: a risk cannot be proposed; it is one of identified | accepted | mitigated | invalidated
$ engr prepare --object 01a0b591 --classify --type decision              # state omitted
error: … (usage) — both halves are required
```
Type/state validity is decided *before* the review gate; a bad pair never gets a
digest. Good.

### `--supersede`
```
$ engr prepare --object 01a0b58a --supersede 01a0b595 --text "…" --agent
error: object.superseded.v1 sets the object's own lifecycle, which is a human admission
$ <human path>
Challenge  supersede
Role       supersession
Relation   superseded_by -> engr:obj:01m2tsbw4nemraa2rhjbvp9p8h
State      superseded — this object leaves the default listing, and the relation above is where a reader is sent instead
```
One command, one confirmation, state + relation + reason together, as documented.

### `--oversize`
```
$ engr prepare --object 01a0b595 --add --text-file big.txt --agent           # 16080 bytes
error: this Section is past what the record will hold at all: section text is 16080 against a hard limit of 5000. … There is no flag for this one: the hard ceiling always refuses, and --oversize will refuse it again.

$ engr prepare --object 01a0b595 --add --text-file mid2.txt --oversize       # never proposed before
error: an oversize exception is the retry of a refusal, and engr has not refused this proposal; prepare it without --oversize first and read what that refusal suggests

$ engr prepare --object 01a0b595 --add --text "A short assertion…" --oversize
error: nothing in this Section exceeds a normal limit, so there is no exception to make; prepare it without --oversize

$ engr prepare --object 01a0b595 --add --text-file mid.txt                   # 2520 bytes
error: this Section is larger than one assertion normally needs: section text is 2520 against a normal limit of 1200. … prepare it again with --oversize and the candidate will say so where the human can see it.
$ engr prepare --object 01a0b595 --add --text-file mid.txt --oversize …
OVERSIZE   admitted by exception: section text is 2520 against a normal limit of 1200
```
engr really does remember which exact proposal it refused — `--oversize` is
**enforced** as a retry, not requested. Exactly as documented. Note it is
Human-only: `error: --oversize is a Human candidate exception; an Agent admission
cannot claim it` (undocumented in SKILL.md — see GAP 5d).

### `work`
```
$ engr work start engr:backlog:01m2tryrj3ezgtnc3naqrhwdfr --summary "…"
$ engr work item add / state --state active / result / commit --commit HEAD
$ engr work block --reason "waiting for the partner to answer"     → State blocked, Blocked by [0]
$ engr work unblock --index 0
$ engr work depend --on engr:obj:01m2tsbw4nemraa2rhjbvp9p8h --reason "…"
$ engr work summary … --text <400 chars>
error: summary is a handoff note, not the work itself (400 characters, limit 300). …
$ engr work item add … --text <200 chars>
error: a work item is a handoff note, not the work itself (200 characters, limit 160). …
```
Limits are enforced with no escape hatch, as promised.

The sidecar refusal on consume is real, and fires *before* the review gate:
```
$ engr backlog consume 01a0b58f --section 1 --expect 7a6b5b29… --attempt 1
error: this was the last unresolved point, so resolving it removes the item — but it still has execution memory, which cannot outlive what it belongs to. Discard it with `engr work rm engr:backlog:01m2tryrj3ezgtnc3naqrhwdfr` and consume again, or record what it was for first
exit 5
```
Consuming a *non-last* point with a sidecar present is unaffected (`consumed §2`),
as documented.

`pause` / `rm`, the honour-based part:
```
$ engr work pause engr:backlog:…       → State  paused
$ engr work rm engr:backlog:…
no execution memory for backlog item engr:backlog:01m2tryrj3ezgtnc3naqrhwdfr
that work was paused; a human's stop signal went with it
exit 0
```
It deletes paused work and then tells you what you destroyed. That is "the tool
merely asks you not to", exactly as SKILL.md says.

### `collection`
```
$ engr collection new q4-billing --title "Q4 partner billing" --description "…" --start 2026-10-01 --end 2026-12-31
$ engr collection add q4-billing --target engr:obj:… --order 10 --priority high --reason "…"
$ engr collection add q4-billing --target <other> --order 10
error: two members are both ranked 10, so the order says nothing at that point
exit 4
$ engr collection add q4-billing --target engr:obj:01m2ts2vp1ekgsjedv3qq7p4re:1
error: --target "engr:obj:…:1" must identify a current whole Object or Backlog item
$ engr collection add q4-billing --target engr:backlog:<consumed item>
error: backlog:01m2tryrj3ezgtnc3naqrhwdfr does not exist
$ engr collection order … --order 5 ; engr collection priority … (omitted → cleared)
$ engr collection ls
q4-billi  open        2 members   2 need attention  Q4 partner billing
$ engr collection state q4-billing --state completed        → State completed
$ engr collection delete q4-billing
deleted collection q4-billing — "Q4 partner billing", 1 member(s) of planning context
exit 0
```
Everything documented holds, including duplicate-rank refusal and the
section-target refusal. `delete` just does it and reports the loss — honour-based,
as stated.

Minor: `collection ls` prints the id truncated to eight characters
(`q4-billi`), which is not the id you created; it happens to resolve because
collections accept unique prefixes too, but the listing is showing a string that
is not the identity.

### Backlog `merge` / `produced` / `consume`
```
$ engr backlog produced 01a0b58f --section 1 --target engr:obj:01m2trnt94eag84v13r7g9f0wz:1 --expect … --review … --reviewed-rule …
§1 produced engr:obj:01m2trnt94eag84v13r7g9f0wz:1; still unresolved
$ engr backlog merge 01a0b58f --into 1 --section 3 --text "…" --expect <dest only> --attempt 1
error: what you read is not what is there now; read it again and review the current wording
$ engr backlog merge … --expect <dest> --expect <source> --attempt 1 --review … --reviewed-rule …
merged §3 into §1                    # destination keeps id 1; `produced` came along
$ engr backlog consume 01a0b58f --section 1 --expect … --review … --reviewed-rule …
consumed §1, and the topic with it — nothing else was unresolved
```
**GAP 6a — a merge missing its second `--expect` is reported as a stale read.**
The message "what you read is not what is there now; read it again and review the
current wording" sends you to re-read a topic that has not moved. The actual
fault is a missing token. It matters because the prescribed response (re-read,
re-review) is wasted work and the second read produces the same failure.

### Addressing
```
$ engr show engr:obj:01m2trnt94eag84v13r7g9f0wz        → works
$ engr prepare --object engr:obj:01m2ts2vp1ekgsjedv3qq7p4re --rename …   → works
$ engr show 01a0b5
error: "01a0b5" matches 3 objects; use more characters
exit 3
$ --ref 01a0b58a:1 text,role                            → compact form accepted
$ --ref engr:obj:01m2ts2vp1ekgsjedv3qq7p4re:1@<full sha> text   → pinned form accepted and admitted
$ --ref engr:obj:…:1@HEAD text                          → accepted
$ --ref <a section not present at the basis commit>
error: section 1 is not in 01a0b595-… at 247fff8fb435a2a19dae7e8fb3a060e4162273fe
```

### `migrate`, `init`, `--root`
```
$ engr migrate
error: this workspace is not the released predecessor, so there is nothing to migrate
exit 4
$ engr init
error: /home/user/dogfood/paceq-probe/.engr already exists
exit 4
$ engr --root /home/user/dogfood/paceq-probe ls          # from /tmp — works
```

### BUG 6b — a deletion that orphans another object's section is reviewed blind

`01a0b595 §2` was created with `--ref engr:obj:01m2ts2vp1ekgsjedv3qq7p4re:5 text`.
Deleting that target:
```
$ engr prepare --object 01a0b591 --delete 5 --agent
open  Partner billing cadence

── §4 Invoice check ──
Invoices are checked against the dispatch count before payment, …
    based_on 247fff8f

NEEDS REVIEW  governed by mss-ssot, prose-purity. …
```
Nothing on the review screen, and nothing in the admission output, mentions that
another object depends on the section being removed. Afterwards:
```
$ engr show 01a0b595
── §2 ── REF MISSING
    advice   01a0b591 §5 no longer exists; what this section stood on is gone
```
Why it matters: the screen the agent is told to hand a reviewer is supposed to be
"the whole value" of the mutation, and the most consequential effect of a delete
— breaking authority another section stands on — is not on it. The damage
surfaces only later, on a *different* object, which `engr ls --verify` marks
"nobody is looking at this one" if it is out of the attention set.

### Ungoverned case (both rule files removed)
```
$ engr prepare --object 01a0b591 --add --text "An ungoverned assertion." --agent
error: Agent semantic Object admission needs at least one applicable usable Object Rule

$ engr prepare --object 01a0b591 --add --text "An ungoverned assertion." --agent --review 1:000…000 --reviewed-rule mss-ssot --review-result passed
error: no Object Rule applies to this mutation, so there is no Rule Review to attest
exit 2
```
Both protocol MUSTs hold: an ungoverned semantic Agent mutation is refused
outright, and an attestation over nothing is refused rather than ignored.

### BUG 6c — the candidate note advises a flag the tool refuses

Every passed-review Human candidate ends with:
```
note       this review passed, so there is nothing here for a person to
           overrule. If you are admitting your own work, `--agent` writes it
           now and records agent admission; this path mints a code instead,
           and that code is answered by a human or not at all.
```
It is printed verbatim on `classify`, `close`, `reopen` and `supersede`
candidates — the human-only actions, where `--agent` is refused:
```
$ engr prepare --object 01a0b591 --close --agent
error: object.state_changed.v1 sets the object's own lifecycle, which is a human admission
$ engr prepare --object 01a0b58a --supersede 01a0b595 --text "…" --agent
error: object.superseded.v1 sets the object's own lifecycle, which is a human admission
```
Reproduction: prepare any of those four on the Human path with a passing review
and read the note, then run the same command with `--agent`. It matters because
the note is precisely the nudge an agent reads when it has nobody to hand the
code to, and following it produces a refusal rather than the promised write — and
the message's framing ("if you are admitting your own work") is the one place the
tool appears to invite the agent to take the Human path's subject for itself.

---

## Ranked findings

1. **BUG 2b — `human_confirmation` exhaustion does not escalate on the documented
   route.** Repeating the `--agent` command with `--review-result exhausted` and
   `--review-explanation`, exactly as SKILL.md instructs, is refused with
   `an Agent mutation is admitted only by a passing Rule Review`. Escalation to
   the Human Gate happens only if you drop `--agent` and re-attest against a
   different, unsurfaced digest. The protocol says the mutation MUST escalate.
   This is the only sanctioned way past an exhausted review, and the failure
   pushes an agent toward the one thing the whole mechanism depends on it not
   doing: restarting the attempt count at 1.

2. **BUG 3b — `engr show --format json` drops the stored `header`.** The field is
   inside the seal, `prose-purity` explicitly judges material moved into a
   header, and SKILL.md tells the agent to hand a delegated reviewer the
   structured screen. A reviewer fed the JSON cannot see the header at all,
   reproducing the exact failure SKILL.md spends a paragraph warning about.

3. **GAP 4d-2 — one damaged EventStore stream aborts `engr ls --verify` and
   `engr verify` entirely.** With a healthy second object present, both commands
   exit 4 printing a single file-path error and report nothing about any other
   object, and nothing says which objects went unchecked. This is the command the
   skill says to run first, and the one it says exists so a survey is not cut
   short.

4. **BUG 6b — deleting a section other sections depend on is reviewed blind.**
   Neither the review screen nor the admission output mentions the dependent
   section; the breakage appears afterwards as `REF MISSING` on a different
   object, which `ls --verify` may mark "nobody is looking at this one".

5. **GAP 5d — SKILL.md documents none of the authority matrix, and contradicts
   part of it.** Agent cannot reword or delete a human-admitted section, cannot
   carry `--type`/`--state` on a section action, cannot use `--oversize`,
   `--close`, `--reopen`, `--classify`, `--supersede` or `repair`. The binary
   enforces all of it correctly; the skill instead recommends the combined
   revise+reclassify form ("Prefer that to reclassifying first and revising
   second") that the Agent path refuses, while the refusal tells the agent to do
   the opposite.

6. **BUG 6c — the passed-review candidate note tells the agent that `--agent`
   "writes it now" on actions where `--agent` is refused** (`classify`, `close`,
   `reopen`, `supersede`).

7. **GAP 4d-1 — the documented `unreplayable` integrity state was unreachable.**
   Both EventStore corruptions I produced surfaced as an exit-4 parse/shape error
   with no `integrity` field, and `show --format json` emitted nothing.

8. **GAP 2a — an exhausted `reject` rule is not named in the refusal.** The
   message is identical to an ordinary failed attestation, so an agent cannot
   tell "your autonomous path is over, a human must raise this" from "you
   mis-attested".

9. **GAP 1.4a — attesting `failed` first demands `--review-explanation`, then
   refuses regardless.** The first refusal reads as though an explanation would
   admit it, inviting an agent to manufacture an override rationale for a review
   it knows failed.

10. **GAP 5a — `basis moved` hands you no `git show` command**, only a count of
    commits and files, although SKILL.md instructs you to run the command `show`
    hands you for both drift markings. `refs moved` does hand one, and it works.

11. **GAP 6a — a backlog merge missing its second `--expect` is reported as a
    stale read**, sending you to re-read a topic that has not moved.

12. **Cosmetic:** `engr collection ls` prints an id truncated to eight characters
    that is not the id you created; the two unusable-rule faults exit 3 and 4 for
    the same class of problem.

### What held, that I tried hard to break

- Every ReviewDigest forgery: invented digest, reused digest from another
  mutation, reused digest after a one-character edit, replayed digest for a
  byte-identical repeat, omitted rule id, extra rule id, attempt 0, `passed` past
  the ceiling. All **prevented**.
- Recomputation under the lock: a whitespace-only edit to a rule file, and an
  edit to the file a rule rests on, both invalidate a digest issued seconds
  earlier. **Prevented.**
- `--oversize` as a retry-only exception, including on a proposal engr had never
  refused and on one that breaks no limit. **Prevented.**
- Backlog stale-write (`--expect`), consume/merge/rename past the ceiling, the
  `review_exhaustion` marker, and the work-sidecar refusal on a last-point
  consume. **All prevented.**
- Integrity: `tampered` vs `divergent` distinguished in every surface, ordinary
  mutations refused, `repair` showing exactly what it discards, and a prepared
  repair candidate refused once the object moved. **All prevented.**
- Drift reported only for the selected field, blind to `.engr` commits, with a
  `git show` that recovers the wording the reference was written against.
- The human gate: wrong code, lowercase, double space and bare "yes" all refused;
  a qualified yes **discards** the candidate; a stale code after re-prepare is
  dead and the tool says which code it superseded.

### What the tool only asks

Three things, all documented as such, all confirmed unenforced in this probe:

- **That a review actually happened, and that its verdict is honest.** Every
  `passed` in this file was attested by the author of the wording, with no
  delegated reviewer. engr accepted all of them.
- **That the attempt number is real.** Nothing persists; a new attempt 1 is
  always available.
- **That a human answered the challenge.** I typed `CONFIRM 3QQSR2`,
  `CONFIRM TTRQTL` and others myself. The record now says `"by": "human"` with a
  challenge code, and no later reader can tell.
- **That `paused` and `collection delete` are respected.** `engr work rm` deleted
  paused work and told me a stop signal went with it.

# Working in this repository

## What this is

engr v0. An object holds sections; every change to a section goes through a gate
where a human reads it and types a challenge code. Sections are the current
authority, confirmed events are append-only history, and git anchors committed
projections.

Alongside it, backlog holds work nobody has settled. It is freely agent-editable
and confirmed by no one, which is the whole reason the record can stay strict —
there is somewhere else for unresolved material to go.

Read [protocol/PROTOCOL.md](protocol/PROTOCOL.md) before changing behaviour. It is
normative for the implementation in this branch, and it explains why each rule
exists rather than just stating it.

## Accepted next direction — not implemented in this branch

The trust model above describes the current v0 implementation, but it is no
longer the complete design direction. Do not infer from the current code that
all future Object content must always enter through the Human Gate.

The accepted direction being coordinated in #25, with corresponding baseline
updates to #9/#16, is **rule-governed agent admission of Object knowledge**:

- A project may define agent-readable Rules under `.engr/rules/*.md`.
- Autonomous Object admission is project opt-in through Rules: an Agent may add
  Object content autonomously only when at least one valid, applicable
  `domain=object` Rule resolves successfully. No usable applicable Object Rule
  means the mutation must fall back to the Human Gate path.
- This prerequisite applies to adding agent-admitted Sections to existing
  Objects as well as creating an Object containing agent-admitted Sections; an
  existing Object is not an escape hatch around Rule governance.
- Object Sections are moving toward explicit admission/trust provenance rather
  than treating the whole Object as uniformly human-authoritative. The current
  working distinction is human-admitted authoritative content versus
  rule-reviewed agent-admitted unconfirmed content. The exact persisted field
  name/schema is still subject to the owning design issue; do not invent it.
- Agent admission must not let an Agent silently rewrite or delete
  human-admitted authoritative Sections, demote their authority, or manufacture
  authoritative lifecycle/relationship effects. Promotion of exact current
  agent wording to human authority goes through the Human Gate.
- Human-authoritative dependency semantics must not transitively depend on
  unconfirmed agent wording. Any representation change needed to preserve that
  invariant belongs in the design work, not an ad-hoc implementation shortcut.
- Rule Review is stateless in v1. A review binding hashes the exact mutation,
  target precondition/current-state fingerprint, applicable Rules, and resolved
  Rule bases. Admission recomputes it and fails closed if the reviewed subject
  changed.
- Rule `based_on` entries may be floating current paths or may include an exact
  Git commit. An exact-commit basis becomes stale only when the relevant current
  path content differs from the reviewed historical content, not merely because
  repository HEAD advanced.
- Rules may bound review attempts. The Agent reports the attempt number in v1;
  engr does not yet persist or independently prove attempt history. Backlog may
  soft-admit exhausted review with `rule_review { attempts, limit }`; Object
  exhaustion may escalate to human confirmation according to the final Rule
  policy. Do not invent Collection/Work overflow semantics until specified.
- Confirmed EventStore history remains the history of human-admitted semantic
  authority; autonomous Agent edits should not flood it with pseudo-confirmed
  Events merely to reuse the old mechanism.

This is a coordination notice, not permission to implement the proposal inside
PR #27. PR #27 remains the known-good v0 baseline. When work targets the next
trust model, read the latest accepted refinements on #25/#9/#16 before changing
persisted representation, admission semantics, EventStore semantics, or the
Human Gate. If those sources still leave a trust-boundary choice unresolved,
stop rather than inventing one.

## Layout

```text
crates/engr/src/model.rs        objects, sections, the confirmed payload, projection
crates/engr/src/semantics.rs    type/state, attention, roles, relations, bounded content
crates/engr/src/gate.rs         prepare, confirm, candidates
crates/engr/src/confirmation.rs the shared, domain-neutral admission primitive
crates/engr/src/backlog.rs      unresolved staging: subjects, produced, reconciliation
crates/engr/src/work.rs         execution memory: the sidecar an agent keeps for an object
crates/engr/src/collection.rs   planning metadata: plans, members, order, priority
crates/engr/src/store.rs        filesystem layout, locking, atomic writes
crates/engr/src/git.rs          HEAD, distance, uncommitted, path provenance
crates/engr/src/ops.rs          reconcile, verify
crates/engr/src/reference.rs    the one engr: reference parser and compact codec
crates/engr/src/view.rs         staleness assessment, show, ls, staging surfaces
crates/engr/src/main.rs         the CLI
crates/engr/tests/gate.rs       what may enter the record
crates/engr/tests/semantics.rs  what an object is, and what a section may carry
crates/engr/tests/record.rs     what the record then guarantees
crates/engr/tests/backlog.rs    what staging is, and what it is not
crates/engr/tests/work.rs       what execution memory is, and what it owns (nothing)
crates/engr/tests/collection.rs what a plan is, and what grouping something does not mean
crates/engr/tests/cli.rs        what the command line promises the outside world
```

## If you are the agent using engr, not editing it

Propose with `prepare`; never look for another way in, because there is not one
in the current v0 implementation. Do not confuse that implementation fact with
the accepted next design direction described above.

`engr candidate <code>` re-renders a pending candidate. Use it when a human comes
back later — **re-running `prepare` mints a new code and voids the one they are
holding.**

Read state with `engr show <id>`. Each section is annotated with how far it can be
trusted and what to do about it; that is not a separate report to fetch. Use
`--format json` when you want structure: an Object has one lifecycle field,
`state`, valid for its optional `type`, plus a derived `attention` that is never
stored — while each Section's `status` is computed, not stored either.

When `show` says a section's basis or a dependency moved, do not quietly reason
from it. Recover the old wording with the `git show` command it hands you, decide
whether the section still holds, and if it does not, propose a revision.

Unresolved work goes in `engr backlog`, which needs no confirmation. Never read
what is there as though it were the record — a section that is gone from backlog
is one somebody judged settled, and one that is still there is not, whatever it
has already produced.

## Conventions

Comments explain why, not what. Most of the sharp edges here exist because
something specific failed before; the comment is where that stays recoverable.
Do not add a comment that restates the line beneath it.

Match the surrounding code: `ensure!` for rule violations with an exit code, plain
`Result` everywhere, no `unwrap` outside tests unless the invariant is stated
right there.

Before claiming anything works, run it:

```bash
cargo test --workspace
cargo fmt --all -- --check
```

## What not to do

Do not add fields, actions, or statuses because they seem likely to be needed.
The growth rule in the protocol is the whole reason this version exists — see the
table of what is deliberately absent and the signal that would bring each one in.

Do not make the reducer depend on anything outside the event. No clocks, no git,
no model calls.

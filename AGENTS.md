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
normative, and it explains why each rule exists rather than just stating it.

## Layout

```text
crates/engr/src/model.rs        objects, sections, the confirmed payload, projection
crates/engr/src/semantics.rs    type/state, attention, roles, relations, bounded content
crates/engr/src/gate.rs         prepare, confirm, candidates
crates/engr/src/confirmation.rs the shared, domain-neutral admission primitive
crates/engr/src/backlog.rs      unresolved staging: subjects, produced, reconciliation
crates/engr/src/work.rs         execution memory: the sidecar an agent keeps for an object
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
crates/engr/tests/cli.rs        what the command line promises the outside world
```

## If you are the agent using engr, not editing it

Propose with `prepare`; never look for another way in, because there is not one
and adding one would defeat the point.

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

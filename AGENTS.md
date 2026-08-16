# Working in this repository

## What this is

engr v0. An object holds sections; every change to a section goes through a gate
where a human reads it and types a challenge code. Sections are the current
authority, confirmed events are append-only history, and git anchors committed
projections.

Read [protocol/PROTOCOL.md](protocol/PROTOCOL.md) before changing behaviour. It is
normative, and it explains why each rule exists rather than just stating it.

## Layout

```text
crates/engr/src/model.rs    objects, sections, the confirmed payload, projection
crates/engr/src/gate.rs     prepare, confirm, candidates
crates/engr/src/store.rs    filesystem layout, locking, atomic writes
crates/engr/src/git.rs      HEAD, distance, uncommitted
crates/engr/src/ops.rs      reconcile, verify
crates/engr/src/view.rs     staleness assessment, show, ls
crates/engr/src/main.rs     the CLI
crates/engr/tests/gate.rs   what may enter the record
crates/engr/tests/record.rs what the record then guarantees
```

## If you are the agent using engr, not editing it

Propose with `prepare`; never look for another way in, because there is not one
and adding one would defeat the point.

`engr candidate <code>` re-renders a pending candidate. Use it when a human comes
back later — **re-running `prepare` mints a new code and voids the one they are
holding.**

Read state with `engr show <id>`. Each section is annotated with how far it can be
trusted and what to do about it; that is not a separate report to fetch. Use
`--format json` when you want structure. `status` there is computed, not stored.

When `show` says a section's basis or a dependency moved, do not quietly reason
from it. Recover the old wording with the `git show` command it hands you, decide
whether the section still holds, and if it does not, propose a revision.

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

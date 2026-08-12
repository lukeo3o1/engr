# Contributing

## The bar for adding anything

> Add to the model only when a real, recorded use needed it and working around it
> cost more than adding it.

This is not a style preference. The previous design shipped 48 event types and 35
of them never fired once during the only day it was genuinely used. If you cannot
point at a use that demanded a change, the change is speculation, and speculation
is what v0 exists to undo.

The protocol lists what is deliberately absent, each with the signal that would
bring it in. If you are adding one of those, say which signal fired.

## Changing the model

A change to the object/section model, the actions, or the gate needs three things
to move together:

1. **`protocol/PROTOCOL.md`** — it is normative. If the code and the document
   disagree, say which one is wrong.
2. **Tests** — `tests/gate.rs` for what may enter the record, `tests/record.rs`
   for what the record then guarantees.
3. **The reasoning, in a comment** — not what the code does, but why it is that
   way. Most of the sharp edges here came from a specific failure; the comment is
   where that stays recoverable.

## Things that are load-bearing

Changing any of these needs a paragraph in the pull request, not just a diff:

- **The exact confirmation phrase.** Accepting a bare code would put the agent in
  the position of deciding whether a qualified yes counted as a yes.
- **Section ids are never reused.** Reuse silently repoints every outside
  reference. The counter must survive a purge.
- **The section hash covers `refs` and `based_on`, not just `text`.** Otherwise a
  reference can be repointed and `verify` still passes.
- **The reducer is deterministic.** No clocks, no git, no model calls. `verify`
  has no oracle without it, and a record whose meaning depends on which model read
  it is not a record.
- **`prepare` validates references.** Deferring that to `verify` is what let one
  mistyped id in the previous design poison a global check permanently.

## Tests

```bash
cargo test --workspace
cargo fmt --all -- --check
```

Name tests after the property they pin, not the function they call —
`section_ids_are_never_reused`, not `test_delete`. A test whose name does not say
what breaks if it fails is hard to trust years later.

## Commits

Conventional prefixes (`feat`, `fix`, `docs`, `test`, `refactor`). Say what
changed and why; the diff already says how.

Work on a branch and open a pull request. CI runs the suite and the formatter on
Linux, macOS and Windows.

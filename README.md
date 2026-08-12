# engr

**Engineering records whose every word a human confirmed.**

An object holds sections. Each section carries text, the commit it was written
against, and references to the sections of other objects it depends on. Adding,
revising, merging, deleting, closing — all of it goes through one gate: an agent
proposes, a human reads the change, and types a challenge code. There is no other
way in.

```bash
engr prepare --object 019ff75b --add --text-file draft.txt
#   Candidate  section.added
#   Based on   9348f28f
#
#   寫入只有一條路:prepare → confirm。
#
#   逐字輸入以確認:  CONFIRM 7U9K2U

engr confirm 'CONFIRM 7U9K2U'
```

## What it is for

Long-running work drifts in two different ways, and only one of them is a file
changing. The other is that **nothing changed and the world moved on** — a record
sits untouched for three months, still reads correctly, and its basis has
quietly gone out from under it. Nothing in a diff can see that, because a diff
only records actions.

engr gives that second kind of drift two signals, neither of which needs anyone
to be reading:

- **The basis moved.** A section records the commit it was written against, so
  how far HEAD has since travelled is a computation.
- **A dependency changed.** A reference pins the hash of the section it depends
  on, so the target being rewritten is a computation too — and it pins the commit
  as well, so `git show` recovers what it used to say.

The case worth interrupting for is a **closed** object whose basis moved. Closed
means nobody is looking, which is exactly when drift goes unnoticed.

## Status

**v0, not released.** The protocol is in [protocol/PROTOCOL.md](protocol/PROTOCOL.md).
Sixteen tests cover the gate and the record. There is no installer yet; build
from source.

v0 is a deliberate rewrite. The previous design was event-sourced with 48 event
types, of which **35 never fired once** on the only day it was genuinely used. v0
keeps the part that worked — the gate — and delegates history to git. It grows
only when a recorded use demands it; see the growth rule in the protocol.

One thing it does **not** solve: `prepare` prints the challenge code where the
agent can read it, so nothing stops an agent confirming its own proposal. The
gate is a convention, not yet a mechanism.

## Using it

```bash
engr init                                    # in a git repository
engr prepare --new --text "the title"        # propose an object
engr confirm 'CONFIRM <code>'                # the only way in
engr prepare --object <id> --add  --text-file f.txt
engr prepare --object <id> --revise 3 --text-file f.txt
engr prepare --object <id> --merge 1,2 --text-file f.txt
engr prepare --object <id> --delete 3
engr prepare --object <id> --close
engr candidate                               # what is awaiting a human
engr candidate <code>                        # show it again, hours later
engr ls                                      # open objects
engr ls --all --stale                        # what needs attention
engr ls --all --sections | grep <term>       # one line per section, greppable
engr show <id>                               # sections, and how far each can be trusted
engr show <id> --format json                 # the same, for an agent
engr purge <id>                              # drop the event buffer once settled
engr verify                                  # recompute section hashes
```

Objects are addressed by unique id prefix, like a git commit.

**Commit `.engr/objects`.** git is where old wording is recovered from; without
it, look-back disappears silently.

## Build

```bash
cargo test --workspace
cargo run -p engr -- --help
```

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md). The short version: a change to the model
needs the protocol and the tests to move with it, and anything new has to be
something a real use asked for.

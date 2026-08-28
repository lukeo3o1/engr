# A workspace the published release wrote

These bytes are not a hand-authored approximation of workspace version 1. They
were produced by building the published `latest` release and driving its own
Human Gate — `prepare`, read the rendered candidate, answer `CONFIRM <code>`
exactly — until the record was what it is. Nothing in `workspace/.engr` was
typed by hand, and no byte of it was edited afterwards.

That distinction is the whole point of the fixture. A JSON file written to look
like version 1 proves that the migrator accepts a shape somebody believed
version 1 had. Only the release's own output proves it accepts what the release
actually wrote.

## What produced it

| | |
|---|---|
| Release | `latest`, published 2026-08-16 |
| Commit | `e7d9f99733407a8c31cec33af18a92480f4f4c6f` |
| Declares | `.engr/format.json` → `{"format":"engr-workspace","version":1}` |
| Built with | `cargo build --release` at that commit, unmodified |

The release was checked out clean, built, and run against an empty Git
repository. Every semantic write went through `engr prepare` and was admitted by
the exact challenge phrase; the transcript is in the pull request that added
this fixture.

## What is here

`history.bundle` is the complete Git history, five commits, ending at
`7140a349b81c34fd7027a9d81f04e5ea6e0dfcf6`. It is the fixture's real content:
the two legacy references below pin commits, and migration resolves them by
reading the Object out of the commit it names, so the history is a precondition
rather than decoration.

`workspace/` is the same tree's final state, kept in readable form so the
version 1 bytes can be reviewed in a diff instead of only inside a pack file.
The test suite clones the bundle and asserts the checkout matches it byte for
byte, so the two cannot drift apart unnoticed.

## What the record contains

Four Objects, seven Sections, eighteen admitted Events.

| Object | Sections | What it exercises |
|---|---|---|
| `01a049f0-16fb-…` | §1, §2 | Plain Human-admitted wording with a committed `based_on` |
| `01a049f0-1d33-…` | §1, §2 | A legacy Ref; a Section with **no** `based_on` at all; a revision; a rename |
| `01a049f0-271b-…` | §4, §5 | A merge, a deletion, and a legacy Ref **whose target itself carries one** |
| `01a049f0-3711-…` | §1 | A closed Object |

Section ids carry the record's own rule about identity: `01a049f0-271b-…` added
four Sections, merged §1 and §2 into a new §5, then deleted §3. It keeps §4 and
§5 with `next_section_id` at 6, so nothing renumbered and nothing was reused.
`01a049f0-1d33-…` §2 was admitted with `--no-based-on`, so the member is absent
from the file rather than null — the absence version 3 has to map to an explicit
`null` without inventing a basis.

The chained reference is deliberate. `01a049f0-271b-…` §4 points at
`01a049f0-1d33-…` §1, which carries a legacy reference of its own. Converting a
reference rewrites exactly the member both the predecessor seal and the current
digest cover, so a target that has references is the case where taking those two
hashes over the same content gives the wrong answer.

## Rebuilding it

Nothing regenerates this fixture automatically, on purpose: a fixture that
rebuilds itself from the current tree stops being evidence of what the release
wrote. To reproduce it, check out `e7d9f99`, build it, run its `init` and its
gate, and compare. The identifiers and timestamps will differ, because they come
from the clock and from UUIDv7; everything about the shape will not.

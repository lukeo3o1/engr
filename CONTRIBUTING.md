# Contributing to Engr

Engr changes must preserve the Engineering Record protocol and make their
evidence easy to review. This guide owns repository workflow; the protocol,
schemas, conformance corpus, and Agent constitution own technical authority.

## Start with scope and authority

Read [AGENTS.md](AGENTS.md) before changing the repository. For a runtime or
protocol behavior, read the relevant document in `protocol/`, then the schema,
conformance fixture, and test that cover it. Do not use a README example or an
implementation shortcut to redefine a semantic rule.

Classify the change before coding:

- documentation-only clarification;
- implementation change that preserves the protocol;
- conformance or test coverage for an existing invariant; or
- deliberate semantic or protocol change.

The last category needs explicit human direction and matching protocol, schema,
conformance, implementation, and release-version evidence. Never present it as
a routine refactor or cleanup.

## Branches and commits

Use a short branch name in this form:

```text
<type>/<short-kebab-description>
```

Use common types such as `feat`, `fix`, `docs`, `refactor`, `test`,
`chore`, and `release`. The repository uses a simple trunk-based workflow
around `main`; do not introduce a mandatory long-lived integration branch
without explicit human approval.

Use Conventional Commits:

```text
<type>(<optional-scope>): <description>
```

Examples:

```text
feat(replay): validate canonical chain ancestry
fix(state): preserve retired semantic entities
docs(skill): clarify human alignment behavior
test(conformance): cover competing forks
```

Mark breaking semantic or protocol changes with `!` and/or a `BREAKING CHANGE:`
footer. A commit message must describe the change honestly.

## Implement and test

Keep code, protocol, schemas, fixtures, embedded project assets, and Skills in
sync when they share a contract. Do not hand-edit derived State, EventStore
history, generated release artifacts, or copied generated outputs to make a
test appear to pass.

Before opening review, run the applicable checks from the repository root:

```text
cargo fmt --all -- --check
cargo test --release --workspace --all-targets
cargo run -p engr -- version --handshake
```

For a protocol, reducer, schema, lifecycle, snapshot, confirmation, or renderer
change, also run the native conformance gate against an initialized project.
Review [conformance/CONFORMANCE.md](conformance/CONFORMANCE.md) for the exact
coverage required by the change. Add focused tests only for an invariant already
owned by the protocol; tests must not invent new semantics.

## Reviews and pull requests

Keep a pull request focused. Explain the problem, authority consulted, behavior
preserved or changed, tests run, and evidence still unavailable. Call out any
documentation claim whose evidence is only planned or external.

Reviewers should verify that:

- EventStore authority, deterministic replay, retained State, fork safety, and
  the Human Alignment Gate were not weakened;
- protocol, schemas, conformance, implementation, and documentation agree;
- an output or artifact is not being promoted to semantic truth;
- tests cover the changed invariant rather than only the happy path; and
- a claimed platform, release, or evaluation result has direct evidence.

Resolve review feedback with a new commit or an intentional amend before merge.
Do not force-push shared work without coordinating with its authors. Merge only
after the required checks and review requirements for the target branch are
satisfied.

## Releases

The Cargo package version is the Engr tool version; protocol and schema versions
are independent compatibility contracts. A release tag must match the Cargo
version in the form `v<version>`. Do not create a tag until the required Rust
tests, native conformance, and release-readiness evidence are available.

The release workflow packages each declared target, produces a SHA-256 checksum
and CycloneDX SBOM, and assembles `release-manifest.json` and `TOOLING.lock.json`.
Verify every declared artifact, checksum, SBOM, and manifest entry after the
tagged workflow completes. A configured target is not a supported platform
until its release CI and runtime smoke evidence succeed.

When publishing or withdrawing a release, preserve the evidence and release
metadata needed to identify what consumers ran. Do not rewrite protocol history
or silently replace an already published artifact.

## Skill distribution

This repository owns the canonical `engr` Skill. If another catalog carries the
Skill, that copy must be generated or vendored from this repository. Do not
maintain two independent editable versions.


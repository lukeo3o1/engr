# Engr

**Keep engineering context coherent across long-running human-AI collaboration.**

Engr is a complete Engineering Record system. It combines a protocol, an AI
Agent Skill, a deterministic Rust runtime, schemas, conformance fixtures,
behavioral-evaluation inputs, the `engr` CLI, and release tooling in one
canonical repository.

## The problem

Long-running engineering work often revises ADRs, specifications, design
documents, summaries, handoff material, and reports many times. Those mutable
documents can drift: constraints disappear, rejected options reappear, and
implementation or verification status becomes inconsistent. Future agents then
receive noisy or contradictory engineering context.

Engr is designed to mitigate this failure mode. It does not claim to eliminate
context drift, guarantee decisions, or establish empirical effectiveness.

## The Engr approach

```text
semantic engineering change
        -> Semantic Event
        -> EventStore
        -> deterministic replay
        -> State
        -> targeted views and generated documents
```

EventStore is the append-only authoritative semantic history. State is derived
by deterministic replay. Snapshots accelerate replay without becoming
authority. Artifacts are evidence, while ADRs, specifications, reports, and
agent views are derived communication surfaces. A document does not become
engineering truth merely because it was edited.

Every semantic entity remains in State with an explicit status, including
superseded, rejected, invalidated, and resolved history. Same-parent competing
events are a semantic fork and fail closed until explicit reconciliation.

## Human authority and semantic change

Human-authoritative changes cross the Human Alignment Gate: the exact candidate
is shown, a fresh challenge is issued, and only the exact `CONFIRM <code>`
response admits the displayed wording. This gate does not apply to every event.
Agent-originated observations, findings, hypotheses, implementation progress,
and verification results may be recorded when the protocol permits and their
certainty and provenance are accurate.

Engr records domain changes such as `fact.added`, `solution.selected`,
`verification.result`, and `decision.superseded`; it does not treat paragraph
edits or Markdown patches as semantic history. Implementation completion is not
verification, and verification passing is not automatic resolution.

## Using Engr

`engr` is the stable operational interface. Running `engr init` creates the
single supported project-local workspace, `.engr/`. The project-local runtime
and its declared compatibility are checked with `engr doctor`; the Skill
explains how an agent should read State, retrieve targeted provenance, classify
new knowledge, and record it safely.

The [protocol](protocol/PROTOCOL.md) owns the exact command surface, wire
formats, reducer rules, confirmation behavior, replay selection, fork handling,
and exit categories. The [Skill](skill/SKILL.md) owns runtime agent guidance for
adopted projects.

### Install a released binary

Released binaries are installed only after the installer verifies the release
manifest, its SHA-256 checksum file, and the downloaded archive. The Unix
installer supports Linux and macOS; Linux selects the portable musl artifact by
default, while `--target` can select a GNU artifact. The PowerShell installer
supports native Windows x64 and ARM64. Neither installer uses `sudo` or changes
your `PATH`.

Download the installer from the versioned GitHub Release rather than piping an
unversioned script into a shell:

```bash
curl -fsSLO https://github.com/lukeo3o1/engr/releases/download/v0.1.0/install.sh
bash install.sh --version 0.1.0
```

```powershell
Invoke-WebRequest https://github.com/lukeo3o1/engr/releases/download/v0.1.0/install.ps1 -OutFile install.ps1
.\install.ps1 -Version 0.1.0
```

Use `--bin-dir` / `-BinDir` to choose an installation directory and `--target`
/ `-Target` to select an exact supported target. Omitting the version selects
the latest GitHub release.

## Status and evidence

Source version `0.1.0` contains the Rust runtime, protocol/schema artifacts,
16 fixed conformance fixtures, the T7 long-horizon case, mutation coverage, and
an S01-S16 behavior-contract corpus. The Rust suite verifies the native CLI,
deterministic replay, confirmation recovery, snapshot integrity, conformance
fixtures, and corpus integrity.

Release workflows are configured to build Windows, Linux (GNU and musl), and
macOS artifacts from the same Rust implementation. A platform is described as
supported only after its release CI and runtime evidence are verified. The
repository does not claim a published release merely because a workflow or
manifest template exists.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) for branches, commits, reviews, tests,
and release discipline. Semantic changes require corresponding protocol, schema,
conformance, and behavior-test evidence.

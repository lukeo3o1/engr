# Engr tooling

Engr is the sole production implementation of Engineering Record protocol-v1. A project
bootstrapped by `engr init --root <project-root>` contains a copy at:

```text
.engr/tools/engr.exe  # Windows
.engr/tools/engr      # Linux and macOS
```

Run the platform-appropriate binary directly from the project root:

```text
.engr/tools/engr.exe doctor    # Windows
.engr/tools/engr doctor        # Linux and macOS
```

Every Engr binary must answer this exact, single-line handshake:

```text
engineering-record	protocol=1	event-schema=1	state-schema=1
```

Use `engr version --json` for descriptive implementation metadata. Its protocol, event-schema,
and state-schema fields are JSON integers.

If Engr is unavailable or its handshake fails, stop protocol writes and report the tooling
problem. Do not fall back to a legacy interpreter-based CLI, hand-edit State, or substitute
another writer. `engr verify` and `engr conformance` are the required checks after a tooling change.

Snapshot filenames are bounded by the tool. Do not shorten or rename them manually.

```text
engr conformance [--json]
```

The bundled immutable fixtures exercise the protocol contract. A release is ready only when the
Rust unit suite and the complete conformance suite pass; GitHub Actions supplies the cross-platform
build and packaging evidence.

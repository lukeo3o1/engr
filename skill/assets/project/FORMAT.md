# Engineering Record Format

Protocol: 1
Event schema: 1
State schema: 1

This project keeps durable engineering truth in `.engr/eventstore/` as append-only semantic JSONL events. Existing event lines and snapshots are immutable.

`.engr/state/` is deterministic derived JSON: a registry of every entity in a stream with its current status, never only the active ones. Entities change status; they never vanish. `.engr/outputs/` contains disposable views. Never patch either location to introduce engineering meaning; append an event, replay State, verify it, then regenerate views.

Streams are sharded by event date:

```text
.engr/eventstore/YYYY/MM/DD/WI-YYYYMMDD-NN.jsonl
.engr/eventstore/YYYY/MM/DD/DR-YYYYMMDD-NN-BADGE.jsonl
```

Work Item identity survives changes to title, understanding, solution, and status. Decision badges name a topic, not the chosen outcome. Events use collision-resistant IDs and a per-stream `parent` chain. Timestamp and file order never decide replay order.

Human-originated engineering truth enters only through `engr prepare`, display of the complete exact candidate, an exact fresh `CONFIRM <code>` response, and `engr confirm`. Generic acknowledgement or a qualified response writes nothing; corrections invalidate the old candidate.

At startup, run the project-local command described in `TOOLING.md`:

```text
engr doctor
engr replay <id>
engr show <id> --brief
```

Use State/brief views by default. Retrieve EventStore history with `engr why` and raw artifacts only for a targeted provenance, evidence, replay, or reopen question.

If replay reports a fork, stop. Do not choose by time or Git order. Reconcile every competing branch explicitly under `PROTOCOL.md`.

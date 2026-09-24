# Hooks for Claude Code

Three things in `skill/SKILL.md` fail in the same way when they are left to the
agent: the reads at the start of a session, the writes while the work is going,
and the handoff before it ends. An agent whose context was just compacted does
not remember to read, and an agent whose step just worked does not remember to
tick it. These move all three out of the agent's memory and into the harness.

| Hook | When | What it does |
| --- | --- | --- |
| `session-start.sh` | `SessionStart`, including after `compact` and `clear` | Prints the record's state, every sidecar in full, and anything committed or modified after the newest sidecar was written. Claude Code adds it to the new context. Writes nothing. |
| `stop-check.sh` | `Stop` | Refuses the first attempt to finish when a commit or a modified file is newer than the newest sidecar, and says what to update. Lets the second attempt through. |
| `heartbeat.sh` | `PostToolUse` on `Bash\|Edit\|Write` | Every `HEARTBEAT_EVERY` tool calls (10 by default) without a write to engr, and straight after a `git commit`, adds a one-line reminder to record what finished, settled or came up. Writes nothing. |
| `gate.sh` | `PreToolUse` on `Edit\|Write` | Once `HEARTBEAT_GATE` tool calls (20 by default) have gone by without a write to engr, and there is unrecorded work to show for it, refuses the next edit until engr is written to. Needs `heartbeat.sh`, which keeps the count. |

None of them can tell whether what is in engr is *right*. They catch the
mechanically visible half — work the sidecar does not know about — and leave
the rest to the resume drill in `skill/SKILL.md`.

An abrupt end — a turn limit, a crash, a context cleared mid-step — fires no
`Stop` hook. That is the case the start hook's "committed after the newest
sidecar" list exists for, and the one the heartbeat exists to make smaller: an
agent deep in a build-and-fix loop does not stop to write on its own.

Copy them into the project, then add to `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/session-start.sh\"" }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/stop-check.sh\"" }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash|Edit|Write",
        "hooks": [
          { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/heartbeat.sh\"" }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/gate.sh\"" }
        ]
      }
    ]
  }
}
```

All of them need `engr` and `git` on `PATH`, and do nothing in a repository without
`.engr`.

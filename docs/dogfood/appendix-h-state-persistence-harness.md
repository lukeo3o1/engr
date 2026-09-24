# Appendix H — the harness

Everything needed to run this round again. `$SCRATCH` stands for a scratch
directory holding `bin/engr` (a release build of the branch under test) and
`df/` (these scripts); `$DF` is `$SCRATCH/df`. The issue texts are #41 and #42
as they stood on 2026-09-24, saved to `df/shared/41.md` and `df/shared/42.md`.

Run order: `setup-arm.sh <arm> <commit>` then `run-arm.sh <arm>`. Arm A ignores
the commit and keeps the guide on `main`; B, C and D ran the guide at
`b3ec531`; E, F and G at `6423b70`. D, E and G copy their hooks from the working
tree when set up — D before `7a57d2a` fixed the heartbeat, E and G after.
`metrics.py`, `condense.py` and `nudges.py` read what `run-arm.sh` leaves behind;
`pack.sh` builds a blinded packet for an auditor from them.

## The task, as every builder session got it

Session 1 prefixed it with "Start this task.", sessions 2 and 3 with "An earlier session was working on this task and ended without warning. Continue it."

```text
You are an engineer working in this repository: engr, a Rust CLI for engineering records (read AGENTS.md). The directory you are in is your whole world: do not read or write anything outside it, except temporary directories you create yourself.

The task: implement the first slice of ChangeSet as drafted in docs/issues/41.md, following the rollout plan in docs/issues/42.md — Phase 4 first, and Phase 5's `engr mcp` only once ChangeSet works. The draft is deliberately not implementation-ready: you will have to make design decisions it defers, and some questions you should not settle alone. Scope the slice yourself. A small slice that really works — local persistence that survives a restart, a prepare that freezes and revalidates, an apply of at least two mutations that is atomic and crash-safe — is worth more than a wide one that does not. `cargo test --workspace` and `cargo fmt --all -- --check` must pass on every commit.

This repository has adopted engr for its own development (`.engr/`), and the `engr` on your PATH is the tool to use against it. Use it as your project memory throughout, as skill/SKILL.md describes — read that guide before you start. Settled design decisions go in the record, questions you are leaving open go in the backlog, where execution stands goes in a work sidecar, and the plan goes in a collection. Only the `engr` on your PATH may touch this repository's `.engr`: your own in-development build must only ever run against temporary directories, the way the test suite does.

Your session can end at any moment, without warning. Another agent will continue from only this repository and what engr holds — not from anything you said. No human is available: anything that needs one ends at a challenge code, which you report and never answer yourself.

Commit to git as you go, including `.engr` as the guide says. There is no remote; do not push.
```

## The probe's prompt


```text
An earlier agent was working in this repository, and its session ended without warning. You are deciding how the work should be picked up. Do not change anything: no file edits, no commits, no engr mutations. Reading, and building or testing into target/, is fine.

Read skill/SKILL.md first, then use the repository and the `engr` on your PATH to answer these. For every answer, cite where it came from — an engr id or section, a commit, a file, a command's output. Where you cannot tell, say so rather than guessing.

1. What has been decided and is settled?
2. What is still open or undecided?
3. What was the last piece of work in progress when the session ended, and is it finished? How do you know?
4. What exactly would you do next — the first command — and why?
5. What would you have to ask the previous agent, or a human, before continuing?
6. Does anything engr holds disagree with the repository, or look stale? Check before you answer.

Reply with the six answers and nothing else.
```

## The one project Rule


```markdown
---
id: record-mss
applies:
  domains:
    - object
review:
  max_attempts: 3
  on_exhaustion: human_confirmation
---

# A Section says one settled thing, and says enough of it

A Section is checked against each of these:

1. It makes one assertion. If a reader could accept part of it and reject the
   rest, it is more than one Section.
2. The reason it holds is on the page. A clause saying when something applies
   is not a reason.
3. A quantity somebody decided is written out, not referred to.
4. No narration: not how the work got here, what was tried, or what comes next.
5. It does not repeat what another live Section asserts; it references that
   Section instead.
6. Source is pointed at, not copied. An excerpt is the smallest literal the
   assertion needs, never a whole function.

An Object title is a short label naming what the Object is about. Only that
applies to a title; items 1 to 6 do not.
```

## The auditor's brief


```markdown
# Audit brief: how well did an agent keep its working state in engr?

You are auditing one run of an experiment. You have never seen this work and
you know nothing about how the agent was instructed beyond what is in the
material below. Judge only what is there.

## What happened

An agent worked on a real engineering task in a repository — implementing the
first slice of a "ChangeSet" feature in engr, a Rust CLI for engineering
records — and was told to use engr (the tool itself, adopted in that
repository) as its project memory: settled design decisions in the record
(Objects with Sections), open questions in the backlog, where execution stands
in a work sidecar (a summary plus items with states pending/active/done, a
result and commits), and the plan in a collection.

The work ran as **three sessions**. Each was cut off by a turn limit, without
warning, in the middle of whatever it was doing. The next session started with
an empty context: only the repository and what engr held. No human was
available at any point.

After each cut-off, a separate **probe** agent — cold, read-only — was given
the repository as it stood and asked six questions: what is settled, what is
open, what was in progress and is it finished, what exactly to do next, what it
would have to ask, and whether anything engr holds disagrees with the
repository.

## Your material (all under {{DIR}})

- `S1.log`, `S2.log`, `S3.log` — each session, condensed: what the agent said
  and every tool call, with results cut short. **This is your ground truth** for
  what the agent actually knew, decided, did and intended at the moment it was
  cut off. Read the last part of each log closely.
- `state-S1.txt`, `state-S2.txt`, `state-S3.txt` — everything engr and git held
  at each cut-off.
- `probe-1.md`, `probe-2.md`, `probe-3.md` — the probe's answers after each
  cut-off.
- `metrics-S1.json`, `metrics-S2.json`, `metrics-S3.json` — mechanical counts
  (lengths, item states, commits after the sidecar was last written, when in
  the session engr was written to).

## What to judge

The owner of this work stated what they want, and these are your criteria:

1. **Backlog points** are each one clearly defined question (or a minimal
   sufficient statement of one), short, not narrative — it should be clear what
   would settle each one.
2. **Work is broken down into items**, each a single step whose completion can
   be checked; items are **ticked when finished**, with evidence; the state at
   each cut-off matches what had actually happened.
3. **Reasoning in progress**: what a successor needs from the agent's thinking
   (conclusions, paths ruled out and why, hypotheses being tested) is captured
   somewhere findable; the messy path itself is not dumped into engr.
4. **Resume**: a cold successor can pick up correctly from engr and the
   repository alone.

Answer each section below with evidence — quote engr text, cite log turn
numbers (`T37`), cite metrics. Where you cannot tell, say so.

### A. At each cut-off (S1, S2, S3)
- What was the agent doing and intending at the moment of the cut (from the
  log)? What had it decided, found, ruled out, or left open in that session?
- Which of those were in engr at the cut, which were missing, which were wrong
  or stale? List each.

### B. The probes
For each probe: grade each of its six answers against your ground truth —
correct, partly correct, wrong, or hallucinated (stated with confidence but not
supported). Note what the probe could not recover and why. Did it notice what
was stale or unrecorded?

### C. The successor sessions (S2, S3)
- How many turns before the successor did productive work?
- Did it redo work already done, re-decide something already decided, pick up
  a path already ruled out, or contradict an earlier decision? Cite.
- Did it notice and repair what the previous session left missing or stale?

### D. The state itself, against the four criteria
- Backlog points: for each, is it one question, short, with what would settle
  it? Quote the worst and the best.
- Work items: granularity, completion conditions, ticking, evidence. Quote.
- Reasoning: were ruled-out paths and hypotheses kept where a successor would
  find them? Was narrative dumped anywhere?
- Record Sections: do they say one settled thing with its reason?

### E. Cost
Roughly what share of the agent's turns went to keeping engr (reading the
guide, writing records, reviews, subagents) versus the engineering work? Did
the bookkeeping look proportionate, or did it crowd out the work?

### F. Verdict
Score each criterion 1–5 with one sentence of justification, then list the
three failure modes you consider most important in this run, most damaging
first, each with the evidence that shows it.

Write your report to `{{DIR}}/audit.md`. Be concrete and unsentimental; a
short true report beats a long balanced one.
```

## Arm F's CLAUDE.md

The shipped version, `skill/project-instructions.md`, adds four rules the audits asked for.

```markdown
# Working state lives in engr

Your context can be cleared at any moment, without warning. The next session has
only this repository and what engr holds — nothing you said, and nothing in the
harness's own task list.

- **Do not use the harness's task list** (TodoWrite, TaskCreate). The engr work
  sidecar is your task list: one item per step, written "verb + thing; done
  when …", one item active at a time.
- **When a test passes or a commit lands**, before anything else: close the
  item — `item state done`, `item result` saying how you know, `item commit
  --commit HEAD` — then make the next item active.
- **A decision is recorded when it is made**, as a Section through `prepare
  --agent`. Never as a backlog point whose "settled by" is the answer, and never
  as an item to record it later.
- **A question you will not settle now** is a backlog point: what is undecided,
  why it is not obvious, what would decide it. Nothing else.
- **Keep conclusions, not reasoning**: what you found, which path you ruled out
  and why, and what is next. How you got there stays out.
- **A line from a hook that starts with `engr:` is a stop point.** Answer it
  before your next edit.

`skill/SKILL.md` has the reasons and the commands.
```

## Arm G's task-list mirror

Tested and not shipped; see the report.

```python
#!/usr/bin/env python3
"""Claude Code PostToolUse hook (TaskCreate|TaskUpdate): mirror the harness's
own task list into the engr work sidecar.

The task list is where an agent's urge to track its steps goes, and it
disappears with the context. Rather than taking it away, this copies what the
agent does there into execution memory, so the habit is kept and the plan
survives. It needs a sidecar to write into and will not invent one: with none,
it says how to start one. A task marked complete is ticked done in engr, and the
agent is asked for the result, which the task list has no field for.
"""
import json, os, re, subprocess, sys

def engr(*args):
    r = subprocess.run(["engr", *args], capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr

def say(text):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": text}}))
    sys.exit(0)

event = json.load(sys.stdin)
os.chdir(os.environ.get("CLAUDE_PROJECT_DIR", "."))
if not os.path.isdir(".engr"):
    sys.exit(0)

# The sidecar to write into: the most recently updated active one.
rows = []
for line in engr("work", "ls")[1].splitlines():
    parts = line.split()
    if len(parts) >= 6 and re.fullmatch(r"[0-9a-f]{8}", parts[0]) and parts[2] == "active":
        stamp = next((p for p in parts if re.fullmatch(r"\d{4}-\d\d-\d\dT[\d:]+Z", p)), "")
        rows.append((stamp, parts[0], parts[1]))
if not rows:
    say("engr: that task lives only in the harness's list, which is gone when the context is. "
        "Start a work sidecar (engr work start <subject> --summary \"...\") and further tasks will be mirrored into it.")
_, short, kind = max(rows)
subject = short
if kind == "backlog":
    shown = engr("backlog", "show", short, "--format", "json")[1]
    try:
        subject = json.loads(shown)["reference"]
    except (ValueError, KeyError):
        sys.exit(0)

state_file = os.path.join(os.environ.get("TMPDIR", "/tmp"), "engr-mirror-%s.json" % event.get("session_id", "unknown"))
try:
    mapping = json.load(open(state_file))
except (OSError, ValueError):
    mapping = {}

def reset_heartbeat():
    # A mirrored write is a write to engr, so the heartbeat's count of calls
    # since the last one starts again, as it would for one the agent typed.
    path = os.path.join(os.environ.get("TMPDIR", "/tmp"), "engr-heartbeat-%s" % event.get("session_id", "unknown"))
    open(path, "w").write("0\n")

name = event.get("tool_name")
tool_input = event.get("tool_input") or {}
response = json.dumps(event.get("tool_response") or "")

if name == "TaskCreate":
    # The response is {"task": {"id": ...}}; older builds answered in prose.
    raw = event.get("tool_response")
    task_id = raw.get("task", {}).get("id") if isinstance(raw, dict) else None
    if task_id is None:
        found = re.search(r"#(\d+)", response)
        task_id = found.group(1) if found else None
    text = (tool_input.get("subject") or tool_input.get("description") or "").strip()[:160]
    if task_id is None or not text:
        sys.exit(0)
    task_id = str(task_id)
    code, out = engr("work", "item", "add", subject, "--text", text)
    item = re.search(r"work item (\d+)", out)
    if code != 0 or not item:
        say("engr: could not mirror that task into the sidecar: " + out.strip()[:300])
    mapping[task_id] = item.group(1)
    json.dump(mapping, open(state_file, "w"))
    reset_heartbeat()
    say(f"engr: task #{task_id} mirrored as item {item.group(1)} of {subject}. "
        "Items read best as 'verb + thing; done when ...' — revise it if this one does not.")

if name == "TaskUpdate":
    item = mapping.get(str(tool_input.get("taskId")))
    status = tool_input.get("status")
    if not item or status not in ("in_progress", "completed", "pending"):
        sys.exit(0)
    state = {"in_progress": "active", "completed": "done", "pending": "pending"}[status]
    code, out = engr("work", "item", "state", subject, "--item", item, "--state", state)
    if code != 0:
        say("engr: could not mirror that update: " + out.strip()[:300])
    reset_heartbeat()
    if state == "done":
        say(f"engr: item {item} of {subject} is done in the sidecar too. Give it a result now — "
            f"engr work item result {subject} --item {item} --text \"how you know\" — and, if it was committed, "
            f"engr work item commit {subject} --item {item} --commit HEAD.")
sys.exit(0)
```

## setup-arm.sh


```sh
#!/bin/sh
# setup-arm.sh <arm> <skill-commit>
#   arm A: the guide as it is on main.  arm B: the guide at <skill-commit>, and its hooks.
set -eu
SP=$SCRATCH
DF=$SP/df
ARM=$1
BASE=b3e472b
DIR=$DF/$ARM/repo
rm -rf "$DF/$ARM"; mkdir -p "$DF/$ARM"
git clone -q /home/user/engr "$DIR"
cd "$DIR"
git checkout -q -B work "$BASE"
git remote remove origin
# A clone of a working checkout brings its branches and tags with it, and one of
# them carries the guide under test; the builder should see only the base.
git for-each-ref --format='%(refname:short)' refs/heads | grep -vx work | xargs -r git branch -q -D
git tag -l | xargs -r git tag -d >/dev/null
git config user.name "dogfood builder"
git config user.email "builder@dogfood.invalid"
if [ "$ARM" = B ] || [ "$ARM" = C ] || [ "$ARM" = D ] || [ "$ARM" = E ] || [ "$ARM" = F ] || [ "$ARM" = G ]; then
  for f in skill/SKILL.md AGENTS.md skill/hooks/README.md skill/hooks/session-start.sh skill/hooks/stop-check.sh; do
    mkdir -p "$(dirname "$f")"
    git -C /home/user/engr show "$2:$f" > "$f"
  done
  chmod +x skill/hooks/*.sh
  mkdir -p .claude/hooks
  cp skill/hooks/session-start.sh skill/hooks/stop-check.sh .claude/hooks/
  cat > .claude/settings.json <<'JSON'
{
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/session-start.sh\"" } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/stop-check.sh\"" } ] }
    ]
  }
}
JSON
  if [ "$ARM" = D ] || [ "$ARM" = E ]; then
    cp /home/user/engr/skill/hooks/heartbeat.sh /home/user/engr/skill/hooks/gate.sh .claude/hooks/
    python3 - <<'PY'
import json
p = ".claude/settings.json"; s = json.load(open(p))
s["hooks"]["PostToolUse"] = [{"matcher": "Bash|Edit|Write", "hooks": [
    {"type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/heartbeat.sh\""}]}]
s["hooks"]["PreToolUse"] = [{"matcher": "Edit|Write", "hooks": [
    {"type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/gate.sh\""}]}]
# The harness's own task list satisfies the urge to track work and is gone
# when the context is; with it off, that urge has only engr to go to.
s["permissions"] = {"deny": ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "TodoWrite"]}
json.dump(s, open(p, "w"), indent=2); open(p, "a").write("\n")
PY
  fi
  if [ "$ARM" = C ]; then
    cp /home/user/engr/skill/hooks/heartbeat.sh .claude/hooks/heartbeat.sh
    python3 - <<'PY'
import json
p = ".claude/settings.json"; s = json.load(open(p))
s["hooks"]["PostToolUse"] = [{"matcher": "Bash|Edit|Write", "hooks": [
    {"type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/heartbeat.sh\""}]}]
json.dump(s, open(p, "w"), indent=2); open(p, "a").write("\n")
PY
  fi
  if [ "$ARM" = G ]; then
    # The harness task list stays, and is copied into engr as it is used.
    cp /home/user/engr/skill/hooks/heartbeat.sh /home/user/engr/skill/hooks/gate.sh "$DF/shared/mirror-tasks.py" .claude/hooks/
    python3 - <<'PY'
import json
p = ".claude/settings.json"; s = json.load(open(p))
s["hooks"]["PostToolUse"] = [
    {"matcher": "Bash|Edit|Write", "hooks": [
        {"type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/heartbeat.sh\""}]},
    {"matcher": "TaskCreate|TaskUpdate", "hooks": [
        {"type": "command", "command": "python3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/mirror-tasks.py\""}]}]
s["hooks"]["PreToolUse"] = [{"matcher": "Edit|Write", "hooks": [
    {"type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/gate.sh\""}]}]
json.dump(s, open(p, "w"), indent=2); open(p, "a").write("\n")
PY
  fi
  if [ "$ARM" = F ]; then
    # Instructions only: the rules in the always-loaded project file, and none of
    # the hooks that act mid-session (no heartbeat, no gate, task list allowed).
    cp "$DF/shared/CLAUDE.md" CLAUDE.md
  fi
  git add -A && git commit -qm "Adopt the revised agent guide and its hooks"
fi
mkdir -p docs/issues
cp "$DF/shared/41.md" docs/issues/41.md
cp "$DF/shared/42.md" docs/issues/42.md
git add docs/issues && git commit -qm "docs: the ChangeSet draft (#41) and the rollout plan (#42)"
engr init >/dev/null
cp "$DF/shared/record-mss.md" .engr/rules/record-mss.md
git add -A && git commit -qm "engr: adopt engr for this repository's own development"
engr rules ls
git log --oneline | head -4
```

## run-arm.sh


```sh
#!/bin/sh
# run-arm.sh <arm>: three builder sessions, each cut off by a turn limit, with a
# read-only resume probe on a copy of the repository after each cut.
set -u
SP=$SCRATCH
DF=$SP/df
ARM=$1
A=$DF/$ARM
export PATH=$SP/bin:$PATH
export CARGO_BUILD_JOBS=2
MODEL=claude-sonnet-5
ALLOW="Bash Read Write Edit Glob Grep Task Agent TodoWrite TaskCreate TaskGet TaskList TaskUpdate TaskStop ToolSearch NotebookEdit"
DENY="Artifact ArtifactComments ArtifactData SendUserFile PushNotification CronCreate CronDelete CronList ScheduleWakeup WebFetch WebSearch DesignSync SearchMcpRegistry SearchPlugins SuggestConnectors SuggestPluginInstall SuggestSkills ShowOnboardingRolePicker SendMessage ListAgents Workflow ReadNotifications ListConnectors ListPlugins ListSkills ReportFindings Monitor EnterWorktree ExitWorktree Skill"

log() { echo "$(date -u +%H:%M:%S) [$ARM] $*" >> "$A/run.log"; }

session() { # n max-turns
  log "S$1 start (max-turns $2)"
  ( cd "$A/repo" && "$DF/claude-env.sh" -p "$(cat "$DF/shared/prompt-S$1.md")" \
      --model "$MODEL" --max-turns "$2" --permission-mode acceptEdits \
      --allowedTools "$ALLOW" --disallowedTools "$DENY" \
      --output-format stream-json --verbose < /dev/null > "$A/S$1.jsonl" 2> "$A/S$1.err" )
  log "S$1 end exit=$?"
  "$DF/state-dump.sh" "$A/repo" > "$A/state-S$1.txt" 2>&1
  tar -C "$A/repo" --exclude=./target -czf "$A/snap-S$1.tgz" .
}

probe() { # n
  P=$A/probe-$1
  rm -rf "$P"; mkdir -p "$P/repo"
  tar -C "$P/repo" -xzf "$A/snap-S$1.tgz"
  # The probe only reads, so the Stop hook, which asks for writes, is removed;
  # the SessionStart hook stays, because it is part of what this arm resumes with.
  if [ -f "$P/repo/.claude/settings.json" ]; then
    python3 - "$P/repo/.claude/settings.json" <<'PY'
import json, sys
p = sys.argv[1]; s = json.load(open(p)); s.get("hooks", {}).pop("Stop", None); json.dump(s, open(p, "w"), indent=2)
PY
    git -C "$P/repo" update-index --skip-worktree .claude/settings.json
  fi
  log "probe $1 start"
  ( cd "$P/repo" && "$DF/claude-env.sh" -p "$(cat "$DF/shared/prompt-probe.md")" \
      --model "$MODEL" --max-turns 45 --permission-mode acceptEdits \
      --allowedTools "$ALLOW" --disallowedTools "$DENY" \
      --output-format stream-json --verbose < /dev/null > "$P/probe.jsonl" 2> "$P/probe.err" )
  log "probe $1 end exit=$?"
  python3 - "$P/probe.jsonl" > "$P/answer.md" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    d = json.loads(line)
    if d.get("type") == "result":
        print(d.get("result") or "(no result: %s)" % d.get("subtype"))
PY
  rm -rf "$P/repo/target"
}

# run-arm.sh <arm>            the whole chain
# run-arm.sh <arm> probe <n>  one probe again, from snap-S<n>
if [ "${2:-}" = probe ]; then
  probe "$3"
  exit 0
fi
session 1 70
probe 1 &
session 2 70
probe 2 &
session 3 90
probe 3
wait
log "done"
```

## claude-env.sh


```sh
#!/bin/sh
# Run the claude CLI detached from the orchestrating session: same auth and
# proxy, none of the variables that tie it to that session or load its memory.
for v in CLAUDE_CODE_SESSION_ID CLAUDE_CODE_REMOTE_SESSION_ID CLAUDE_CODE_MESSAGING_SOCKET \
  CLAUDE_CODE_MESSAGING_TOKEN CLAUDE_CODE_CHILD_SESSION CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD \
  CLAUDE_ADDITIONAL_DIRECTORIES CLAUDE_CODE_SYNC_SESSION_REFS CLAUDE_CODE_SYNC_SKILLS \
  CLAUDE_CODE_TEE_SDK_STDOUT CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2 CLAUDECODE CLAUDE_PID \
  CLAUDE_AFTER_LAST_COMPACT CLAUDE_CODE_DIAGNOSTICS_FILE CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH \
  CLAUDE_CODE_ARTIFACT_MULTI_FILE CLAUDE_CODE_ARTIFACT_TYPE_CATALOG CLAUDE_CODE_ARTIFACT_ASSETS \
  CLAUDE_CODE_ARTIFACT_TYPE_CLOUD_CREATE CLAUDE_CODE_ARTIFACT_DB CLAUDE_CODE_ARTIFACT_TYPES \
  CLAUDE_CODE_HOLD_UNANSWERED_PARKED_PERMISSION CLAUDE_CODE_REMOTE_SEND_KEEPALIVES \
  CLAUDE_CODE_INCLUDE_PARTIAL_MESSAGES CLAUDE_CODE_SESSION_ATTENDED CLAUDE_AUTO_BACKGROUND_TASKS \
  CLAUDE_CODE_BG_TASKS_REPORT_RUNNING CLAUDE_CODE_WORKER_EPOCH; do unset "$v"; done
export PATH="$SCRATCH/bin:$PATH"
exec claude "$@"
```

## state-dump.sh


```sh
#!/bin/sh
# state-dump.sh <repo>: everything engr and git hold, as text, for the audit.
cd "$1" || exit 1
ids() { awk '$1 ~ /^[0-9a-f]{8}/ {print $1}'; }
sec() { printf '\n======== %s\n' "$*"; }
sec "engr ls --verify"; engr ls --verify 2>&1
sec "engr ls --all --sections"; engr ls --all --sections 2>&1
for id in $(engr ls --all 2>/dev/null | ids); do sec "engr show $id"; engr show "$id" 2>&1; done
sec "engr candidate"; engr candidate 2>&1
sec "engr backlog ls"; engr backlog ls 2>&1
for id in $(engr backlog ls 2>/dev/null | ids); do sec "engr backlog show $id"; engr backlog show "$id" 2>&1; done
sec "engr work ls"; engr work ls 2>&1
engr work ls 2>/dev/null | while read -r id kind _; do
  case "$kind" in
    obj|object) sec "engr work show $id"; engr work show "$id" 2>&1 ;;
    backlog) r=$(engr backlog show "$id" --format json 2>/dev/null | sed -n 's/^  "reference": "\(engr:backlog:[^"]*\)".*/\1/p')
             sec "engr work show $r"; engr work show "$r" 2>&1 ;;
  esac
done
sec "engr collection ls"; engr collection ls 2>&1
for f in .engr/collections/*.json; do [ -e "$f" ] || continue; c=$(basename "$f" .json); sec "engr collection show $c"; engr collection show "$c" 2>&1; done
sec "git log"; git log --format='%h %ad %s' --date=iso -40 --stat 2>&1 | head -300
sec "git status"; git status --short 2>&1
```

## pack.sh


```sh
#!/bin/sh
# pack.sh <arm> <label>: the auditor's material for one arm, under a neutral label.
DF=$DF
A=$DF/$1; O=$DF/audit/$2
rm -rf "$O"; mkdir -p "$O"
for n in 1 2 3; do
  python3 "$DF/condense.py" "$A/S$n.jsonl" > "$O/S$n.log"
  cp "$A/state-S$n.txt" "$O/state-S$n.txt"
  cp "$A/probe-$n/answer.md" "$O/probe-$n.md"
  python3 "$DF/metrics.py" "$A" "$n" > "$O/metrics-S$n.json"
done
# The labels must not name the arm, and paths in the logs do.
sed -i "s#$DF/$1/#<workdir>/#g; s#df/$1/repo#<workdir>/repo#g" "$O"/*
sed "s#{{DIR}}#$O#g" "$DF/shared/auditor-brief.md" > "$O/BRIEF.md"
wc -c "$O"/* | tail -1
```

## condense.py


```python
#!/usr/bin/env python3
"""condense.py <session.jsonl>: a readable log of one headless session.

Every assistant text and tool call, numbered by turn, with tool results cut
short. This is what the auditor reads instead of the raw stream.
"""
import json, sys

def short(s, n):
    s = s if isinstance(s, str) else json.dumps(s)
    s = s.replace("\r", "")
    return s if len(s) <= n else s[:n] + f" …[{len(s) - n} more chars]"

turn = 0
seen = set()
for line in open(sys.argv[1]):
    try:
        d = json.loads(line)
    except json.JSONDecodeError:
        continue
    t = d.get("type")
    if t == "system" and d.get("subtype") == "hook_response":
        print(f"\n[hook {d.get('hook_name')} exit={d.get('exit_code')}]\n{short(d.get('stdout') or '', 1500)}{short(d.get('stderr') or '', 1500)}")
    elif t == "assistant" and not d.get("parent_tool_use_id"):
        # One API response arrives as several stream lines, one per content
        # block; a turn is the response, which is what --max-turns counts.
        mid = d["message"].get("id")
        if mid not in seen:
            seen.add(mid)
            turn += 1
        for c in d["message"].get("content", []):
            if c.get("type") == "text" and c["text"].strip():
                print(f"\n## T{turn} says\n{short(c['text'], 2500)}")
            elif c.get("type") == "tool_use":
                i = c.get("input", {})
                name = c["name"]
                if name == "Bash":
                    body = i.get("command", "")
                elif name in ("Write",):
                    body = f"{i.get('file_path')}\n{short(i.get('content', ''), 1200)}"
                elif name in ("Edit",):
                    body = f"{i.get('file_path')}\n--- old\n{short(i.get('old_string', ''), 500)}\n--- new\n{short(i.get('new_string', ''), 800)}"
                elif name in ("Task", "Agent"):
                    body = f"{i.get('description')}\n{short(i.get('prompt', ''), 2500)}"
                else:
                    body = short(i, 400)
                print(f"\n## T{turn} {name}\n{short(body, 3000)}")
    elif t == "user" and not d.get("parent_tool_use_id"):
        for c in d["message"].get("content", []) if isinstance(d["message"].get("content"), list) else []:
            if c.get("type") == "tool_result":
                content = c.get("content")
                if isinstance(content, list):
                    content = "\n".join(x.get("text", "") for x in content if isinstance(x, dict))
                print(f"-> {short(content or '', 700)}")
    elif t == "result":
        print(f"\n## END subtype={d.get('subtype')} turns={d.get('num_turns')} cost={d.get('total_cost_usd')}\n{short(d.get('result') or '', 3000)}")
```

## metrics.py


```python
#!/usr/bin/env python3
"""metrics.py <arm-dir> <n>: mechanical measures of what session n left behind.

Only what can be counted without judgement. Whether a point is well defined, or
a resume was right, is the auditor's; this says how long, how many, and whether
the repository moved after the handoff was last written.
"""
import json, os, re, subprocess, sys, tarfile, tempfile

ENGR = "$SCRATCH/bin/engr"
arm, n = sys.argv[1], sys.argv[2]

def run(args, cwd):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True).stdout

def js(args, cwd):
    out = run([ENGR] + args + ["--format", "json"], cwd)
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return None

def ids(text):
    return [l.split()[0] for l in text.splitlines() if re.match(r"^[0-9a-f]{8}\b", l)]

def words(s):
    return len(s.split())

def sentences(s):
    return len([x for x in re.split(r"(?<=[.?!])\s+", s.strip()) if x])

m = {}
with tempfile.TemporaryDirectory() as tmp:
    with tarfile.open(os.path.join(arm, f"snap-S{n}.tgz")) as t:
        t.extractall(tmp)
    # record
    objs = ids(run([ENGR, "ls", "--all"], tmp))
    sections = []
    for o in objs:
        d = js(["show", o], tmp) or {}
        for s in d.get("sections", []):
            sections.append({"object": o, "words": words(s.get("text", "")),
                             "by": (s.get("admitted") or {}).get("by"), "role": s.get("role")})
    m["objects"] = len(objs)
    m["sections"] = len(sections)
    m["section_words"] = [s["words"] for s in sections]
    m["candidates_pending"] = len([l for l in run([ENGR, "candidate"], tmp).splitlines() if re.match(r"^[A-Z0-9]{6}\s", l)])
    # backlog
    points = []
    for b in ids(run([ENGR, "backlog", "ls"], tmp)):
        d = js(["backlog", "show", b], tmp) or {}
        for s in d.get("sections", []):
            text = s.get("text", "")
            points.append({"topic": b, "words": words(text), "sentences": sentences(text),
                           "settle_clause": bool(re.search(r"settl|decid|resolved (by|when|once)", text, re.I))})
    m["backlog_points"] = len(points)
    m["backlog_words"] = [p["words"] for p in points]
    m["backlog_sentences"] = [p["sentences"] for p in points]
    m["backlog_with_settle_clause"] = sum(p["settle_clause"] for p in points)
    # work
    sidecars = []
    for line in run([ENGR, "work", "ls"], tmp).splitlines():
        parts = line.split()
        if len(parts) < 2 or not re.match(r"^[0-9a-f]{8}$", parts[0]):
            continue
        subject = parts[0]
        if parts[1] == "backlog":
            d = js(["backlog", "show", parts[0]], tmp) or {}
            subject = d.get("reference", subject)
        w = js(["work", "show", subject], tmp)
        if w:
            sidecars.append(w)
    items = [i for w in sidecars for i in w.get("items", [])]
    m["sidecars"] = len(sidecars)
    m["items"] = len(items)
    m["items_by_state"] = {s: sum(1 for i in items if i["state"] == s) for s in ("pending", "active", "done")}
    m["done_without_result"] = sum(1 for i in items if i["state"] == "done" and not i.get("result"))
    m["done_without_commit"] = sum(1 for i in items if i["state"] == "done" and not i.get("commits"))
    m["items_with_done_condition"] = sum(1 for i in items if re.search(r"done when|done if|until|passes|green", i["text"], re.I))
    m["max_active_in_one_sidecar"] = max([sum(1 for i in w.get("items", []) if i["state"] == "active") for w in sidecars] or [0])
    m["summary_chars"] = [len(w.get("summary") or "") for w in sidecars]
    m["blockers"] = sum(len(w.get("blockers", [])) for w in sidecars)
    # the handoff against the repository
    newest = max([w["updated_at"] for w in sidecars] or [""])
    m["newest_sidecar"] = newest
    if newest:
        after = run(["git", "log", f"--since={newest}", "--format=%h %s", "--", ".", ":!.engr"], tmp)
        m["commits_after_sidecar"] = [l for l in after.splitlines() if l]
    m["dirty_outside_engr"] = [l for l in run(["git", "status", "--porcelain", "--", ".", ":!.engr"], tmp).splitlines() if l]
    m["dirty_inside_engr"] = [l for l in run(["git", "status", "--porcelain", "--", ".engr"], tmp).splitlines() if l]
    m["commits_total"] = int(run(["git", "rev-list", "--count", "HEAD"], tmp).strip() or 0)

# the session itself
turn = 0; seen = set(); writes = []; commits = []; tasks = 0; read_skill = False; hook_outputs = []; result = None
MUT = re.compile(r"\bengr\s+(prepare|confirm|backlog\s+(new|add|revise|merge|produced|consume|rename|subjects)|work\s+|collection\s+(new|add|order|priority|rm|state))")
for line in open(os.path.join(arm, f"S{n}.jsonl")):
    try:
        d = json.loads(line)
    except json.JSONDecodeError:
        continue
    if d.get("type") == "system" and d.get("subtype") == "hook_response":
        hook_outputs.append((d.get("hook_name"), d.get("exit_code")))
    if d.get("type") == "assistant" and not d.get("parent_tool_use_id"):
        mid = d["message"].get("id")
        if mid not in seen:
            seen.add(mid)
            turn += 1
        for c in d["message"].get("content", []):
            if c.get("type") != "tool_use":
                continue
            i = c.get("input", {})
            if c["name"] in ("Task", "Agent"):
                tasks += 1
            if c["name"] == "Read" and str(i.get("file_path", "")).endswith("skill/SKILL.md"):
                read_skill = True
            cmd = i.get("command", "") if c["name"] == "Bash" else ""
            if "skill/SKILL.md" in cmd:
                read_skill = True
            if MUT.search(cmd) and not re.search(r"engr\s+work\s+(ls|show)\b", cmd):
                writes.append(turn)
            if re.search(r"\bgit\s+commit\b", cmd):
                commits.append(turn)
    if d.get("type") == "result":
        result = d.get("subtype")
m["session"] = {"turns": turn, "end": result, "engr_write_turns": writes, "git_commit_turns": commits,
                "subagents": tasks, "read_skill": read_skill, "hooks": hook_outputs,
                "turns_after_last_engr_write": (turn - writes[-1]) if writes else None}
print(json.dumps(m, indent=1))
```

## nudges.py


```python
#!/usr/bin/env python3
"""nudges.py <session.jsonl>: each heartbeat reminder, and whether engr was written within 3 turns."""
import json, re, sys
MUT = re.compile(r"\bengr\s+(prepare|backlog\s+(new|add|revise|merge|produced|consume|rename|subjects)|work\s+(start|summary|item|block|unblock|depend|rm)|collection\s+(new|add|order|priority|rm|state))")
seen=set(); turn=0; nudges=[]; writes=set(); tasks=0
for line in open(sys.argv[1]):
    try: d=json.loads(line)
    except: continue
    if d.get("type")=="system" and d.get("subtype")=="hook_response" and "additionalContext" in (d.get("stdout") or ""):
        kind = "commit" if "commit just landed" in d["stdout"] else "count"
        nudges.append((turn, kind))
    if d.get("type")=="assistant" and not d.get("parent_tool_use_id"):
        mid=d["message"].get("id")
        if mid not in seen: seen.add(mid); turn+=1
        for c in d["message"].get("content",[]):
            if c.get("type")=="tool_use" and c["name"]=="Bash" and MUT.search(re.sub(r'--text "[^"]*"','',c["input"].get("command",""))):
                writes.add(turn)
            if c.get("type")=="tool_use" and c["name"] in ("TaskCreate","TaskUpdate","TodoWrite"):
                tasks+=1
out=[]
for t,k in nudges:
    hit=[w for w in sorted(writes) if t < w <= t+3]
    out.append(f"T{t}:{k}->{'WROTE T'+str(hit[0]) if hit else 'ignored'}")
print(f"turns={turn} native_task_calls={tasks} nudges={len(nudges)} answered={sum('WROTE' in o for o in out)} | "+"; ".join(out))
```


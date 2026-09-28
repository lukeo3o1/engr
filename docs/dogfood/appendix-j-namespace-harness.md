# Appendix J — the namespace harness

What arm H′ ran under, in addition to what `appendix-h-state-persistence-harness.md`
describes. `$SCRATCH` is the scratch directory holding `df/` (these scripts) and
`bin-h2/engr`; the arm's `engr` is copied to `/opt/engr/bin/engr` before a run.

Run order: `setup-v2.sh H2`; build `engr` from `/srv/engr/repo` and copy it to
`/opt/engr/bin/engr`; `engr init` in the sandbox, copy the `record-mss` Rule in,
and commit ("engr: adopt engr for this repository's own development"); then
`run-v2.sh H2 preflight`, which must print `tests passed … failed 0`, `fmt ok`
and `commit ok`; then `run-v2.sh H2`. `run-v2.sh H2 smoke <prompt>` runs a
four-turn session in a copy, for checking what a builder can see from inside.

The files `setup-v2.sh` copies in for H2 are built from this repository:
`shared/v2/SKILL-H2.md` is `skill/SKILL.md` at `6423b70` with the section
"Several Sections of one Object, one review" from `a406c35` added before "Coming
back later", its phrase "in the dogfood" removed; `shared/H/AGENTS.md` is
`AGENTS.md` at `6423b70` with `a406c35`'s sentence about `engr changeset`; and
`shared/H/*.sh` are `skill/hooks/*.sh` at `a406c35`. `shared/v2/40.md` is the
issue text and `shared/v2/record-mss.md` the Rule, as in appendix H.

`/opt/engr/gitconfig` is `/root/.gitconfig` with the `[gpg]`, `[gpg "ssh"]` and
`[commit]` sections and the `signingkey` line removed: the environment signs
commits through a helper under `/tmp`, which the namespace hides.

## The task

Session 1 prefixed it with "Start this task.", sessions 2 and 3 with "An earlier
session was working on this task and ended without warning. Continue it." The
issue text is #40 as it stood on 2026-09-27, saved to `docs/issues/40.md`.

```text
You are an engineer working in this repository: engr, a Rust CLI for engineering records (read AGENTS.md). The directory you are in is your whole world: do not read or write anything outside it, except temporary directories you create yourself.

The task: implement the first slice of the Dynamic UI drafted in docs/issues/40.md — the local, non-authoritative Session and View store underneath it. The maintainer has decided to build this layer now, as a library with a small inspection CLI, ahead of the MCP and Web surfaces the draft describes; `engr mcp`, `engr web`, any frontend, and any ChangeSet integration are out of scope. The draft defers many details to the implementation: you will have to make design decisions it leaves open, and some questions you should not settle alone. Scope the slice yourself. A small slice that really works — Sessions and Views that survive a restart, `ui.update` as an RFC 6902 JSON Patch checked against `expectedRevision` and applied atomically, interaction events with a per-Session sequence, and stale interactions rejected — is worth more than a wide one that does not. `cargo test --workspace` and `cargo fmt --all -- --check` must pass on every commit.

This repository has adopted engr for its own development (`.engr/`), and the `engr` on your PATH is the tool to use against it. Use it as your project memory throughout, as skill/SKILL.md describes — read that guide before you start. Settled design decisions go in the record, questions you are leaving open go in the backlog, where execution stands goes in a work sidecar, and the plan goes in a collection. Only the `engr` on your PATH may touch this repository's `.engr`: your own in-development build must only ever run against temporary directories, the way the test suite does.

Your session can end at any moment, without warning. Another agent will continue from only this repository and what engr holds — not from anything you said. No human is available: anything that needs one ends at a challenge code, which you report and never answer yourself.

Commit to git as you go, including `.engr` as the guide says. There is no remote; do not push.
```

## The added section of the auditor's brief

The brief's opening describes the task as "implementing the first slice of a
local Session and View store for a planned "Dynamic UI" in engr, a Rust CLI for
engineering records, from a draft that leaves many design details open", and
the old *F. Verdict* becomes *G*. Otherwise it is arm E's round's brief.

```markdown
### F. Decisions, one by one
From the logs, list every design decision the agent made — a choice between
alternatives that a successor would need to know, not a routine coding step.
For each, give: the turn it was made (`S1 T23`), the turn it first reached
engr and where (a record Section, a backlog point, a sidecar summary or item,
or never), and whether what engr holds says the same thing with its reason.
Then say how many reached the record as Sections within the session they were
made, how many only later, and how many never; and what recording them cost —
turns, review or cold-read subagents, refused attempts — with the evidence.
```

## Counting writes

`metrics.py`, `writes.py`, `nudges.py` and `decisions.py` no longer count a
command as a write to engr when it runs engr somewhere else — a scratch
workspace — or only asks for `--help`:

```python
TEMP = re.compile(r"\bcd\s+[\"']?(?![^\"' \n]*/df/[A-Z0-9]+/repo\b)(/tmp|/var/tmp|\$|~)|--root\b|\bmktemp\b|--help\b")
```

The exception in it is for the earlier arms, whose sandboxes were themselves
under `/tmp`. `metrics.py` also reports `longest_stretch_without_write`.

## `setup-v2.sh`

```sh
#!/bin/sh
# setup-v2.sh <arm>: the sandbox for arm H2 or E2, at /srv/engr/repo.
#
# Both start from b3e472b, the base every earlier arm used. H2 adds the
# ChangeSet commit's code, protocol and README, scrubbed of the two sentences
# that cite the dogfood, and the guide with its ChangeSet section. E2 is arm E's
# setup exactly. Nothing from the branch under test survives in the object
# store, and nothing names an experiment.
set -eu
SP=$SCRATCH
DF=$SP/df
V=$DF/shared/v2
ARM=$1
SRC=/home/user/engr
BASE=b3e472b
W=/srv/engr
DIR=$W/repo
rm -rf "$W"; mkdir -p "$W"
git clone -q --no-hardlinks "$SRC" "$DIR"
cd "$DIR"
git checkout -q -B work "$BASE"
git remote remove origin
git for-each-ref --format='%(refname)' refs/heads refs/tags refs/remotes | grep -vx refs/heads/work | xargs -r -n1 git update-ref -d
git reflog expire --expire=now --all
git gc -q --prune=now
if git cat-file -e a406c35^{commit} 2>/dev/null || git cat-file -e fd0812d^{commit} 2>/dev/null; then
  echo "branch history survived gc" >&2; exit 1
fi
git config user.name "maintainer"; git config user.email "maintainer@engr.invalid"

if [ "$ARM" = H2 ]; then
  git -C "$SRC" diff "$BASE" a406c35 -- crates protocol README.md | git apply
  python3 - <<'PY'
p = "crates/engr/src/changeset.rs"; s = open(p).read()
old = """//! A Rule Review costs a reader who never saw the draft, and an agent recording
//! eight decisions about one piece of work paid for eight of them — about half
//! of a dogfood session, and the reason the other sessions recorded none. The
//! growth rule names the signal for more than one action per admission ("one
//! piece of work needs the same object prepared … three times over, and the
//! human says so"), and both halves of it fired."""
new = """//! A Rule Review costs a reader who never saw the draft, and an agent recording
//! several decisions about one piece of work paid for one review each. The
//! growth rule names the signal for more than one action per admission ("one
//! piece of work needs the same object prepared … three times over, and the
//! human says so"), and it fired."""
assert old in s; open(p, "w").write(s.replace(old, new))
p = "protocol/PROTOCOL.md"; s = open(p).read()
old = """A Rule Review is paid for with a reader who never saw the draft, and it is paid
per admission. In the dogfood of 2026-09-24 the arm that recorded its decisions
as Sections spent about 75 turns and ten reviewer subagents on eight of them,
most in its last session; another spent 17 turns and six subagents on one title
and one sentence; five of the seven admitted no Section at all. A review priced
per Section pushes decisions to the end of the work or out of the record, which
is the opposite of what a Rule is for. So the review — and only the review — can
be shared."""
new = """A Rule Review is paid for with a reader who never saw the draft, and it is paid
per admission. Recording several decisions about one piece of work cost one
review each, and a review priced per Section pushes decisions to the end of the
work or out of the record, which is the opposite of what a Rule is for. So the
review — and only the review — can be shared."""
assert old in s; open(p, "w").write(s.replace(old, new))
PY
  git add -A
  git commit -qm "changeset: several Section mutations of one Object, reviewed once" \
    -m "An ordered list of Section mutations to one existing Object, kept under .engr/local until applied, and admitted under one Agent Rule Review as one record per step, published together."
fi

# The guide, the hooks and the harness settings, as arm E had them.
mkdir -p skill/hooks .claude/hooks
git -C "$SRC" show 6423b70:skill/hooks/README.md > skill/hooks/README.md
if [ "$ARM" = H2 ]; then
  cp "$V/SKILL-H2.md" skill/SKILL.md
  python3 - "$DF/shared/H/AGENTS.md" <<'PY'
import sys
s = open(sys.argv[1]).read()
for anchor, line in (
    ("crates/engr/src/backlog.rs      unresolved staging: subjects, produced, reconciliation\n",
     "crates/engr/src/changeset.rs    several Section mutations of one Object, reviewed once\n"),
    ("crates/engr/tests/backlog.rs    what staging is, and what it is not\n",
     "crates/engr/tests/changeset.rs  what one review over several mutations may admit\n")):
    assert anchor in s; s = s.replace(anchor, anchor + line)
open("AGENTS.md", "w").write(s)
PY
  HOOKS="$DF/shared/H"
  for f in session-start.sh stop-check.sh; do cp "$HOOKS/$f" skill/hooks/; done
  for f in session-start.sh stop-check.sh heartbeat.sh gate.sh; do cp "$HOOKS/$f" .claude/hooks/; done
else
  git -C "$SRC" show 6423b70:skill/SKILL.md > skill/SKILL.md
  git -C "$SRC" show 6423b70:AGENTS.md > AGENTS.md
  for f in session-start.sh stop-check.sh; do git -C "$SRC" show 6423b70:skill/hooks/$f > skill/hooks/$f; done
  for f in session-start.sh stop-check.sh heartbeat.sh gate.sh; do git -C "$SRC" show 6423b70:skill/hooks/$f > .claude/hooks/$f; done
fi
chmod +x skill/hooks/*.sh .claude/hooks/*.sh
cat > .claude/settings.json <<'JSON'
{
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/session-start.sh\"" } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/stop-check.sh\"" } ] }
    ],
    "PostToolUse": [
      { "matcher": "Bash|Edit|Write", "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/heartbeat.sh\"" } ] }
    ],
    "PreToolUse": [
      { "matcher": "Edit|Write", "hooks": [ { "type": "command", "command": "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/gate.sh\"" } ] }
    ]
  },
  "permissions": { "deny": ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "TodoWrite"] }
}
JSON
git add -A && git commit -qm "Adopt the revised agent guide and its hooks"
mkdir -p docs/issues && cp "$V/40.md" docs/issues/40.md
git add docs/issues && git commit -qm "docs: the Dynamic UI draft (#40)"
git config user.name "engr developer"; git config user.email "developer@engr.invalid"
```

## `run-v2.sh`

```bash
#!/bin/bash
# run-v2.sh <arm>: three builder sessions, each cut off by a turn limit, and a
# read-only probe after each cut — every one in its own PID and mount namespace.
#
# Inside, the builder sees its repository at /srv/engr/repo, the engr under test
# on PATH, and nothing of the experiment: /home/user (the repository under
# test), /tmp (this harness, every earlier arm, their transcripts' temp dirs),
# the Claude transcript store and the probes' copies are empty mounts, other
# processes are invisible, and the prompt arrives on stdin rather than argv.
set -u
SP=$SCRATCH
DF=$SP/df
V=$DF/shared/v2
ARM=$1
A=$DF/$ARM
REPO=/srv/engr/repo
COPIES=/var/lib/engr-copies
mkdir -p "$A" "$COPIES"
# The same variables claude-env.sh drops: nothing ties a builder to this session.
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
export PATH=/opt/engr/bin:$(printf '%s' "$PATH" | tr ':' '\n' | grep -v scratchpad | paste -sd:)
export CARGO_BUILD_JOBS=2
MODEL=claude-sonnet-5
ALLOW="Bash Read Write Edit Glob Grep Task Agent TodoWrite TaskCreate TaskGet TaskList TaskUpdate TaskStop ToolSearch NotebookEdit"
DENY="Artifact ArtifactComments ArtifactData SendUserFile PushNotification CronCreate CronDelete CronList ScheduleWakeup WebFetch WebSearch DesignSync SearchMcpRegistry SearchPlugins SuggestConnectors SuggestPluginInstall SuggestSkills ShowOnboardingRolePicker SendMessage ListAgents Workflow ReadNotifications ListConnectors ListPlugins ListSkills ReportFindings Monitor EnterWorktree ExitWorktree Skill"

log() { echo "$(date -u +%H:%M:%S) [$ARM] $*" >> "$A/run.log"; }

# nsrun <source-repo> <command...>: run a command where a builder would.
#
# The environment signs commits through a helper under /tmp, which the empty
# /tmp hides, so git gets the same settings without signing — the first run of
# this harness lost every commit to it, and the preflight now runs here too.
nsrun() {
  unshare -m -p -f --mount-proc --propagation private /bin/bash -c '
    set -e
    src=$1; shift
    if [ "$src" != /srv/engr/repo ]; then
      mount -t tmpfs tmpfs /srv && mkdir -p /srv/engr/repo && mount --bind "$src" /srv/engr/repo
    fi
    mount -t tmpfs tmpfs /var/lib/engr-copies
    mount --bind /opt/engr/gitconfig /root/.gitconfig
    mount -t tmpfs tmpfs /tmp
    mount -t tmpfs tmpfs /home/user
    for d in projects tasks shell-snapshots session-env sessions backups todos; do
      [ -d /root/.claude/$d ] && mount -t tmpfs tmpfs /root/.claude/$d
    done
    cd /srv/engr/repo
    exec "$@"' _ "$@"
}

# isolated <source-repo> <prompt-file> <max-turns> <out-prefix>
isolated() {
  nsrun "$1" claude -p --model "$MODEL" --max-turns "$3" --permission-mode acceptEdits \
      --allowedTools "$ALLOW" --disallowedTools "$DENY" --output-format stream-json --verbose \
      < "$2" > "$4.jsonl" 2> "$4.err"
}

ended() { # the result line of a stream: turn cap, a voluntary finish, or something else
  python3 -c '
import json, sys
r = []
for l in open(sys.argv[1]):
    try: d = json.loads(l)
    except ValueError: continue
    if d.get("type") == "result": r.append(d)
print(r[-1].get("subtype") if r else "no-result")' "$1"
}

session() { # n max-turns
  log "S$1 start (max-turns $2)"
  isolated "$REPO" "$V/prompt-S$1.md" "$2" "$A/S$1"
  local end; end=$(ended "$A/S$1.jsonl")
  case "$end" in error_max_turns|success) log "S$1 end $end" ;; *) log "S$1 end $end — INVALID: not ended by the turn cap" ;; esac
  "$DF/state-dump.sh" "$REPO" > "$A/state-S$1.txt" 2>&1
  tar -C "$REPO" --exclude=./target -czf "$A/snap-S$1.tgz" .
}

probe() { # n
  local P=$A/probe-$1 C=$COPIES/$1/repo
  rm -rf "$P" "$COPIES/$1"; mkdir -p "$P" "$C"
  tar -C "$C" -xzf "$A/snap-S$1.tgz"
  python3 - "$C/.claude/settings.json" <<'PY'
import json, sys
p = sys.argv[1]; s = json.load(open(p)); s.get("hooks", {}).pop("Stop", None); json.dump(s, open(p, "w"), indent=2)
PY
  git -C "$C" update-index --skip-worktree .claude/settings.json
  log "probe $1 start"
  isolated "$C" "$V/prompt-probe.md" 45 "$P/probe"
  log "probe $1 end $(ended "$P/probe.jsonl")"
  python3 - "$P/probe.jsonl" > "$P/answer.md" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    d = json.loads(line)
    if d.get("type") == "result":
        print(d.get("result") or "(no result: %s)" % d.get("subtype"))
PY
  rm -rf "$COPIES/$1"
}

if [ "${2:-}" = probe ]; then probe "$3"; exit 0; fi
if [ "${2:-}" = preflight ]; then # the suite, fmt and a commit, where the builder will run them
  rm -rf "$COPIES/preflight"; mkdir -p "$COPIES/preflight"; cp -a "$REPO" "$COPIES/preflight/repo"
  nsrun "$COPIES/preflight/repo" bash -c '
    cargo test --workspace 2>&1 | grep -E "^test result" | awk "{p+=\$4; f+=\$6} END {print \"tests passed\", p, \"failed\", f; exit (f>0)}" &&
    cargo fmt --all -- --check && echo "fmt ok" &&
    d=$(mktemp -d) && git -C "$d" init -q && git -C "$d" commit -q --allow-empty -m probe && echo "commit ok"'
  status=$?; rm -rf "$COPIES/preflight"; exit $status
fi
if [ "${2:-}" = smoke ]; then # what a builder can see, asked from inside
  rm -rf "$COPIES/smoke"; mkdir -p "$COPIES/smoke"; cp -a "$REPO" "$COPIES/smoke/repo"
  isolated "$COPIES/smoke/repo" "$3" 4 "$A/smoke"; rm -rf "$COPIES/smoke"; exit 0
fi
log "claude $(claude --version 2>/dev/null) / $(engr --version)"
session 1 70
probe 1 &
session 2 70
probe 2 &
session 3 90
probe 3
wait
log "done"
```

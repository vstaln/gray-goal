# gray-goal

Persistent goals + an **independent completion auditor** for the
[gray](https://github.com/) agent harness — a Rust port of the pi extension
[`pi-goal-x`](https://www.npmjs.com/package/pi-goal-x) distilled to its core:
a `/goal` command, an ordered checklist the agent maintains, per-turn context
injection, a completion nudge, and an audit that is **never self-graded**.

Goals live as human-editable markdown docs in `~/.gray/goals/<id>.md` and
survive sessions.

## Install

```bash
cargo build --release
gray plugin install target/release/gray-goal   # or: register the binary
```

`gray-goal manifest` is the install probe; `gray plugin check
target/debug/gray-goal` runs the conformance suite (manifest handshake,
tool round-trip, concurrency, command dispatch, shutdown).

## Setup

None — no credentials, no gateway service. The only knob is the auditor
command itself:

| Env var | Meaning |
|---|---|
| `GRAY_GOAL_AUDITOR` | `sh -c` command used instead of `gray --bare -p` (tests, custom runners). Gets `GRAY_GOAL_FILE`, `GRAY_GOAL_CWD`, `GRAY_GOAL_ID`. Must end output with `VERDICT: PASS` or `VERDICT: FAIL`. |
| `GRAY_BIN` | Path to the gray binary for the default auditor (default: `gray`). |
| `GRAY_HOME` | Plugin state root (default `~/.gray`). |

## Commands

```text
/goal <desc>    create a goal; the session agent drafts the ordered checklist
/goal           status of the current goal
/goal list      every goal (* = active)
/goal show [id] doc summary, items, last audit lines, file path
/goal done [n]  check item n — or, bare/with an id, mark the goal done
/goal check <n> check item n explicitly
/goal drop [id] drop a goal (file kept, status=dropped)
/goal audit [id] independent audit of every checked item → pass/fail
```

`gray goal …` works identically outside a session.

## The `goal` tool (model-facing)

`status` · `set_items{items[]}` · `add_item{item}` · `check_item{n}` /
`uncheck_item{n}` · `complete` · plus optional `goal_id`.

`complete` is the claim gate: with open items it is **refused** — the open
list is returned and a one-shot `agent/before_start` reminder fires on the
next run. With every item checked it marks the goal done and points at
`/goal audit`. Item edits on a done goal reopen it.

## How the audit works

`/goal audit` marks the goal `audit: running`, then spawns a detached
`gray-goal audit-run <id>` (30s `command/run` TTL is never at risk). The
child invokes `gray --bare -p <reviewer prompt>` — a fresh agent with
blocking bash and **no plugins** — instructing it to verify every *checked*
item against the actual repo (`ITEM n: PASS|FAIL|OPEN` lines, then
`VERDICT: PASS|FAIL`). The verdict + summary are committed to the goal doc's
frontmatter and `## Audit log`; full auditor output lands in
`<id>.audit.log`, spawn output in `<id>.audit-run.log`. `prompt/context` and
`/goal show` surface the verdict; a FAIL is flagged in next turn's context.

## Limits

- The auditor needs a working `gray` (provider creds) or
  `GRAY_GOAL_AUDITOR`; audits time out after 600s → `audit: error`.
- "Current goal" = most recently updated non-dropped goal; there is no
  explicit multi-active-goal switcher beyond `show/drop/audit <id>`.
- The checklist is only as honest as the agent that checks it — that's why
  grading is delegated to an independent process, not this plugin.
- Sidecar protocol 1.1: JSON-lines stdio, `prompt/context`,
  `agent/before_start`, `command/run`, `tool/call`, `event/notify`,
  `plugin/shutdown`.

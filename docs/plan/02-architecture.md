# Architecture

This file is the contract between the server and the UI. Changing a shape or a route here is a
decision, not a detail: stop and ask first.

## 1. Shape

```text
browser tab or mac app window
  │  static files            GET /               (HTML, CSS, JS modules, vendored libs)
  │  JSON API                /api/v1/...         (cookie or bearer key)
  │  event stream (SSE)      GET /api/v1/stream  (session and pull updates)
  │  terminal socket (WS)    GET /api/v1/sessions/{id}/terminal
  ▼
syncaidync serve   (one Rust process, 127.0.0.1 only)
  ├─ registry      sessions in memory, saved to ~/.syncaidync/sessions.json
  ├─ terminals     one PTY per live session, 256 KiB replay buffer each
  ├─ git, gh       run as child processes
  └─ one-shot      runs an agent CLI once for a plan or takeaways
        ▲
        │  POST /agent/v1/signal, /agent/v1/usage  (per-session token)
agent CLI in the PTY ──hook──▶ `syncaidync signal` (same binary, tiny HTTP client)
```

## 2. Repo layout

```text
Cargo.toml                    workspace
Cargo.lock                    committed
LICENSE                       MIT
README.md
AGENTS.md                     short rules for agents working in this repo
THIRD_PARTY_NOTICES.md        vendored libraries and their licences
crates/
  syncaidync-core/            pure logic, no I/O (serde, serde_json, sha2 only)
    src/lib.rs
    src/model.rs              Session, Workspace, Blocked, Usage, ToolUse, AgentId, State
    src/signal.rs             Signal enum and the reducer
    src/agents/{mod,claude,codex,pi,cursor,shell}.rs
    src/startup.rs            startup-prompt detection on terminal text
    src/ring.rs               byte ring buffer
    src/diff.rs               unified diff parser
    src/imports.rs            import extraction and resolution
    src/changemap.rs          groups, map, digests, stamp
    src/readplan.rs           prompt builder and reply validator
    src/notes.rs              review data shapes
    src/pulls.rs              PR reference parsing, draft-review payload
    src/takeaways.rs          prompt, reply parser, file merge
    src/transcript.rs         transcript condenser
    src/version.rs            version compare
    src/time.rs               civil date from epoch millis
  syncaidync/                 the binary
    build.rs                  rerun-if-changed for ../../web
    src/main.rs               argument parsing, subcommands
    src/config.rs             home, port, key, roots
    src/http/{mod,router,auth,sse,ws,static_files,error,json}.rs
    src/api/{sessions,files,changes,pulls,takeaways,discovery,misc,agent,margins}.rs
    src/registry.rs
    src/pty.rs
    src/store.rs              atomic JSON files
    src/workspaces.rs         worktree, folder, scratch
    src/discovery.rs          repo scan, agent history scan
    src/files.rs              tree, read, write with containment
    src/margins.rs            workspace `.syncaidync/margins` sidecar
    src/git.rs  src/gh.rs     child-process helpers
    src/changes.rs            gathers a ChangeSet from a folder or a commit
    src/pulls.rs              pull request list, fetch, draft review
    src/takeaways.rs          destinations, reading transcripts, writing files
    src/oneshot.rs            run an agent once, get text back
    src/hookcli.rs            `signal` and `usage` subcommands
    src/links.rs  src/update.rs  src/markdown.rs  src/ids.rs  src/load.rs
    tests/                    integration tests (start the server on a free port)
web/
  index.html
  favicon.svg
  css/{tokens,base,chrome,board,canvas,dialogs,workspace,files,changes,markdown,pulls}.css
  js/main.js
  js/lib/                     pure modules, no DOM (tested with node --test)
  js/components/              custom elements, one per file
  vendor/xterm/  vendor/codemirror/  vendor/uigradients/
  test/*.test.js
mac/                          Swift wrapper app (WO-19)
scripts/{check.sh,license-check.sh,dev.sh,vendor-xterm.sh,vendor-codemirror.sh,vendor-uigradients.sh}
docs/plan/
```

## 3. Dependencies

These exact crates and features were resolved and checked on 2026-11-17: 65 crates, every one
offers MIT except the approved `unicode-ident` exception. Do not add crates or features.

`crates/syncaidync-core/Cargo.toml`

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
```

`crates/syncaidync/Cargo.toml`

```toml
[dependencies]
syncaidync-core = { path = "../syncaidync-core" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "sync", "time", "process", "signal", "fs"] }
hyper = { version = "1", features = ["http1", "server"] }
hyper-util = { version = "0.1", features = ["tokio"] }
http-body-util = "0.1"
tokio-tungstenite = { version = "0.29", default-features = false, features = ["handshake"] }
futures-util = { version = "0.3", default-features = false, features = ["sink", "std"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
pty-process = { version = "0.5", features = ["async"] }
pulldown-cmark = { version = "0.13", default-features = false, features = ["html"] }
include_dir = "0.7"
getrandom = "0.3"
sha2 = "0.10"
libc = "0.2"
```

What we write ourselves instead of pulling a crate: the router, argument parsing, UUIDs (16
random bytes from `getrandom`, version and variant bits set), hex encoding, dates, percent
decoding of query strings, the tiny HTTP client used by `syncaidync signal`, and constant-time
comparison.

There is no TLS and no HTTP client crate. Anything that needs GitHub goes through `gh` or `git`.

## 4. Command line and environment

```text
syncaidync [serve] [--port N] [--no-open] [--parent-pid PID]
syncaidync signal --agent <id>      reads one JSON object on stdin, reports it, exits 0
syncaidync usage  --agent <id>      same, and prints a one-line status for the agent's status bar
syncaidync --version | --help
```

- `serve` is the default. It prints the loopback address and opens the authenticated URL
  in the default browser unless `--no-open` (macOS `open`, Linux `xdg-open`).
  Wrappers using `--no-open` supply their own launch key.
- If the port is taken, exit 1 with: `Port 7717 is in use. Is SyncAiDync already running? Try --port.`
- `--parent-pid`: exit when that process is gone (checked every 2 s). Used by the mac app.
- Ctrl-C or SIGTERM: stop every terminal (section 9.4), save, exit 0.
- `signal` and `usage` never fail loudly: any error is swallowed, the exit code is 0, and they
  give up after 2 seconds. A broken board must never break an agent.

| Variable | Meaning |
|---|---|
| `SYNCAIDYNC_HOME` | Home folder. Default `~/.syncaidync`. |
| `SYNCAIDYNC_PORT` | Port. Default 7717. `--port` wins. |
| `SYNCAIDYNC_KEY` | Launch key to use instead of a random one (the mac app sets this). At least 32 characters. |
| `SYNCAIDYNC_ROOTS` | Colon-separated folders to scan for repos, replacing the defaults. |
| `SYNCAIDYNC_WEB_DIR` | Serve the UI from this folder on disk instead of the embedded copy. For development. |
| `SYNCAIDYNC_DEV_SHELL` | `1` adds a `shell` agent that runs your login shell. For development and tests. |
| `SYNCAIDYNC_DEBUG_SIGNALS` | `1` appends every raw signal payload to `logs/signals.jsonl`. |
| `SYNCAIDYNC_NO_UPDATE_CHECK` | `1` turns off the daily check for a newer release. |
| `SYNCAIDYNC_BIN_CLAUDE`, `_CODEX`, `_PI`, `_CURSOR` | Full path to an agent CLI, instead of searching `PATH`. |
| `SYNCAIDYNC_GOAL_TICK_MS` | How often goal cost and time limits are checked while an agent works (default 15000). For tests. |

Set for every process started inside a session terminal:

| Variable | Value |
|---|---|
| `SYNCAIDYNC` | `1` |
| `SYNCAIDYNC_URL` | `http://127.0.0.1:<port>` |
| `SYNCAIDYNC_SESSION` | the session id |
| `SYNCAIDYNC_SIGNAL_TOKEN` | that session's callback token (not the launch key) |
| `TERM` | `xterm-256color` |
| `COLORTERM` | `truecolor` |
| `CLICOLOR` | `1`, unless the user's environment already sets `CLICOLOR` or `NO_COLOR` |

## 5. Home folder

```text
~/.syncaidync/                 created with mode 0700
  sessions.json                { "version": 1, "sessions": [Session, ...] }
  pulls.json                   { "version": 1, "pulls": [Pull, ...] }
  reviews.json                 { "version": 1, "reviews": { "<key>": Review } }
  config.json                  { "version": 1, "update": { "checked_at": 0, "latest": null } }
  worktrees/<repo-name>/<branch-slug>/
  scratch/<session-id>/
  notes/everywhere.md          takeaways with scope "note"
  logs/<session-id>.log        raw terminal output, capped at 16 MiB (oldest half dropped)
  logs/signals.jsonl           only with SYNCAIDYNC_DEBUG_SIGNALS=1
  goals/<session-id>.json      GoalRecord rounds and check output (WO-21)
  run/<session-id>/            per-session generated files (for example claude-settings.json)
```

- Writes are atomic: write `name.tmp` in the same folder, `fsync`, rename over `name`.
- Saves are debounced: at most one write per file per 250 ms, plus a final write on shutdown.
- A file that fails to parse is renamed to `name.broken-<millis>` and the server starts empty.
  Say so on stderr.
- Callback tokens are not saved. They are made fresh each time a session's process starts.

## 6. Security

1. **Bind:** `127.0.0.1` only. Never `0.0.0.0`.
2. **Host check, every request:** the `Host` header must be `127.0.0.1:<port>`,
   `localhost:<port>` or `[::1]:<port>`. Otherwise 403 `bad_host`.
3. **Launch key:** 32 random bytes as 64 hex characters (or `SYNCAIDYNC_KEY`). Kept in memory.
4. **Getting a cookie:** the UI reads `#key=...` from the URL fragment, immediately removes it
   from the address bar (`history.replaceState`), and calls `POST /api/v1/auth` with
   `{ "key": "..." }`. On a match the server answers 204 with
   `Set-Cookie: syncaidync_<port>=<key>; HttpOnly; SameSite=Strict; Path=/`.
5. **Credential on `/api/v1/*`** (all routes except `POST /api/v1/auth`): the cookie, or
   `Authorization: Bearer <key>`. Otherwise 401 `unauthorized`. Compare in constant time.
6. **Origin check:** for every request that is not `GET`/`HEAD`, and for WebSocket upgrades, an
   `Origin` header, if present, must be `http://127.0.0.1:<port>` or `http://localhost:<port>`.
   Otherwise 403 `bad_origin`.
7. **Agent callbacks** (`/agent/v1/*`): `Authorization: Bearer <signal token>` matching the
   session named in `?session=`. These routes accept nothing else and can only report signals
   and usage for that one session.
8. **Static files** need no credential (they hold no data) but still pass the Host check.
9. **Headers on every HTML response:**
   `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ws://127.0.0.1:<port> ws://localhost:<port>; frame-ancestors 'none'; base-uri 'none'; form-action 'none'`
   plus `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer` on everything.
   (`'unsafe-inline'` for styles is needed by the terminal and editor libraries.)
10. **Request bodies** are capped at 4 MiB (413 `too_large`).
11. **Paths from the client** are always relative to a session workspace and go through
    `files.rs` containment (section 11). There is no API that reads an arbitrary path.
12. The key is never logged. Request logging, if any, prints method, path without query, status.

## 7. Data shapes

All JSON is `snake_case`. All times are integer milliseconds since the Unix epoch. `null` means
"not known" or "none"; fields are always present.

### 7.1 Session

```json
{
  "id": "0b9c0c5e-6d0b-4f7e-9a53-3a0f3f1f7c11",
  "title": "Fix the login redirect",
  "agent": "claude",
  "model": null,
  "autonomy": false,
  "prompt": "The login page redirects twice. Find out why.",
  "state": "blocked",
  "blocked": { "kind": "approval", "message": "Bash: rm -rf build" },
  "snoozed": false,
  "archived": false,
  "live": true,
  "exit_code": null,
  "workspace": {
    "kind": "worktree",
    "path": "/Users/me/.syncaidync/worktrees/shop/fix-login-redirect",
    "repo": "/Users/me/code/shop",
    "branch": "dync/fix-login-redirect",
    "base": "main",
    "base_commit": "4f2a9c1e...",
    "owned": true
  },
  "created_at": 1795000000000,
  "updated_at": 1795000050000,
  "state_since": 1795000040000,
  "usage": { "cost_usd": 0.42, "context_pct": 31, "tokens": null },
  "tools": [{ "name": "Bash", "detail": "cargo test", "at": 1795000030000 }],
  "agent_session": "the id the agent CLI uses to resume",
  "transcript": "/Users/me/.claude/projects/.../abc.jsonl",
  "pull": null,
  "goal": null
}
```

- `agent`: `claude`, `codex`, `pi`, `cursor`, and `shell` when the dev shell is on.
- `state`: `launching`, `working`, `blocked`, `ready`, `ended`.
- `blocked` is non-null exactly when `state` is `blocked`. `kind`: `approval`, `question`,
  `startup`. `message` may be empty.
- `live`: a terminal process is attached. `ended` implies `live: false`.
- `workspace.kind`: `worktree`, `folder`, `scratch`. `owned` is true when SyncAiDync created the
  folder and may delete it. `repo`, `branch`, `base`, `base_commit` are null when not a git
  repository. `base_commit` is what the Changes tool diffs against: for a worktree, the base
  branch's commit when the worktree was made; for a folder inside a repo, `HEAD` when the
  session was created.
- `tools`: newest last, at most 20. `detail` is at most 160 characters.
- `usage` is null until the agent reports it. `cost_usd` and `context_pct` come from agents
  that report them (Claude). `tokens` is a running sum of per-request token counts (Codex, Pi).
- `pull` is the id of a Pull when the session was started from one.
- `goal` is a Goal (section 7.9) when the session was started with one.

### 7.2 Meta

```json
{
  "name": "SyncAiDync",
  "version": "0.1.0",
  "home": "/Users/me/.syncaidync",
  "platform": "macos",
  "git": true,
  "gh": true,
  "agents": [
    {
      "id": "claude", "label": "Claude Code", "installed": true, "bin": "/opt/homebrew/bin/claude",
      "observes": "signals", "models": ["opus", "sonnet", "haiku"], "history": true
    }
  ]
}
```

`observes` is `signals` (the agent reports through hooks) or `activity` (state is inferred from
terminal output; the UI shows "approximate" beside the state).

### 7.2a Load

Live process snapshot for the top-bar chip. Not persisted. `agents` sums every live session's
PTY process group (the child is the session leader). Cost stays on each Session's `usage`.

```json
{
  "host": { "cpu_pct": 1.2, "rss_bytes": 67108864 },
  "agents": { "cpu_pct": 38.0, "rss_bytes": 1288490188, "sessions": 2 }
}
```

### 7.3 ChangeSet

```json
{
  "stamp": "sha256 of the raw diff text and the untracked file list",
  "base": { "label": "main", "commit": "4f2a9c1e..." },
  "head": { "label": "dync/fix-login-redirect", "commit": "9e1b...", "dirty": true },
  "truncated": false,
  "files": [
    {
      "path": "src/auth/redirect.ts",
      "old_path": null,
      "status": "modified",
      "binary": false,
      "added": 12,
      "removed": 3,
      "group": "packages/web",
      "language": "typescript",
      "digest": "sha256 of this file's hunk lines",
      "hunks": [
        {
          "id": "src/auth/redirect.ts#a1b2c3d4",
          "old_start": 10, "old_lines": 8, "new_start": 10, "new_lines": 17,
          "heading": "export function redirectAfterLogin(",
          "lines": [
            { "kind": "context", "old": 10, "new": 10, "text": "  const next = params.get('next');" },
            { "kind": "remove",  "old": 11, "new": null, "text": "  return go(next);" },
            { "kind": "add",     "old": null, "new": 11, "text": "  return go(safe(next));" }
          ]
        }
      ]
    }
  ],
  "groups": [{ "name": "packages/web", "manifest": "package.json", "files": ["src/auth/redirect.ts"] }],
  "map": {
    "nodes": [
      { "path": "src/auth/redirect.ts", "changed": true },
      { "path": "src/pages/login.tsx", "changed": false }
    ],
    "edges": [{ "from": "src/pages/login.tsx", "to": "src/auth/redirect.ts" }]
  }
}
```

- `status`: `added`, `modified`, `deleted`, `renamed`.
- A file is identified by `path`. A hunk id is `<path>#<first 8 hex of sha256 of its lines>`; if
  two hunks in a file collide, the later one gets `-2`, `-3`. This keeps notes and plans attached
  across refreshes when a hunk did not change.
- An edge `from → to` means "`from` imports `to`".
- `truncated` is true when a limit in section 12 cut something off.

### 7.4 Review

One per session (`s:<session id>`) or pull (`p:<pull id>`).

```json
{
  "reviewed": { "src/auth/redirect.ts": "<file digest when it was ticked>" },
  "notes": [
    {
      "id": "uuid", "path": "src/auth/redirect.ts", "side": "new", "start": 11, "end": 11,
      "quote": "  return go(safe(next));", "text": "What does safe() do with an absolute URL?",
      "sent": false, "created_at": 1795000060000
    }
  ],
  "overall": "",
  "plan": null,
  "updated_at": 1795000060000
}
```

- A file counts as reviewed only while its current `digest` equals the stored one.
- A note with `path: null` is not possible; the overall note is `overall`. A file-level note has
  `start: null`, `end: null`, `side: "new"`, `quote: ""`.
- `plan` is a ReadPlan or null. The UI never writes `plan`; only the plan endpoint does.

### 7.5 ReadPlan

```json
{
  "stamp": "the ChangeSet stamp it was made for",
  "agent": "claude", "model": null, "created_at": 1795000070000,
  "summary": "One paragraph on what this change does.",
  "checkpoints": [
    {
      "id": "c1", "title": "Redirect target is now validated",
      "why": "This is the behaviour change everything else supports.",
      "look_for": ["Absolute URLs", "Empty next parameter"],
      "hunks": ["src/auth/redirect.ts#a1b2c3d4"]
    }
  ]
}
```

### 7.6 Pull

```json
{
  "id": "uuid", "owner": "cfcomputing", "repo": "shop", "number": 123,
  "url": "https://github.com/cfcomputing/shop/pull/123",
  "title": "Validate the redirect target", "author": "octocat",
  "body_html": "<p>sanitized HTML made by the server from the PR's markdown</p>",
  "state": "open", "draft": false,
  "base_ref": "main", "head_ref": "fix/redirect",
  "base_commit": "merge-base sha", "head_commit": "sha",
  "clone": "/Users/me/code/shop",
  "status": "active", "session": null,
  "added_at": 1795000000000, "fetched_at": 1795000000000,
  "progress": { "files": 12, "reviewed": 4 }
}
```

`state`: `open`, `closed`, `merged`. `status`: `active`, `done`.

### 7.7 Takeaway

```json
{ "title": "Run the migration check before pushing", "body": "One short paragraph.", "scope": "project" }
```

`scope`: `everywhere`, `project`, `note`.

### 7.8 Errors

Every non-2xx JSON response:

```json
{ "error": { "code": "not_found", "message": "No session with that id." } }
```

Codes: `bad_request` 400, `unauthorized` 401, `bad_host` 403, `bad_origin` 403, `forbidden` 403,
`not_found` 404, `conflict` 409, `stale` 409, `too_large` 413, `unprocessable` 422,
`unavailable` 503 (a needed CLI is missing or signed out), `internal` 500. `message` is a full
sentence a person can act on.

### 7.9 Goal

```json
{
  "text": "The login page redirects exactly once",
  "criteria": [{ "id": "k1", "text": "Signing in lands on the dashboard after one redirect" }],
  "checks": [
    {
      "id": "c1", "criterion": "k1", "title": "Login flow test", "kind": "command",
      "stage": "quick", "command": "cargo test --test login", "guards": ["tests/login.rs"],
      "timeout_s": 300,
      "why": "Drives the real sign-in and counts redirects.",
      "done": false,
      "last": { "passed": false, "exit_code": 1, "timed_out": false, "tampered": false, "skipped": false, "duration_ms": 4200, "at": 1795000060000 }
    }
  ],
  "limits": { "rounds": 3, "cost_usd": 5.0, "minutes": 60 },
  "baseline_on": true,
  "baseline": [{ "check": "c1", "passed": false }],
  "reviewer": { "on": true, "agent": "claude", "model": "fable" },
  "verdict": null,
  "status": "working",
  "paused": null,
  "round": 1,
  "running": null,
  "cost_at_start": 0.0,
  "working_ms": 120000,
  "started_at": 1795000000000,
  "report": null
}
```

- `kind`: `command`, `manual`. `command` is null for `manual`; its `title` is the instruction.
- `stage`: `quick` (tests, build, lint) or `full` (an end-to-end run). Full checks run only when
  every quick check passed in that round; otherwise their result is `skipped`.
  `done` matters only for `manual`.
- `guards` are workspace-relative paths (files or folders). Their hashes are kept on the server
  and are not part of this shape.
- `last` is null until the check has run. `tampered` means a guarded path changed since the goal
  started or since the change was accepted.
- `limits.cost_usd` and `limits.minutes` may be null (no limit). `minutes` counts only time in
  `working`.
- `status`: `baseline`, `working`, `checking`, `reviewing`, `paused`, `met`, `dropped`.
- `running` is the id of the check running now, or null.
- `reviewer.on` adds a reviewer: once every command check passes, a one-shot run of
  `reviewer.agent` (default: the session's agent) with `reviewer.model` reads the goal, the diff
  and the evidence. `verdict` is its last answer: `{ approved, concerns: [text], round, at }`.
  Concerns go back to the agent as a round.
- `paused` is non-null exactly when `status` is `paused`:
  `{ "reason": "rounds", "message": "2 of 5 checks still fail after 3 rounds." }`. `reason`:
  `rounds`, `cost`, `time`, `guard`, `manual`, `ended`, `review` (the reviewer could not run).
- `baseline` is empty when `baseline_on` is false or before it has run.
- `report` is null until the goal is met, then the finish report:
  `{ state, at, round, files: [{ path, status, added, removed }], more_files, summary,
  concerns: [text], commit_message, error, committed }`. `state`: `writing` (the facts are in,
  the summary is coming from a one-shot run of the session's agent that reads the diff and the
  transcript), `ready`, `failed` (`error` says why; `commit_message` falls back to one built from
  the goal). `files` leaves out the workspace's `.syncaidync/` sidecar and lists at most 200;
  `more_files` counts the rest. `committed` is the short hash once Commit ran, or the agent's
  own last commit when it committed everything itself. `merged` is the merge commit's short
  hash after Merge. Keep working clears the report.

GoalRecord (`GET /sessions/{id}/goal`): `{ "goal": Goal, "rounds": [{ "round": 1, "at": 0,
"results": [{ "check": "c1", "passed": false, "exit_code": 1, "timed_out": false, "tampered":
false, "skipped": false, "duration_ms": 4200, "changed": [], "created": [], "output": "…" }] }] }`:
the baseline (round 0) and the last 10 rounds, oldest first. `changed` lists the guarded paths
that changed; `created` those of them that did not exist when the goal started. A round re-run
after accepting a guard replaces that round.
`output` keeps the last 16 KiB.

## 8. HTTP API

Base path `/api/v1`. JSON in, JSON out, unless noted. `{id}` is a session id, `{pid}` a pull id.

### Auth and basics

| Method and path | Body | Returns |
|---|---|---|
| `POST /auth` | `{ key }` | 204 and the cookie, or 401 |
| `GET /auth` | | 204 if the cookie is good, else 401 |
| `GET /meta` | | Meta |
| `GET /stream` | | Server-sent events (section 9.1) |
| `GET /update` | | `{ current, latest, newer, command }` (`latest` may be null) |
| `GET /load` | | Load (live process CPU and memory; not stored) |
| `POST /markdown` | `{ text, source_lines? }` | `{ html }` — sanitized, no session media rewrite; optional `data-source-line` on blocks |

### Sessions

| Method and path | Body | Returns |
|---|---|---|
| `GET /sessions` | | `{ sessions: [Session] }` |
| `POST /sessions` | NewSession (below) | 201 Session |
| `DELETE /sessions/{id}?workspace=keep\|remove` | | 204 |
| `POST /sessions/{id}/resume` | | Session |
| `POST /sessions/{id}/stop` | | Session |
| `POST /sessions/{id}/archive` | `{ archived: bool }` | Session. Archive requires an ended session. Restore before resuming. |
| `POST /sessions/{id}/snooze` | `{ snoozed: bool }` | Session |
| `POST /sessions/{id}/title` | `{ title }` (1–120 chars after trim) | Session |
| `POST /sessions/{id}/send` | `{ text, submit: bool }` | 204; 409 if not live |
| `GET /sessions/{id}/terminal[?mode=view]` | | WebSocket (section 9.2) |
| `GET /sessions/{id}/links` | | `{ repo_url, branch_url, pull: { number, url, state } \| null }` (any may be null) |
| `GET /sessions/{id}/removal` | | `{ owned, dirty, unpushed, path }`, for the remove dialog |

NewSession:

```json
{
  "agent": "claude", "model": null, "title": null, "prompt": null, "autonomy": false,
  "workspace": { "kind": "worktree", "repo": "/Users/me/code/shop", "base": "main", "branch": "dync/fix-login-redirect" }
}
```

`workspace` is one of `{ kind: "worktree", repo, base, branch }`, `{ kind: "folder", path }`,
`{ kind: "scratch" }`.

NewSession may also carry `goal`: `{ text, criteria: [{ id, text }], checks: [{ id, criterion,
title, kind, stage, command, guards, timeout_s, why }], limits, baseline_on, reviewer }`. The
server fills in the rest of the Goal. With `baseline_on`, the response comes back with
`live: false` and `goal.status: "baseline"`; the agent starts once the checks have run.

### Goals

| Method and path | Body | Returns |
|---|---|---|
| `POST /goals/draft` | `{ agent, model, text, criteria, answers: [{ question, answer }], workspace }` (`workspace` as in NewSession) | `{ checks, questions, missing_guards }`: `missing_guards` are guarded paths that do not exist in the workspace (for a worktree: at the base) |
| `GET /sessions/{id}/goal` | | GoalRecord |
| `POST /sessions/{id}/goal/run` | | Session; 409 while checking |
| `POST /sessions/{id}/goal/continue` | `{ limits }` | Session; 422 if the paused reason still holds |
| `POST /sessions/{id}/goal/accept` | `{ check }`, or `{}` for every flagged check | Session; 409 when nothing is flagged. Clearing the last flag on a goal paused for `guard` re-runs that round's checks and carries on |
| `POST /sessions/{id}/goal/manual` | `{ check, done }` | Session |
| `POST /sessions/{id}/goal/drop` | | Session |
| `POST /sessions/{id}/goal/report` | | Session; writes the finish report again. 409 unless met, or while writing |
| `GET /sessions/{id}/goal/commit` | | `{ branch, workspace, files }`: what Commit would take in. 409 unless met; 422 outside git |
| `POST /sessions/{id}/goal/commit` | `{ message }` | Session with `report.committed`. Runs `git add -A -- <files>` then `git commit -m <message> -- <files>` on the uncommitted paths (tracked changes against HEAD and untracked files, never `.syncaidync/`). 409 when there is nothing to commit. Never pushes |
| `GET /sessions/{id}/goal/merge` | | `{ repo, base, branch, commits, uncommitted, checked_out, blocked }` for a worktree session; `blocked` is a sentence when the merge cannot run (uncommitted files in the worktree, nothing to merge, the repository folder on another branch, or uncommitted changes to tracked files there). 422 for other workspaces |
| `POST /sessions/{id}/goal/merge` | | Session with `report.merged`. Runs `git merge --no-ff -m "Merge <branch>…" <branch>` in the repository folder; a conflict is aborted and reported (422). 409 when blocked. Never pushes |
| `POST /sessions/{id}/goal/more` | `{ text, rounds }` | Session; sends `text` to the agent and sets the goal back to `working` with `limits.rounds = rounds` (default: rounds used + 2, at most 10). 409 unless met; 422 if the session is not live |

All goal routes on a session without a goal → 404.

### Files (session workspace only)

| Method and path | Body | Returns |
|---|---|---|
| `GET /sessions/{id}/tree?path=<rel>` | | `{ path, entries: [{ name, kind, size, ignored }], truncated }`, one level, folders first, then by name |
| `GET /sessions/{id}/find?q=<text>` | | `{ paths: [rel], truncated }`, up to 200 matches |
| `GET /sessions/{id}/file?path=<rel>` | | `{ path, text, revision, size }` |
| `PUT /sessions/{id}/file` | `{ path, text, revision, force }` | `{ revision }`, or 409 `stale` |
| `POST /sessions/{id}/markdown` | `{ text, base_path, source_lines? }` (`base_path` may be null) | `{ html }` sanitized for the page; optional `data-source-line` on blocks |
| `GET /sessions/{id}/media?path=<rel>` | | image bytes (`png`, `jpeg`, `gif`, `webp`, `svg`); at most 2 MiB |

`kind` is `dir`, `file` or `link`. `revision` is the sha256 of the bytes on disk. Markdown
rendering uses section 14; when `base_path` is set, relative **image** URLs are rewritten to the
session media route. Media uses the same path containment as files (section 11).

### Margins (workspace sidecar)

Stored under `<workspace>/.syncaidync/margins/`, never under `~/.syncaidync`. Gitignored by
default via `.syncaidync/.gitignore` (`*` plus `!.gitignore`). Opt-in share: edit that ignore
or `git add -f`.

```text
.syncaidync/
  .gitignore
  margins/
    index.json                 catalog for the UI and for regenerating INDEX.md
    INDEX.md                   short list agents can open on demand
    entries/<id>.json          full margin
    media/<id>/<filename>      attached images
```

Margin (full):

```json
{
  "id": "uuid", "path": "src/auth/redirect.ts", "start": 11, "end": 14,
  "quote": "  return go(safe(next));", "title": "Redirect flow",
  "body": "Markdown; images as ![x](media/diagram.png)",
  "media": ["diagram.png"], "updated_at": 1795000060000
}
```

Index entry: `{ id, path, start, end, quote, title, updated_at }` (no body; `quote` is
for relocating marks when lines slide).

| Method and path | Body | Returns |
|---|---|---|
| `GET /sessions/{id}/margins[?path=<rel>]` | | `{ margins: [index entry] }` |
| `GET /sessions/{id}/margins/{mid}` | | Margin |
| `PUT /sessions/{id}/margins/{mid}` | Margin fields (id must match) | Margin; rewrites `INDEX.md` |
| `DELETE /sessions/{id}/margins/{mid}` | | 204 |
| `PUT /sessions/{id}/margins/{mid}/media/{name}` | raw image bytes (≤2 MiB) | `{ name }` |
| `GET /sessions/{id}/margins/{mid}/media/{name}` | | image bytes |
| `DELETE /sessions/{id}/margins/{mid}/media/{name}` | | 204 |

Rendering a margin body uses the markdown pipeline with image destinations under
`media/<name>` rewritten to the margin media route (`/sessions/{id}/…` or `/pulls/{id}/…`).
Caps: 500 margins per workspace, body ≤20,000 chars, title ≤120 chars.

Pull request margins use the same sidecar on the pull's local clone (the discovered repo or
`~/.syncaidync/pull-clones/…`). Routes mirror the session table with `/pulls/{pid}/` instead of
`/sessions/{id}/`.

### Changes and reviews

| Method and path | Body | Returns |
|---|---|---|
| `GET /sessions/{id}/changes` | | ChangeSet; 422 if the workspace is not a git repository |
| `GET /sessions/{id}/review` | | Review |
| `PUT /sessions/{id}/review` | `{ reviewed, notes, overall }` | Review |
| `POST /sessions/{id}/changes/plan` | `{ agent, model }` | ReadPlan (also saved into the review) |

### Takeaways

| Method and path | Body | Returns |
|---|---|---|
| `GET /sessions/{id}/takeaways/destinations` | | `{ everywhere, project, note }`: the file each scope would write to (`project` is null outside a repo) |
| `POST /sessions/{id}/takeaways/suggest` | `{ agent, model }` | `{ takeaways: [Takeaway] }` |
| `POST /sessions/{id}/takeaways/save` | `{ takeaways: [Takeaway] }` | `{ written: [{ scope, path, count }] }` |

### Discovery

| Method and path | Body | Returns |
|---|---|---|
| `GET /repos` | | `{ repos: [{ path, name }] }` |
| `GET /repos/branches?path=<abs>` | | `{ default, current, branches: [name] }`; the path must be one returned by `/repos` or a git repository the user typed |
| `GET /folders[?path=<abs>]` | | `{ path, parent, entries: [{ name, path }], truncated }`; empty path lists configured roots; otherwise one level of subfolders under a path inside those roots |
| `GET /history` | | `{ items: [{ agent, agent_session, cwd, first_prompt, last_active, transcript }] }` |
| `POST /history/import` | `{ agent, agent_session }` | 201 Session |

### Pull requests

| Method and path | Body | Returns |
|---|---|---|
| `GET /pulls` | | `{ pulls: [Pull] }` |
| `POST /pulls/open` | `{ ref }` (URL or `owner/repo#n`) | Pull (existing one if already listed) |
| `GET /pulls/{pid}/changes[?refresh=1]` | | ChangeSet |
| `GET /pulls/{pid}/file?path=<rel>` | | `{ path, text }` at the PR head (UTF-8, ≤1 MiB) |
| `GET /pulls/{pid}/margins[?path=<rel>]` | | `{ margins: [index entry] }` on the clone |
| `GET /pulls/{pid}/margins/{mid}` | | Margin |
| `PUT /pulls/{pid}/margins/{mid}` | Margin fields (id must match) | Margin; rewrites `INDEX.md` |
| `DELETE /pulls/{pid}/margins/{mid}` | | 204 |
| `PUT /pulls/{pid}/margins/{mid}/media/{name}` | raw image bytes (≤2 MiB) | `{ name }` |
| `GET /pulls/{pid}/margins/{mid}/media/{name}` | | image bytes |
| `DELETE /pulls/{pid}/margins/{mid}/media/{name}` | | 204 |
| `GET /pulls/{pid}/review` | | Review |
| `PUT /pulls/{pid}/review` | `{ reviewed, notes, overall }` | Review |
| `POST /pulls/{pid}/plan` | `{ agent, model }` | ReadPlan |
| `POST /pulls/{pid}/draft` | | `{ url }` of the pending review |
| `POST /pulls/{pid}/session` | `{ agent, model, autonomy }` | 201 Session |
| `PATCH /pulls/{pid}` | `{ status }` | Pull |
| `DELETE /pulls/{pid}` | | 204 |

### Agent callbacks

Base path `/agent/v1`. Bearer signal token.

| Method and path | Body | Returns |
|---|---|---|
| `POST /signal?session={id}` | `{ agent, payload }` (payload is the hook's raw JSON) | 204 |
| `POST /usage?session={id}` | `{ agent, payload }` | `{ line }`, the text to print in the status bar |

## 9. Live channels

### 9.1 Event stream

`GET /api/v1/stream`, `Content-Type: text/event-stream`, `Cache-Control: no-store`.

| Event | Data |
|---|---|
| `state` | `{ sessions: [Session], pulls: [Pull] }`, sent first on every connection |
| `session` | Session (created or changed) |
| `session_gone` | `{ id }` |
| `pull` | Pull |
| `pull_gone` | `{ id }` |

A comment line `: ping` goes out every 20 seconds. Events are fanned out through one
`tokio::sync::broadcast` channel of pre-serialized strings. If a subscriber falls behind
(`Lagged`), send it a fresh `state` and carry on. Usage-only changes are coalesced: at most one
`session` event per session per 500 ms for those.

### 9.2 Terminal socket

`GET /api/v1/sessions/{id}/terminal` upgrades to a WebSocket. Same credential and Origin rules.

Server to client:

- **Binary frame:** raw bytes from the PTY.
- **Text frame:** one JSON object: `{ "t": "size", "cols": 120, "rows": 32 }`,
  `{ "t": "replay_done" }`, `{ "t": "exit", "code": 0 }`.

Client to server:

- **Binary frame:** bytes to write to the PTY (keystrokes, paste).
- **Text frame:** `{ "t": "resize", "cols": 120, "rows": 32 }`.

On attach the server sends `size`, then the replay buffer as binary frames, then `replay_done`,
then live output. For a session that is not live, it sends the replay (from the retained log when no PTY is running), `replay_done`, `exit`, and keeps the socket open until the client
leaves or the session is resumed, at which point live output continues on the same socket.

Several sockets may attach to one session (board card, workspace panel, pop-out window). With
`?mode=view`, the server ignores `resize` from that socket. The PTY size is the size last sent by
a non-view socket. Every socket is told the new `size` when it changes, so a viewer can scale.

### 9.3 Replay buffer

An editable terminal replays the retained log on attach. A view-only terminal uses the newest
256 KiB, backed by a byte ring per live session (`core::ring`). An already connected terminal
receives only the new process's recent output on resume. When cutting old bytes, skip leading
UTF-8 continuation bytes.

### 9.4 Stopping a terminal

Send `SIGHUP` to the child's process group; after 3 seconds, `SIGKILL` if it is still there. The
child is a session leader (the PTY library calls `setsid`), so its pid is its group id.

## 10. Finding repos

- Roots: `SYNCAIDYNC_ROOTS`, else these under the home directory when they exist: `code`, `src`,
  `dev`, `work`, `projects`, `repos`, `git`, `Developer`, `Documents/GitHub`, and the home
  directory itself (depth 1 only).
- A repo is a folder containing `.git` (a folder or a file). Do not descend into a repo.
- Depth 3 below each root. Skip names starting with `.`, and `node_modules`, `target`, `dist`,
  `build`, `vendor`, `Library`, `Applications`.
- Skip SyncAiDync's own `worktrees` folder.
- At most 500 repos. Sorted by name. Cached for 30 seconds.

## 11. File access rules (`files.rs`)

Given a workspace root and a client path:

1. Reject absolute paths, empty segments, `.` and `..` segments, NUL bytes, and any segment
   equal to `.git`. 400 `bad_request`.
2. Canonicalize the root once. Join, then canonicalize the parent folder of the target; it must
   start with the root. 403 `forbidden` otherwise. (This catches symlinked folders that lead
   outside.)
3. Open the final file with `O_NOFOLLOW` (`OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)`), so a
   symlink as the last segment is refused, not followed.
4. Read: regular files only, at most 1 MiB, must be valid UTF-8. Otherwise 422 with a message
   that includes the size.
5. Write: the file must already exist or its parent folder must exist. Compare `revision` unless
   `force`. Write to a temp file in the same folder, rename. Keep the file's mode.
6. Listing: one level. At most 2,000 entries (`truncated: true` beyond). `ignored` is true for
   names in the fixed skip list (`node_modules`, `target`, `dist`, `build`, `.venv`,
   `__pycache__`, `.next`, `.turbo`) and for dotfolders.
7. Find: in a git repository use `git ls-files -co --exclude-standard`; otherwise walk (depth
   32, 10,000 files, skip list applies). Match is case-insensitive substring on the path.

## 12. Limits for Changes

| Limit | Value |
|---|---|
| Files in a ChangeSet | 200 |
| Untracked files included | 100, each at most 256 KiB and UTF-8 |
| Lines per file diff | 5,000 (then the file is marked truncated with no hunks past the limit) |
| Unchanged importer nodes in the map | 40 |
| Checkpoints in a plan | 8 |
| Diff command | `git diff --no-color --no-ext-diff -M -U5 <base_commit>` (working tree against base) |

## 13. The web app

### 13.1 Files

```text
web/index.html              loads css, then <script type="module" src="/js/main.js">
web/js/main.js              auth handshake, defines elements, mounts <dync-app>
web/js/lib/element.js       DyncElement base class, escapeText, html tagged template
web/js/lib/api.js           fetch wrapper: JSON, error shape, 401 handling
web/js/lib/store.js         sessions, pulls, meta; subscribes to /stream; emits "change"
web/js/lib/route.js         hash routing
web/js/lib/queue.js         queue order, counts, labels             (pure)
web/js/lib/layout.js        rows, grid, free rectangles             (pure)
web/js/lib/tabgroups.js     panel and tab model                     (pure)
web/js/lib/notes.js         compose the notes message               (pure)
web/js/lib/diffview.js      rows for unified and side-by-side views (pure)
web/js/lib/themes.js        accent themes, contrast                 (pure)
web/js/lib/time.js          "3m", "2h 05m" formatting               (pure)
web/js/lib/prefs.js         typed localStorage access
web/js/lib/drag.js          pointer move, resize, chip drag
web/js/lib/appearance.js    auto, day, night
web/js/lib/sound.js         synthesized tones
web/js/lib/alerts.js        title, favicon, notifications, reminders
web/js/lib/markdown.js      the one place server HTML is inserted
web/js/lib/native.js        bridge to the mac app when present
web/js/components/dync-*.js
```

### 13.2 Routes (URL hash)

| Hash | Screen |
|---|---|
| `#/` | Board |
| `#/s/<session id>` | Workspace view |
| `#/s/<session id>/<tool>?solo=1` | One tool alone, for pop-out windows |
| `#/pulls` | Pull request list |
| `#/p/<pull id>` | Pull request review |

A pop-out window is a normal second window on the same origin. It opens its own event stream
and its own terminal socket. DOM is never moved between windows.

### 13.3 The store

`store.js` exports one object with `sessions` (a `Map` by id), `pulls`, `meta`, `connected`, and
`subscribe(fn)`. It applies `state`, `session`, `session_gone`, `pull`, `pull_gone` events.
`EventSource` reconnects on its own; while disconnected the top bar shows "Reconnecting…". On a
401 from any call, the app shows the "Open SyncAiDync from the link it printed" screen.

### 13.4 Browser storage keys

All under `syncaidync:`. `prefs.js` owns reading and writing; nothing else touches
`localStorage`.

| Key | Value |
|---|---|
| `syncaidync:appearance` | `auto`, `day`, `night` |
| `syncaidync:theme` | accent theme name |
| `syncaidync:alerts` | `{ sound, volume, notify, remind_minutes, on_ready }` |
| `syncaidync:show-archived-sessions` | boolean, defaults to false |
| `syncaidync:board` | `{ layout, collapsed: [id], free: { <id>: { x, y, w, h, z } } }` |
| `syncaidync:new-session` | last agent, model per agent, workspace kind, repo |
| `syncaidync:workspace:<id>` | panel groups and rectangles for that session |
| `syncaidync:changes` | `{ view: "unified" \| "split" }` |
| `syncaidync:goal-defaults` | last goal limits, baseline switch and reviewer choice |

Entries for sessions that no longer exist are pruned when the store first loads.

## 14. Building blocks that were checked

These patterns were compiled against the crate versions above on 2026-11-17. The WebSocket
handshake, the event stream and the markdown filter were also run. The PTY part compiled but was
not run (the checking environment could not open a PTY), so WO-03 starts by proving it. They show
how the crates fit together; write the real code to the work order, do not paste these.

**Serving and upgrading without a framework**

```rust
type Body = http_body_util::combinators::BoxBody<Bytes, Infallible>;

// accept loop
let (stream, _) = listener.accept().await?;
tokio::spawn(async move {
    let conn = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service_fn(handle))
        .with_upgrades();                       // required for WebSockets
    let _ = conn.await;
});

// inside the handler, for a WebSocket route (req: Request<Incoming>, mutable)
let accept = derive_accept_key(key.as_bytes()); // key = the Sec-WebSocket-Key header value
tokio::spawn(async move {
    if let Ok(upgraded) = hyper::upgrade::on(&mut req).await {
        let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
        // ws.split() gives a sink and a stream of tungstenite Messages
    }
});
// then answer 101 with: Connection: Upgrade, Upgrade: websocket, Sec-WebSocket-Accept: <accept>
```

`derive_accept_key` is `tokio_tungstenite::tungstenite::handshake::derive_accept_key`; `Role` is
`tungstenite::protocol::Role`. Also check `Sec-WebSocket-Version: 13` and `Upgrade: websocket`
before upgrading.

**A streaming body for server-sent events**

```rust
let stream = futures_util::stream::unfold(rx, |mut rx| async move {
    let text = rx.recv().await?;                      // rx: mpsc::Receiver<String>
    Some((Ok::<_, Infallible>(Frame::data(Bytes::from(text))), rx))
});
let body = BodyExt::boxed(StreamBody::new(stream));   // Content-Type: text/event-stream
```

**A PTY**

```rust
let (pty, pts) = pty_process::open()?;
pty.resize(pty_process::Size::new(rows, cols))?;      // rows first
let mut child = pty_process::Command::new(bin).args(args).envs(env).current_dir(cwd).spawn(pts)?;
let (mut reader, mut writer) = pty.into_split();      // AsyncRead / AsyncWrite; writer.resize(...)
// child is a tokio::process::Child; child.id() is the pid and the process group id
```

**Markdown to safe HTML**

Parse with `pulldown_cmark::Parser::new_ext` (tables, strikethrough, task lists). Drop every
`Event::Html` and `Event::InlineHtml`. For `Tag::Link` and `Tag::Image`, replace the destination
with `#` unless it starts with `https://`, `http://`, `mailto:` or `#` (compare lower-cased and
trimmed). Then `html::push_html`. No sanitizer library is needed because no raw HTML survives.

When rendering for a session with a `base_path`, relative **image** destinations are instead
resolved against the directory of `base_path` (POSIX-style `.` / `..` segments, no absolute
paths) and rewritten to `/api/v1/sessions/{id}/media?path=<resolved>`. Relative **links**
become `#dync-open:<resolved>` so the Files View can open that workspace path. Remote `https`
image URLs may appear in the HTML but the page CSP (`img-src 'self' data:`) blocks them in the
browser; local media is the supported path. The shared prose surface is `.md-body` in
`web/css/markdown.css`. Mermaid (and similar diagram engines) are not vendored: their
dependency trees are not MIT-only; use attached SVG/PNG diagrams in margins instead.

## 15. Tests

- `syncaidync-core`: unit tests beside each module. This is where most logic is tested.
- `syncaidync/tests`: start the real server on port 0 with a temp home and
  `SYNCAIDYNC_DEV_SHELL=1`, talk to it over real sockets using a small test client written with
  `tokio` (plain HTTP/1.1 requests; for WebSockets, do the handshake by hand and wrap the stream
  with `WebSocketStream::from_raw_socket(..., Role::Client, None)`).
- `web/test`: `node --test "web/test/*.test.js"` on the pure modules in `web/js/lib`. Use the
  quoted glob: passing the folder fails on Node 24. Test files use `import` with no
  `package.json`; Node detects ES modules by syntax.
- UI behaviour: a manual pass in the browser, with screenshots in the work order report.
- `scripts/check.sh` runs, in order: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `node --test "web/test/*.test.js"`, `scripts/license-check.sh`.

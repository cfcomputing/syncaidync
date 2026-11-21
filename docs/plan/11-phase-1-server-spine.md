# Phase 1: Server spine

After this phase the binary serves an authenticated API, runs real terminals, and remembers
sessions. No agents yet: the dev shell stands in for one.

## WO-01: Core model and signal reducer

**Goal.** The data types and the state rules, pure and fully tested.

**Depends on.** WO-00.

**Read first.** `00-ground-rules.md` sections 5, 6; `02-architecture.md` section 7.1;
`04-agent-adapters.md` sections 1, 2.

**Build** (all in `crates/syncaidync-core/src`).

- `model.rs`: `Session`, `Workspace`, `WorkspaceKind`, `Blocked`, `BlockedKind`, `State`,
  `Usage`, `ToolUse`, `AgentId`, with serde derives that produce exactly the JSON in the
  architecture doc (lower-case enum values, `snake_case` fields, nulls kept). `Session::new(...)`
  builds a `launching` session. Helpers: `mark_launching`, `mark_ended`, `push_tool`,
  `set_title` (trim, 1–120 chars, error otherwise), `default_title(prompt)` (first non-empty
  line, cut to 60 chars on a character boundary, else "Untitled session").
- `signal.rs`: `Signal` and `apply`, as specified.
- `agents/mod.rs`: `AgentId`, `Observes`, the `Adapter` trait and the request/plan structs. No
  adapters yet except `shell.rs` (runs `$SHELL -i`; the shell path is passed in, not read from
  the environment).
- `agents/activity.rs`: the activity watcher state machine from `04-agent-adapters.md`
  section 3.
- `startup.rs`: `strip_ansi`, `normalize_tail`, `find_marker`.
- `ring.rs`: `ByteRing::new(capacity)`, `push(&[u8])`, `snapshot() -> Vec<u8>` (starts on a UTF-8
  boundary), `len()`.
- `time.rs`: `civil_date(epoch_ms) -> (year, month, day)` in UTC, and
  `format_date(epoch_ms) -> "YYYY-MM-DD"`.
- `version.rs`: `parse("v1.2.3" | "1.2.3") -> Option<(u64,u64,u64)>`, `is_newer(latest, current)`.

**Tests.**

- Serde: a session round-trips; the JSON has every field in the doc and no others; unknown
  fields in input are ignored; a missing optional field in old data loads as null (use
  `#[serde(default)]`).
- Reducer: a table test covering every state × every signal, plus: working clears snooze;
  `state_since` moves only on a state or blocked-kind change; tools capped at 20; detail cut to
  160 chars on a character boundary; signals on an ended session do not change state.
- Activity watcher: prompt on the command line; no prompt; quiet timer; typing echo does not
  flip to working; long output while ready does.
- `strip_ansi`: colours, cursor moves, OSC titles, a lone ESC at the end.
- Ring: wrap-around, exact capacity, a multi-byte character split at the cut.
- Dates: 1970-01-01, a leap day, 2026-11-17, a year boundary.

**Done when.** `cargo test -p syncaidync-core` passes and the crate has no I/O.

**Not in this order.** The Claude, Codex, Pi and Cursor adapters (WO-05). Diff code (WO-12).

---

## WO-02: HTTP spine: auth, static files, event stream

**Goal.** `syncaidync serve` listens on loopback, enforces the security rules, serves the UI
files and a live event stream.

**Depends on.** WO-00. (Uses WO-01's types for `/meta` if it has landed; otherwise return the
fields that exist.)

**Read first.** `00-ground-rules.md` section 7; `02-architecture.md` sections 4, 6, 7.8, 8
("Auth and basics"), 9.1, 14.

**Build** (in `crates/syncaidync/src`).

- `main.rs`: hand-written argument parsing for the command line in the architecture doc. Unknown
  flags print usage and exit 2.
- `config.rs`: resolve home (create with mode 0700), port, launch key (random or
  `SYNCAIDYNC_KEY`, refused if shorter than 32 chars), roots, web dir.
- `ids.rs`: `uuid_v4()`, `random_hex(bytes)`.
- `http/mod.rs`: bind `127.0.0.1:<port>`, accept loop, graceful shutdown on SIGINT/SIGTERM,
  `--parent-pid` watcher (`libc::kill(pid, 0)` every 2 s; this one `unsafe` call may live here
  with its reason comment).
- `http/router.rs`: split the path into segments and `match` on `(method, segments)`. One
  `AppState` (behind `Arc`) passed to handlers. Handlers return `Result<Response<Body>, ApiError>`.
- `http/error.rs`: `ApiError { status, code, message }` → the JSON error shape. A panic in a
  handler must not kill the server: run handlers so a failure becomes a 500 `internal`.
- `http/json.rs`: read a body with the 4 MiB cap; parse into a type; 400 with a helpful message
  on bad JSON. Query-string parsing with percent-decoding.
- `http/auth.rs`: Host check, Origin check, cookie and bearer check, constant-time compare,
  `POST /auth`, `GET /auth`.
- `http/static_files.rs`: serve from `SYNCAIDYNC_WEB_DIR` when set (read from disk each time,
  `Cache-Control: no-store`), else from the embedded copy
  (`include_dir!("$CARGO_MANIFEST_DIR/../../web")`, with an `ETag` and `Cache-Control: no-cache`;
  answer 304 on a matching `If-None-Match`). `/` serves `index.html`. Content types for `html`,
  `css`, `js`, `mjs`, `json`, `svg`, `png`, `ico`, `map`, `txt`; everything else
  `application/octet-stream`. Refuse paths with `..`. Never serve `web/test/`. Send the security
  headers; the CSP goes on HTML.
- `http/sse.rs`: a `Hub` holding a `broadcast::Sender<Arc<str>>`; `publish(event, &impl Serialize)`;
  the `/stream` handler (initial `state`, live events, `: ping` every 20 s, resync on lag).
- `api/misc.rs`: `GET /meta` (name, version, home, platform; agents list may be empty for now),
  `GET /update` returning `{ current, latest: null, newer: false, command }` for now.
- Opening the browser on start unless `--no-open`.

**Tests** (`crates/syncaidync/tests/http.rs`, using a small test client helper in
`tests/common/mod.rs` that later work orders reuse: start the server on port 0 in a temp home,
return its address and key, make requests).

- Wrong `Host` → 403 on a static file and on an API route.
- No credential → 401 on `/api/v1/meta`; wrong key → 401; bearer key → 200; `POST /auth` then
  cookie → 200.
- `POST` with a foreign `Origin` → 403; with the right one → passes.
- Body over 4 MiB → 413.
- `GET /` → 200 HTML with the CSP header; `GET /../Cargo.toml` and `GET /test/smoke.test.js` →
  404.
- `/stream` → first event is `state`; a published event arrives; two subscribers both get it.
- Unknown route → 404 in the JSON error shape.

**Done when.** `scripts/dev.sh` prints the URL, the placeholder page loads from it, and
`curl -H 'Host: evil.example' http://127.0.0.1:7717/` is refused.

**Not in this order.** Sessions, terminals, WebSockets.

---

## WO-03: Terminals, session registry, storage

**Goal.** Create a session that runs a real program in a PTY, attach to it from several sockets,
stop it, resume it, and find it again after a restart.

**Depends on.** WO-01, WO-02.

**Read first.** `02-architecture.md` sections 4 (session environment), 5, 7.1, 8 ("Sessions"),
9.2–9.4, 14; `04-agent-adapters.md` sections 3, 10.

**Start by proving the PTY.** Before anything else, write the smallest test that opens a PTY,
runs `/bin/sh -c 'printf hello'`, and reads `hello` back. If `pty-process` does not work on
this machine, stop and report; do not switch crates on your own.

**Build.**

- `store.rs`: `load<T>(path)`, and a `Saver` that debounces atomic writes (tmp + fsync +
  rename), handles a broken file by renaming it aside, and flushes on shutdown.
- `pty.rs`: `spawn(bin, args, env, cwd, rows, cols) -> Terminal`. A `Terminal` owns: a writer
  half (behind a mutex), the child, the size, a `ByteRing`, a `broadcast::Sender<Bytes>` for
  live output, and a log-file appender (cap 4 MiB: when exceeded, keep the newest half). One
  reader task per terminal pumps PTY output into the ring, the log and the broadcast, and feeds
  the activity watcher and startup detection. `write(bytes)`, `resize(rows, cols)`,
  `stop()` (SIGHUP to the group, SIGKILL after 3 s; `libc::kill` is the allowed `unsafe` here).
  On exit, report the exit code to the registry.
- `registry.rs`: owns `HashMap<id, Session>` plus live terminals and per-session signal tokens.
  Operations: `create`, `resume`, `stop`, `remove`, `snooze`, `rename`, `send`,
  `apply_signal`, `on_exit`, `list`. Every change saves (debounced) and publishes a `session`
  event; removal publishes `session_gone`. One lock; never hold it across an await on I/O.
- On startup: load `sessions.json`; every session that was live becomes `ended` with
  `live: false` (the processes are gone).
- On shutdown: stop every terminal, wait up to 3 s, save.
- `api/sessions.rs`: the session routes in the architecture doc, except `links` and `removal`
  (WO-18, WO-06). For now `POST /sessions` supports `workspace.kind = "folder"` (the path must
  exist and be a directory) and `"scratch"`; `"worktree"` returns 422 until WO-06. Only the
  `shell` agent exists, and only with `SYNCAIDYNC_DEV_SHELL=1`.
- `http/ws.rs` and the terminal route: upgrade, then the protocol in section 9.2, including
  replay, `mode=view`, several sockets per session, and staying open across a resume.
- `send`: bracketed paste as in `04-agent-adapters.md` section 13.

**Tests** (`tests/sessions.rs`, with a WebSocket test client helper).

- Create a shell session in a temp folder → 201, state `launching`, a `session` event on the
  stream.
- Attach: get `size`, then `replay_done`; send `echo marker-$((6*7))\r` as binary; read until
  `marker-42` appears.
- A second socket attached later receives the earlier output in its replay.
- `resize` from an edit socket changes the size reported to a view socket; `resize` from a view
  socket is ignored.
- `stop` → an `exit` frame, state `ended`, `live` false. `resume` → state `launching`, and the
  same socket receives new output.
- `send` with three lines arrives in the shell as pasted text (check with `cat` running).
- Restart the server on the same home → the session is listed as `ended`; resume works.
- Remove → 204, `session_gone` on the stream, gone after restart.
- Killing the server leaves no child process behind (check the pid is gone).
- A corrupt `sessions.json` is moved aside and the server starts.

**Done when.** With `scripts/dev.sh` running, a WebSocket client (or the UI from WO-04) can
type into a shell session, and the session survives a server restart as `ended`.

**Not in this order.** Real agents, worktrees, the UI.

## Field notes

# Phase 3: Agents and workspaces

## WO-05: Agent adapters and callbacks

**Goal.** Real agents start in sessions and report what they are doing.

**Depends on.** WO-03.

**Read first.** `04-agent-adapters.md` (all); `02-architecture.md` sections 4, 6 (item 7), 8
("Agent callbacks").

**This work order needs real CLIs.** It is the one most likely to meet surprises. Do the
**Verify** items in `04-agent-adapters.md` as you go and record what you find in its field
notes. If an agent is not installed on this machine, build its adapter from the doc, test what
is pure, and say in your report that it was not run.

**Build.**

- Core adapters: `agents/claude.rs`, `codex.rs`, `pi.rs`, `cursor.rs`: `launch`, `signals`,
  `usage`, `startup_markers`, `models`. (`oneshot` and `oneshot_reply` come in WO-14; leave them
  returning an "unsupported" value the trait allows, or add them now if simple.)
- Binary discovery and the `agents` list in `GET /meta`.
- Registry: on create/resume, build the `LaunchPlan`, write its files (mode 0600, parent
  0700), build the environment (`04-agent-adapters.md` section 5), spawn. Make a fresh signal
  token per start.
- `api/agent.rs`: `POST /agent/v1/signal` and `/agent/v1/usage`. Check the token against the
  session. Pass the payload to the adapter, apply each signal, publish. With
  `SYNCAIDYNC_DEBUG_SIGNALS=1`, append `{ at, session, agent, payload }` to
  `logs/signals.jsonl`.
- `hookcli.rs`: the `signal` and `usage` subcommands. Read stdin (or the last argument, for
  Codex's notify form). Read `SYNCAIDYNC_URL`, `SYNCAIDYNC_SESSION`, `SYNCAIDYNC_SIGNAL_TOKEN`;
  if any is missing, exit 0 silently (the hook was run outside a session). POST with a
  hand-written HTTP/1.1 request over `std::net::TcpStream` to `127.0.0.1` (2 s connect and read
  timeouts, `Connection: close`). `usage` prints the returned line. Always exit 0.
- Startup-prompt detection and the activity watcher wired into the terminal reader.
- `POST /sessions` now accepts any installed agent, `model`, `autonomy` and `prompt`.

**Tests.**

- Core: each adapter's `launch` for start and resume, with and without model, autonomy and
  prompt (compare the argument lists). Claude's settings file is valid JSON with all eight
  events and the status line, and quotes a binary path that contains spaces.
- Core: Claude payload fixtures → signals (make the fixtures from real captures, trimmed). Tool
  detail extraction for each tool kind.
- Core: usage parsing with missing fields.
- Integration: post a signal with the right token → the session changes and an event is
  published; wrong token → 401; a token for another session → 401; the launch key is not
  accepted on `/agent/v1`.
- Integration: run the built binary's `signal` subcommand with the environment set and a
  payload on stdin → the session changes. Run it with no environment → exit 0, no output. Run
  it against a closed port → exit 0 within 3 seconds.
- Integration (dev shell): startup marker detection, using a shell session that prints a marker
  phrase.

**Manual check, per installed agent.** Start a session with a small prompt in a scratch folder.
Watch the state on the stream: `launching` → `working` → `ready`. Trigger an approval (ask it to
run a shell command, autonomy off): `blocked` with the command in the message; approve:
`working`. Stop, resume: the conversation continues. For Claude: cost and context appear, and
its status bar shows the SyncAiDync line.

**Done when.** At least Claude Code passes the manual check end to end, and the report lists the
state of each other agent honestly.

**Not in this order.** One-shot runs (WO-14). History import (WO-18).

---

## WO-06: Workspaces, repo discovery, new-session dialog

**Goal.** Start a session in a new worktree, an existing folder or a scratch folder, from a
proper dialog.

**Depends on.** WO-03, WO-04.

**Read first.** `01-product.md` sections 2.1, 2.2; `02-architecture.md` sections 5, 7.1, 8
("Sessions", "Discovery"), 10; `03-design-system.md` section 4.

**Build: server.**

- `git.rs`: one helper that runs `git` with arguments in a folder, with
  `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, a timeout (default 20 s), captured output,
  and a typed error carrying stderr. Arguments are always passed as a list, never through a
  shell. Helpers: `is_repo`, `toplevel`, `head_commit`, `current_branch`, `default_branch`
  (`symbolic-ref refs/remotes/origin/HEAD`, else `main` or `master` if present, else the
  current branch), `branches` (local, newest first, max 200), `rev_parse`, `is_dirty`,
  `unpushed_count`.
- `workspaces.rs`:
  - **worktree:** validate that `repo` is a git repository and `base` resolves. Validate the
    branch name with `git check-ref-format --branch`. If the branch exists, 409 with "That
    branch already exists. Pick another name." Folder:
    `~/.syncaidync/worktrees/<repo folder name>/<slug>`, where slug is the branch with the
    `dync/` prefix removed, lower-cased, anything outside `a-z0-9._-` turned into `-`, runs of
    `-` collapsed, at most 60 chars; add `-2`, `-3` if the folder exists. Run
    `git worktree add -b <branch> <folder> <base>`. Record `base_commit` (the commit `base`
    resolved to). `owned: true`.
  - **folder:** the path must exist and be a directory (expand a leading `~`). If inside a
    repo, record `repo` (top level), `branch`, `base` (the default branch), `base_commit`
    (`HEAD` now). `owned: false`.
  - **scratch:** create `~/.syncaidync/scratch/<session id>`. `owned: true`. No git.
  - **remove:** only for `owned` workspaces and only inside the SyncAiDync home (check the
    canonical path starts with it; refuse otherwise). Worktree: `git worktree remove --force`
    from the repo, then delete the branch only if it has no commits beyond `base_commit`.
    Scratch: delete the folder.
  - `suggest_branch(title) -> "dync/<slug>"`.
- `discovery.rs`: the repo scan (architecture doc, section 10) on a blocking thread, cached.
- Routes: `GET /repos`, `GET /repos/branches`, `GET /sessions/{id}/removal`; `POST /sessions`
  with `worktree`; `DELETE /sessions/{id}?workspace=remove`.

**Build: UI.**

- `web/js/lib/dialog.js`: `openDialog(element)` helper around `<dialog>` (focus the first
  field, restore focus on close, close on backdrop click and `Escape`, a `busy` state that
  disables the form).
- `dync-new-session`: the dialog in the product doc, section 2.1. Agent picker as a row of
  choice buttons (not installed ones are listed below, disabled, with the command name). Model
  field with a `<datalist>`. Workspace kind as three choice buttons; each shows its own fields.
  The repo field is a filtering combobox (arrow keys, `Enter`, `role="listbox"`) that also
  accepts a typed path. Choosing a repo loads its branches and sets the base to the default.
  The branch field follows the title until the user edits it. Remembers the last choices in
  `syncaidync:new-session`.
- `dync-remove-session`: the remove dialog, section 2.2, using `/removal` to word the folder
  option ("This worktree has uncommitted changes." / "…has 2 commits that are not pushed.").
- Add the "New session" button to the top bar and `Alt+N`.

**Tests.**

- Core or bin unit tests: slug rules, branch suggestion, tilde expansion.
- Integration, in a temp git repo made by the test: create a worktree session → the folder
  exists on the new branch, `base_commit` is right. A second one with the same title gets a
  different folder. Existing branch → 409. Remove with `workspace=remove` → folder and branch
  gone. Remove with `keep` → folder stays. A session whose workspace is a plain folder is never
  deleted, whatever the query says.
- Integration: `GET /repos` with `SYNCAIDYNC_ROOTS` pointing at a temp tree finds repos at
  depth 3, not 4, and skips `node_modules`.
- A branch name with spaces or `..` → 400 with a sentence.
- `remove` refuses any folder outside the SyncAiDync home, even one marked `owned` (test with a
  tampered `sessions.json` that points an owned workspace at a temp folder elsewhere).

**Manual check, with screenshots.** The dialog in both appearances; keyboard-only run through
it; an error from the server (existing branch) shows inside the dialog; a worktree session
starts and the agent's first prompt runs.

**Done when.** All three workspace kinds work from the dialog.

**Not in this order.** The card redesign (WO-07). History import (WO-18).

## Field notes

# Agent adapters

How SyncAiDync starts each agent CLI, learns what it is doing, resumes it, and asks it a one-off
question. One adapter per agent, in `crates/syncaidync-core/src/agents/`.

Flags in this file were read from each CLI's `--help` on 2026-11-17 (claude 2.1.326,
pi 0.80.10, cursor agent 2026.11.13, codex current). CLIs change. Lines marked **Verify** were
not confirmed by a real run: confirm them before relying on them, and write what you find under
"Field notes" at the bottom.

## 1. The adapter interface

The core crate does no I/O, so an adapter only describes what to do. The binary does it.

```rust
pub enum AgentId { Claude, Codex, Pi, Cursor, Shell }

pub enum Observes { Signals, Activity }

pub struct LaunchRequest<'a> {
    pub session_id: &'a str,            // SyncAiDync's id
    pub agent_session: Option<&'a str>, // the CLI's own id, when known
    pub resume: bool,
    pub model: Option<&'a str>,
    pub autonomy: bool,
    pub prompt: Option<&'a str>,
    pub hook_bin: &'a str,              // absolute path of the running syncaidync binary
    pub run_dir: &'a str,               // ~/.syncaidync/run/<session id>
}

pub struct LaunchPlan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub files: Vec<(String, String)>,   // (absolute path, contents) to write before starting
    pub agent_session: Option<String>,  // set when the adapter chose the CLI's id itself
}

pub struct OneShotPlan {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

pub trait Adapter {
    fn id(&self) -> AgentId;
    fn label(&self) -> &'static str;
    fn bin_names(&self) -> &'static [&'static str];
    fn observes(&self) -> Observes;
    fn models(&self) -> &'static [&'static str];        // suggestions only; any text is accepted
    fn launch(&self, req: &LaunchRequest) -> LaunchPlan;
    fn signals(&self, payload: &serde_json::Value) -> Vec<Signal>;
    fn usage(&self, payload: &serde_json::Value) -> Option<Usage>;
    fn startup_markers(&self) -> &'static [Marker];
    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan;
    fn oneshot_reply(&self, stdout: &str) -> Option<String>;
}
```

Finding the binary (in the bin crate): `SYNCAIDYNC_BIN_<AGENT>` if set, else the first of
`bin_names()` found on `PATH`, else in `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`,
`~/.cargo/bin`, `~/.bun/bin`, `~/.npm-global/bin`. Checked at startup and again when Meta is
requested (cached 30 s).

## 2. Signals and the reducer (`core::signal`)

```rust
pub enum Signal {
    SessionStarted { agent_session: Option<String>, transcript: Option<String> },
    PromptSubmitted,
    ToolStarting { name: String, detail: String },
    ToolFinished { name: String },
    ApprovalNeeded { message: String },
    QuestionAsked { message: String },
    TurnEnded,
    SessionEnded,
    Other,
}

pub fn apply(session: &mut Session, signal: &Signal, now_ms: u64) -> bool  // true if anything changed
```

| Signal | Effect on the session |
|---|---|
| `SessionStarted` | Store `agent_session` and `transcript` when given. `launching` → `ready`. |
| `PromptSubmitted` | → `working`. |
| `ToolStarting` | → `working`. Append to `tools` (keep the newest 20; `detail` cut to 160 chars, one line). |
| `ToolFinished` | `blocked` or `launching` → `working`. Otherwise no state change. |
| `ApprovalNeeded` | → `blocked` with kind `approval` and the message. |
| `QuestionAsked` | → `blocked` with kind `question` and the message. |
| `TurnEnded` | → `ready`. |
| `SessionEnded` | `working` or `blocked` → `ready`. (The conversation was cleared or closed inside the CLI. The session only becomes `ended` when the process exits.) |
| `Other` | Nothing. |

Rules that apply to every row:

- `blocked` is set only by the two blocking signals and by startup detection (section 4); every
  other transition clears it.
- Entering `working` sets `snoozed` to false, so the next time it needs you, you hear about it.
- `state_since` changes only when `state` or the blocked kind changes. `updated_at` changes on
  any change.
- Signals for a session that is `ended` update ids and usage but never the state.
- Process exit is not a signal. The registry calls `mark_ended(session, exit_code, now)`: state
  `ended`, `live` false, `blocked` null.
- Starting or resuming calls `mark_launching(session, now)`: state `launching`, `live` true,
  `exit_code` null.

Test the reducer as a table: every state × every signal.

## 3. Watching by activity (`Observes::Activity`)

For agents without hooks, the registry infers state from the terminal. It is approximate, and
the UI says so.

- Input containing a carriage return, written while `ready` or `launching` → `PromptSubmitted`.
- While `working`, each chunk of output resets a quiet timer. After 5 seconds of quiet →
  `TurnEnded`.
- While `launching`: if a first prompt was passed on the command line, the first output →
  `PromptSubmitted`. Otherwise, 2 seconds of quiet after the first output → `SessionStarted`.
- Output while `ready` that keeps flowing for more than 1.5 seconds (not just an echo of typing)
  → `PromptSubmitted`.
- `blocked` cannot be told apart from `ready` this way, except through markers (section 4).

Keep this in one small, separately tested state machine in the core crate
(`agents/activity.rs`): inputs are `Output { at }`, `Input { has_return, at }`, `Tick { at }`;
outputs are signals.

## 4. Startup prompts (`core::startup`)

Before an agent's first turn it may stop on a question of its own: trust this folder, accept a
warning, sign in. No hook fires for these. So while a session is `launching`, the registry
strips ANSI escapes from the last 4 KiB of terminal output, collapses whitespace, lower-cases
it, and looks for the adapter's markers:

```rust
pub struct Marker { pub needle: &'static str, pub message: &'static str }
```

On a match → `blocked` with kind `startup` and the marker's message. Input containing a carriage
return while blocked on `startup` puts the session back to `launching`.

Starter markers. **Verify every row** by starting each CLI in a brand-new empty folder (and once
with autonomy on, once signed out if you can) and reading what it prints. Replace the needles
with the text you actually see.

| Agent | Needle (lower case) | Message shown |
|---|---|---|
| claude | `do you trust the files in this folder` | Asking whether to trust this folder |
| claude | `bypass permissions mode` | Asking you to accept running without approval prompts |
| claude | `select login method` | Needs you to sign in |
| codex | `sign in with chatgpt` | Needs you to sign in |
| codex | `do you trust` | Asking whether to trust this folder |
| cursor | `workspace trust` | Asking whether to trust this folder |
| cursor | `log in` | Needs you to sign in |
| pi | `api key` | Needs credentials |

The ANSI stripper is a small pure function with tests: remove CSI sequences (`ESC [ … final
byte`), OSC sequences (`ESC ] … BEL` or `ESC \`), and other two-byte escapes.

## 5. Environment for the agent process

Start from the server's environment, then:

1. Remove variables that mark "already inside an agent", so a server started from an agent's
   terminal does not confuse its children: `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`,
   `CLAUDE_CODE_SSE_PORT`, `CURSOR_AGENT`, `CODEX_SANDBOX`, `CODEX_SANDBOX_NETWORK_DISABLED`,
   and any `SYNCAIDYNC_SESSION` / `SYNCAIDYNC_SIGNAL_TOKEN` inherited from outside.
   **Verify:** run `env` inside each agent's shell tool and add any other marker you see. Do not
   remove user settings such as API keys, provider switches or config-dir variables.
2. Add the variables from `02-architecture.md` section 4.
3. Add the adapter's `env`.

## 6. Claude Code

| | |
|---|---|
| Binary | `claude` |
| Observes | Signals |
| Model suggestions | `opus`, `sonnet`, `haiku` |
| History | Yes (section 11) |

**Start**

```text
claude --session-id <uuid> --settings <run_dir>/claude-settings.json
       [--model <model>] [--dangerously-skip-permissions] [<prompt>]
```

The adapter makes a fresh UUID for `--session-id` and returns it as `agent_session`.

**Resume**

```text
claude --resume <agent_session> --settings <run_dir>/claude-settings.json
       [--model <model>] [--dangerously-skip-permissions]
```

**Settings file** (returned in `LaunchPlan.files`, rewritten on every start because the binary
path can change). Every hook is a `command` hook that runs this binary:

```json
{
  "hooks": {
    "SessionStart":      [{ "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "UserPromptSubmit":  [{ "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "PreToolUse":        [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "PostToolUse":       [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "Notification":      [{ "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "Stop":              [{ "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }],
    "SessionEnd":        [{ "hooks": [{ "type": "command", "command": "\"<hook_bin>\" signal --agent claude", "timeout": 5 }] }]
  },
  "statusLine": { "type": "command", "command": "\"<hook_bin>\" usage --agent claude" }
}
```

`--settings` adds to the user's own settings; it does not replace them. The hook command prints
nothing and exits 0, so it never blocks or alters a tool call.

**Verify:** the event names and the settings shape against `claude --help` and the hooks
documentation for the installed version; an unknown event name may be rejected.

**Payload → signals.** The hook's stdin is one JSON object with `hook_event_name` and common
fields `session_id`, `transcript_path`, `cwd`.

| `hook_event_name` | Signal |
|---|---|
| `SessionStart` | `SessionStarted { agent_session: session_id, transcript: transcript_path }` |
| `UserPromptSubmit` | `PromptSubmitted` |
| `PreToolUse` with `tool_name` = `AskUserQuestion` | `QuestionAsked { message: first question's text }` |
| `PreToolUse` with `tool_name` = `ExitPlanMode` | `ApprovalNeeded { message: "Plan is ready for your review" }` |
| `PreToolUse` otherwise | `ToolStarting { name: tool_name, detail }` |
| `PostToolUse` | `ToolFinished { name: tool_name }` |
| `PermissionRequest` | `ApprovalNeeded { message: "<tool_name>: <detail>" }` |
| `Notification` with `notification_type` = `permission_prompt` | `ApprovalNeeded { message }` |
| `Notification` with `notification_type` = `elicitation_dialog` | `QuestionAsked { message }` |
| `Notification` otherwise (idle reminders and the rest) | `Other` |
| `Stop` | `TurnEnded` |
| `SessionEnd` | `SessionEnded` |
| anything else | `Other` |

Every payload that carries `session_id` and `transcript_path` also refreshes those two fields on
the session (a resumed or cleared conversation can change them).

`detail` for a tool, from `tool_input`: `command` for Bash; `file_path` for Read, Edit, Write
(shown relative to the workspace when inside it); `pattern` for Grep and Glob; `url` for
WebFetch; `query` for WebSearch; `description` for Task; otherwise the first string value found,
or empty. One line, 160 characters.

**Verify:** capture real payloads with `SYNCAIDYNC_DEBUG_SIGNALS=1` for: a plain turn, a Bash
approval (approve once, deny once), a question, plan mode, `/clear`, and quitting. Save trimmed
copies as fixtures under `crates/syncaidync-core/tests/fixtures/claude/` (remove anything
personal) and test the mapping against them.

**Usage.** The status-line command receives JSON on stdin. Read
`cost.total_cost_usd` → `cost_usd` and `context_window.used_percentage` → `context_pct` (round
to a whole number). Both are optional. **Verify** the field names on a real payload. The
`usage` subcommand prints the line the server returns, for example `SyncAiDync · $0.42 · 31%`.

## 7. Codex

| | |
|---|---|
| Binary | `codex` |
| Observes | Signals if hooks can be set from the command line, else Activity plus turn-complete |
| Model suggestions | none (free text) |
| History | Only if section 11's check passes |

**Start**

```text
codex [-m <model>] [--dangerously-bypass-approvals-and-sandbox] [<config overrides>] [<prompt>]
```

**Resume**

```text
codex resume <agent_session> [same options]      when the id is known
codex resume --last [same options]               otherwise (run in the workspace folder)
```

**Reporting back.** Two mechanisms; use the first that works on the installed version.

1. **Hooks through config overrides. Verify first.** Codex accepts `-c key=value` (TOML values)
   and has a hooks system (its help lists `--dangerously-bypass-hook-trust`). Find the
   documented config shape for hooks in the installed version and pass it with `-c`, one
   override per event, each running `"<hook_bin>" signal --agent codex`. Map its events the same
   way as section 6 (session start, prompt submitted, tool starting and finished, approval
   needed, turn ended). If hooks require a trust step, add
   `--dangerously-bypass-hook-trust` only for hooks SyncAiDync itself passed on the command
   line.
2. **Turn-complete notification.** `-c 'notify=["<hook_bin>","signal","--agent","codex"]'`.
   Codex runs that program with one JSON argument when a turn completes. The `signal`
   subcommand therefore also accepts the payload as its last argument when stdin is empty or a
   terminal. A payload whose `type` is `agent-turn-complete` → `TurnEnded`; take the
   conversation id from it (`thread-id`; **Verify** the name) as `agent_session`. With only this
   mechanism the adapter reports `Observes::Activity` and the activity watcher supplies the
   rest; a real `TurnEnded` simply arrives sooner than the quiet timer.

Never edit the user's `~/.codex/config.toml`.

**Usage.** Pass a `PostModelRequest` hook with `-c` that runs
`"<hook_bin>" usage --agent codex` (async). Read `token_usage.total_tokens` (else sum
input/output/reasoning) as a per-request delta; the registry adds it to `usage.tokens`.
No dollar cost — Codex does not report one.

## 8. Pi

| | |
|---|---|
| Binary | `pi` |
| Observes | Activity |
| Model suggestions | none (free text; `pi --list-models` shows them) |
| History | No |

**Start and resume** are the same command; Pi creates the session if the id is new:

```text
pi --session-id <uuid> --extension <run_dir>/pi-usage.js [--model <model>] [--approve] [<prompt>]
```

The adapter makes the UUID and returns it as `agent_session`. On resume, do not pass the
prompt. Write `pi-usage.js` into `run_dir` (not the workspace) and pass it with `--extension`.
That extension listens for `message_end` and POSTs each assistant message's
`usage.totalTokens` to `/agent/v1/usage` as a delta for `usage.tokens`.

**Verify:** that `--approve` is the flag that turns off approval prompts (`pi --help`), and that
a positional prompt starts an interactive session rather than a print-and-exit run.

## 9. Cursor Agent

| | |
|---|---|
| Binary | `agent` (also look for `cursor-agent`) |
| Observes | Activity |
| Model suggestions | none (free text; `agent --list-models` shows them) |
| History | No |

**Start**

```text
agent [--model <model>] [--force] [<prompt>]
```

**Resume**

```text
agent --resume <agent_session> [...]     when a chat id is known
agent --continue [...]                   otherwise (continues the latest chat in that folder)
```

SyncAiDync does not learn the chat id on its own, so `--continue` is the normal path.
Do not write hook files into the user's project or home for this agent.

**Usage:** none in interactive mode. Print-mode `stream-json` can include token counts, but
there is no interactive hook to report them, so `usage` stays null.

## 10. Shell (development only)

Present only with `SYNCAIDYNC_DEV_SHELL=1`. Runs `$SHELL -i` (fallback `/bin/sh -i`). Observes
by activity. Lets you build and test the board without spending tokens, and is what the
integration tests use. Never listed in Meta otherwise.

## 11. History import

Only for agents whose history is readable files with a known layout.

**Claude Code.** Under `${CLAUDE_CONFIG_DIR:-~/.claude}/projects/<folder>/<session id>.jsonl`,
one JSON object per line. For each file: skip it if a session on the board already has that
`agent_session`; read at most the first 200 lines to find `cwd` and the first line whose `type`
is `user` with real text (`message.content` is a string, or a list of blocks; take the first
`text` block; skip lines that are tool results or start with `<`). `last_active` is the file's
modified time. Return the newest 200, newest first. Skip files whose `cwd` no longer exists.
**Verify** the field names on a real file.

Importing makes a Session with workspace kind `folder` at that `cwd` (not owned), state `ended`,
`agent_session` set, title from the first prompt. Resuming it runs the normal resume command.

**Codex. Verify, optional.** If the installed version keeps one JSONL file per conversation
under `${CODEX_HOME:-~/.codex}/sessions/` with a readable id, folder and first prompt, support
it the same way. If the layout is unclear, leave Codex out and say so in the field notes.

## 12. One-shot runs (`oneshot.rs`)

Used for read-through plans and takeaways. Runs the chosen agent once, with no tools, and
returns its text.

| Agent | Command | Prompt goes | Reply is |
|---|---|---|---|
| claude | `claude -p --output-format json --tools "" [--model m]` | stdin | field `result` of the JSON on stdout |
| codex | `codex exec --ephemeral --sandbox read-only --skip-git-repo-check --json [-m m] -` | stdin | the `text` of the last `item.completed` event whose item type is `agent_message` (stdout is JSON lines) |
| pi | `pi --print --no-session --no-tools [--model m] <prompt>` | argument | stdout |
| cursor | `agent --print --mode ask --output-format text [--model m] <prompt>` | argument | stdout |

Rules:

- Working directory: a fresh empty temp folder, removed afterwards. The model gets everything it
  needs in the prompt; it must not be able to read the workspace on its own.
- Environment: as in section 5, but without the `SYNCAIDYNC_*` session variables, so hooks stay
  quiet. For claude also pass `--settings '{"hooks":{}}'` if needed to silence user hooks.
  **Verify** that a one-shot run does not show up as a session signal.
- Timeout 240 seconds, then kill. Output capped at 2 MiB.
- When the prompt is passed as an argument, cap it at 120,000 bytes (operating-system limit on
  arguments); the callers already keep prompts under that.
- Failure (non-zero exit, timeout, empty reply) → 503 `unavailable` with the last 400 characters
  of stderr in the message.
- At most 2 one-shot runs at a time; more wait their turn.

The reply is text that should contain one JSON object. `core` has one helper,
`extract_json(text) -> Option<Value>`: try the whole text; else the contents of the first
fenced block; else the span from the first `{` to the matching last `}`. Callers validate the
shape and ignore anything extra.

## 13. Typing into a session (`POST …/send`)

To deliver a multi-line message to a terminal UI without each newline submitting early: write
`ESC [ 200 ~`, the text (with `ESC` bytes removed), `ESC [ 201 ~` (bracketed paste). If
`submit` is true, wait 150 ms, then write a carriage return. **Verify** with each agent that a
three-line message arrives as one prompt.

## 14. Interrupting a turn

When a goal reaches its cost or time limit mid-turn (WO-21), SyncAiDync writes the agent's
interrupt key to the terminal so the turn ends and the session becomes `ready`.

| Agent | Key |
|---|---|
| claude | `ESC` (0x1b) |
| codex | `ESC` (0x1b) |
| pi | `ESC` (0x1b) |
| cursor | `ESC` (0x1b) |

**Verify every row**: start a long turn in each CLI, write the byte, and confirm the turn stops
without the process exiting. If `ESC` does not do it, try `Ctrl-C` (0x03) once, and record
whether a second one would exit the CLI. Never send a key that ends the process. If no key
works, pause the goal anyway and add "The agent may still be working" to the paused message.
SyncAiDync checks 10 seconds after writing the key: if the session is still `working`, it adds
that sentence (`SYNCAIDYNC_INTERRUPT_GRACE_MS` shortens the wait in tests). The key stays `ESC`
for all four until a real run shows otherwise.

## Field notes

### 2026-11-17

- **Claude Code 2.1.327:** `--session-id`, `--settings`, `--resume`, `--dangerously-skip-permissions`
  confirmed in `--help`. Hook event names taken from this doc; not yet captured live with
  `SYNCAIDYNC_DEBUG_SIGNALS=1` in this pass — unit fixtures follow the documented payload shape.
- **Pi 0.80.10:** `--session-id`, `--model`, `--print` confirmed. `--approve` / `-a` is “Trust
  project-local files for this run”, not a full permission bypass. Still passed when autonomy is
  on as the closest documented flag.
- **Cursor Agent 2026.11.13:** `--resume`, `--continue`, `--model`, `--force` confirmed.
- **Codex 0.169.0:** `-m`, `--dangerously-bypass-approvals-and-sandbox`,
  `--dangerously-bypass-hook-trust`, `-c` config overrides, and `resume` / `--last` confirmed.
  No `notify` string in top-level help; adapter still passes
  `-c 'notify=["…","signal","--agent","codex"]'` plus hook-trust bypass, and reports
  `Observes::Activity` so the quiet timer covers turns if notify is ignored.
- All four CLIs were present on this machine under `~/.local/bin` or nvm.

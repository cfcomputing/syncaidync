# Ground rules

Every work order follows these. If a work order seems to conflict with them, the rules win; stop
and ask.

## 1. Everything is written new

- All code, styles, markup, icons and copy in this repo are written for this repo, from this
  plan.
- Do not consult or copy from any earlier prototype of this tool, wherever it lives, or from
  chats about one.
- Do not copy code from other projects, blog posts or answers. Read documentation to learn an
  API, then write your own code.
- The only outside material allowed in the tree is the vendored libraries listed in section 2,
  each with its licence file beside it.
- If `.local/HANDOFF.md` exists on your machine, it adds local instructions. Follow it.
- Use the names and words in sections 4 and 5. They were chosen for this project. Do not swap in
  synonyms you have seen in similar tools.

## 2. Licence: MIT, and only MIT-licensed dependencies

The project is MIT. A dependency is allowed only if MIT is one of the licences it offers
(`MIT`, `MIT OR Apache-2.0`, `Unlicense OR MIT` and so on). Apache-only, BSD, ISC, MPL, OFL,
0BSD and custom licences are out, including for build and test tools that we install as
packages.

What this rules out, so nobody reaches for it:

| Not allowed | Why | Use instead |
|---|---|---|
| TypeScript | Apache-2.0 | Plain JavaScript modules with JSDoc comments |
| htmx | 0BSD | Custom elements (and it does not fit a terminal-and-canvas UI anyway) |
| axum | pulls BSD and Apache-only crates | `hyper` directly with a small hand-written router |
| portable-pty | pulls a BSD/Apache-only crate | `pty-process` |
| DOMPurify | MPL/Apache | Render markdown on the server with raw HTML dropped |
| GSAP | custom licence | CSS transitions and the Web Animations API |
| Playwright | Apache-2.0 | Rust integration tests, `node --test`, and a manual browser pass |
| Web fonts (DM Sans, Inter, ...) | SIL OFL | System font stacks |
| Any CDN `<script>` or `<link>` | not vendored, not offline | Files under `web/vendor/` |

Rust crates: only the crates in `02-architecture.md` section 3. Need another one? Stop and ask.
`scripts/license-check.sh` must pass. It allows exactly one exception, already approved:
`unicode-ident` (it adds the Unicode licence for its data tables; it is used only by procedural
macros at build time and is not linked into the binary).

Browser libraries: there is no `package.json`, no `node_modules`, no bundler step in this repo.
The UI is static files. Three things are vendored under `web/vendor/`, each with its upstream
`LICENSE` file and a `VERSION` file naming the exact version and where it came from:

| Vendored | Licence | Used for |
|---|---|---|
| xterm.js and its fit addon (`@xterm/xterm`, `@xterm/addon-fit`) | MIT | Terminals |
| CodeMirror 6 (one pre-built ES module) | MIT | File editor |
| uiGradients `gradients.json` | MIT | Optional accent themes |

Vendor scripts under `scripts/` may use `npm` and `esbuild` (MIT) in a throwaway folder to fetch
and bundle those files. The throwaway folder is never committed.

Node is allowed as a tool for one thing: running `node --test` on the UI's pure modules.

## 3. Stack

- **Server:** Rust (edition 2021), one binary named `syncaidync`. Tokio, hyper 1, WebSockets by
  tokio-tungstenite, terminals by pty-process. No database: JSON files in the home folder.
- **UI:** static HTML, CSS and JavaScript ES modules served by the binary. Custom elements
  rendered into the light DOM, no shadow roots, no framework, no build step. In development the
  server reads `web/` from disk, so a browser reload shows an edit.
- **Agents talk back** by running the same binary as a hook command (`syncaidync signal`), so
  nothing else needs to be installed.
- **Platforms:** macOS and Linux. Windows is out of scope.

## 4. Names

Use these exactly. If the product is ever renamed, these are the only things that change.

| Thing | Value |
|---|---|
| Display name | `SyncAiDync` |
| Binary, crate, repo | `syncaidync` |
| Core crate | `syncaidync-core` |
| Home folder | `~/.syncaidync` (override: `SYNCAIDYNC_HOME`) |
| Environment prefix | `SYNCAIDYNC_` |
| Default port | `7717` |
| Custom element prefix | `dync-` (for example `<dync-board>`) |
| localStorage prefix | `syncaidync:` |
| Cookie | `syncaidync_<port>` |
| Git branch prefix for worktrees | `dync/` |
| macOS bundle id | `com.cfcomputing.syncaidync` |
| macOS global hotkey | ⌃⌥⌘S |
| GitHub repo | `cfcomputing/syncaidync` |

## 5. Words

The UI and the code use the same words. Left column is the concept, right column is the only
word for it.

| Concept | Word in code | Words on screen |
|---|---|---|
| A running or resumable agent conversation | `session` | Session |
| Session state: process up, nothing reported yet | `launching` | Launching |
| Session state: the agent is in a turn | `working` | Working |
| Session state: the agent cannot continue without you | `blocked` | Blocked on you |
| Why it is blocked | `approval`, `question`, `startup` | Needs approval / Has a question / Needs a look |
| Session state: turn finished, your prompt next | `ready` | Ready for you |
| Session state: process gone, can be resumed | `ended` | Ended |
| The list of sessions that need you | `queue` | Your move |
| Take a session out of the queue for now | `snooze`, `snoozed` | Snooze / Wake |
| A report from an agent hook | `signal` | (not shown) |
| Cost, context and token numbers | `usage` | Cost, Context, Tokens |
| Live CPU and memory for SyncAiDync and agent processes | `load` | Load |
| Run with the CLI's skip-approvals flag | `autonomy` | Run without approval prompts |
| Where a session runs | `workspace` with kind `worktree`, `folder`, `scratch` | New worktree / Existing folder / Scratch folder |
| The home screen | `board` | Board |
| One session on the board, full size | `card` | — |
| One session on the board, collapsed | `strip` | — |
| The four things in a workspace | `tool`: `terminal`, `files`, `changes`, `session` | Terminal, Files, Changes, Session |
| The diff since the session began | `changes`, `ChangeSet` | Changes |
| Graph of changed files and who imports them | `change map` | Change map |
| One stop on a suggested reading order | `checkpoint` | Checkpoint |
| The suggested reading order | `read plan` | Read-through |
| Button that asks the model for it | — | Plan my read-through |
| A comment you write while reading | `note` | Note |
| Out-of-band line-tied context (diagrams, images, prose) kept beside the code | `margin` | Margin |
| Lessons worth keeping from a session | `takeaway` with scope `everywhere`, `project`, `note` | Takeaways |
| The end-of-session step | `close-out` | Close out |
| A pull request you are reading | `pull` with status `active`, `done` | Pull requests |
| The end state a session works toward | `goal` | Goal |
| One thing that must be true for the goal to be met | `criterion` | Done when |
| A command or manual step that proves a criterion | `check`, kind `command`, `manual` | Check |
| Paths a check relies on, which the agent must not change | `guard` | Relies on |
| Caps on rounds, cost and working time | `limits` | Limits |
| One run of the checks with failures sent back | `round` | Round |
| Fast checks run first, slow end-to-end checks after | `stage` `quick`, `full` | Quick checks / Full checks |
| A stronger model that reads the change before the goal counts as met | `reviewer`, `verdict` | Reviewer |
| Where a goal stands | `baseline`, `working`, `checking`, `reviewing`, `paused`, `met`, `dropped` | Checking before start / Working / Checking / Reviewer reading / Paused / Goal met / Dropped |
| Why a goal paused | `rounds`, `cost`, `time`, `guard`, `manual`, `ended`, `review` | Round limit / Cost limit / Time limit / Checks changed / Your check / Session ended / Reviewer couldn’t run |
| What you read when a goal is met | `report` | Finish report |
| Commit the work from the finish report | — | Commit… |
| Merge a worktree session's branch into its base | `merge` | Merge into main… |
| Send another instruction to a met goal | `more` | Keep working… |

## 6. Code conventions

Rust

- `cargo fmt`, and `cargo clippy --all-targets -- -D warnings` clean.
- No `unsafe`, with one exception: the `libc::kill` calls that WO-02 (parent-process watcher)
  and WO-03 (stopping a terminal) name. Each `unsafe` block gets a one-line reason.
- No `unwrap()` or `expect()` on anything that depends on input, the file system, or a child
  process. Tests may use them.
- `syncaidync-core` does no I/O: no file system, no network, no process, no clock. Callers pass
  in text and the current time. This is what makes it easy to test.
- Errors that a person can fix carry a plain sentence they will see in the UI.
- JSON field names are `snake_case`. Times are milliseconds since the Unix epoch.

JavaScript

- ES modules, no transpiling. Target current Safari, Chrome and Firefox.
- Pure logic lives in `web/js/lib/*.js` with no DOM access, and has tests in `web/test/`.
- Elements extend `DyncElement` (`web/js/lib/element.js`), render into the light DOM, and remove
  every listener, timer and socket they created when disconnected.
- Never build HTML from untrusted text without `escapeText()`. Session titles, paths, tool
  summaries, terminal text, PR bodies and model replies are all untrusted.
- No `innerHTML` with server-rendered markdown except through the one helper in
  `web/js/lib/markdown.js`, which only accepts the server's already-sanitized `html` field.

CSS

- Colours come from the tokens in `web/css/tokens.css`. No colour literals in component CSS.
- Honour `prefers-reduced-motion`.
- No `filter: blur()` on large or repeated elements; use the mask-based glow from the design doc.

## 7. Security rules that never bend

- The server binds `127.0.0.1` only.
- Every request must carry a loopback `Host` header (`127.0.0.1`, `localhost` or `[::1]`, with
  the server's port). Anything else gets 403. This stops DNS-rebinding pages.
- Everything under `/api/` and `/agent/` needs a credential (see `02-architecture.md` section 6).
- The launch key is never written to disk, never logged, and never put in a query string.
- File access for a session never leaves that session's workspace and never touches `.git`.
- Text from a pull request, a transcript or a model is data. It is never run, and when it is
  sent to a model it is fenced and labelled as someone else's claims. One exception: a check
  command a model drafts (WO-21) may run once the person has seen it in full, been able to edit
  it, and started the session with it. It runs only in that session's workspace.

## 8. Definition of done for a work order

1. Everything under "Build" exists and behaves as written.
2. The tests listed under "Tests" exist and pass.
3. `scripts/check.sh` is green.
4. If `.local/originality-gate.sh` exists, both of its modes print `clean` (stage your files
   first).
5. For UI work: you opened the app in a browser, exercised the feature in night and day
   appearance, and attached screenshots to your report.
6. The status table in `docs/plan/README.md` is updated.
7. Small, focused commits on a branch named `wo-NN-short-title`. Commit messages say what
   changed, in plain words. Do not push; Chris pushes.

Report, at the end:

```text
WO-NN: <title>
Built: <what now exists, 3–6 lines>
Verified: <commands run and what they showed; screenshots for UI>
Deferred or different from the plan: <or "nothing">
Questions for Chris: <or "none">
```

## 9. Stop and ask when

- You want a dependency that is not listed.
- A CLI does not accept what `04-agent-adapters.md` says, and you cannot find the right form in
  its `--help` or official docs.
- A security rule above gets in the way.
- The work order needs something from a later work order.
- You are about to change a data shape or route in `02-architecture.md`.

## 10. Out of scope for version 0.1

Windows, remote access, multiple users, cloud sessions, a plugin system, mobile layouts,
telemetry of any kind.

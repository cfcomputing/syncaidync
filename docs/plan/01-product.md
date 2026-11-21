# Product

What SyncAiDync does, screen by screen. Work orders point at sections here by number.

## 1. The idea

You run several AI coding agents at once. Each one stops now and then: it needs approval, it has
a question, or it finished a turn. The cost is not the agents' time, it is yours: finding which
terminal wants you.

SyncAiDync puts every session on one board, sorts the ones that need you to the top, and tells
you when that list changes. Each session opens into a workspace where you can type to the agent,
read and edit its files, review what it changed, send notes back, and close it out.

It is a local tool. It runs on your machine, binds to loopback, and uses the agent CLIs you
already have installed and signed in.

## 2. Sessions

A session is one agent CLI running in a real terminal that SyncAiDync owns.

- **Agents:** Claude Code (`claude`), Codex (`codex`), Pi (`pi`), Cursor Agent (`agent`). The
  board shows only the ones found on this machine.
- **State:** `launching`, `working`, `blocked`, `ready`, `ended`. See `04-agent-adapters.md` for
  how each agent reports state.
- **Blocked** always has a reason (`approval`, `question`, `startup`) and usually a short
  message, for example the command waiting for approval.
- **Ended** sessions stay on the board. They can be resumed (the agent picks its conversation
  back up) or removed.
- Sessions survive a server restart: the terminal process does not, but the session comes back
  as `ended` and can be resumed.

### 2.1 Starting a session

The "New session" dialog asks for:

1. **Agent.** Only installed agents are offered. Remembered from last time.
2. **Model.** A text field with suggestions for that agent. Empty means the agent's default.
3. **Where to run:**
   - **New worktree** (default when a repo is chosen): pick a repo, a base branch (defaults to
     the repo's default branch), and a branch name (suggested from the title, editable).
     SyncAiDync creates a git worktree in its home folder on a new branch.
   - **Existing folder:** type or pick a path. Nothing is created.
   - **Scratch folder:** SyncAiDync creates an empty folder in its home.
4. **Title.** Optional. If empty, the title is the first line of the first prompt, cut to 60
   characters; if there is no prompt, "Untitled session".
5. **First prompt.** Optional, multi-line.
6. **Run without approval prompts.** Off by default. A one-line warning sits beside it: the agent
   will run commands and edit files without asking.

Repos are found by scanning a few usual folders under the home directory (see
`02-architecture.md` section 10). The repo field filters as you type and also accepts a pasted
path.

`Cmd/Ctrl+Enter` starts the session. While it starts, the button shows progress; if it fails,
the dialog stays open with the reason.

### 2.2 Ending and removing

- **Stop:** ends the agent process. The session becomes `ended` and can be resumed.
- **Remove:** takes the session off the board. If SyncAiDync created its worktree or scratch
  folder, the dialog offers "Also delete the folder" (default off for worktrees with uncommitted
  or unpushed work, and it says so).
- **Resume:** starts the agent again in the same workspace, continuing its conversation.

### 2.3 Goals

A session can start with a goal, set in the "Set a goal" part of the New session dialog:

1. **Goal:** the end state in one sentence.
2. **Done when:** the criteria, one line each.
3. **Checks:** "Draft checks" asks a model to propose, for each criterion, a check that fails
   until the criterion holds. A check is a command run in the workspace, or a manual step that
   you tick. The draft may ask questions; answer them and draft again. You edit, add or remove
   checks. Each check lists the paths it relies on (its test file, its script), which the agent
   must not change. The draft only guards paths that exist; when it names one that does not
   exist yet, the dialog marks it, since the agent would then write the file that judges it.
4. **Limits:** rounds (default 3), cost and working time (both optional).
5. **Run the checks once before the agent starts** (on by default). A check that already passes
   proves nothing yet, and the Goal section says so.

Turning a goal on while "Existing folder" points at a git checkout switches the session to a new
worktree of it (base: the checkout's current branch), so rounds never land on the checkout. The
dialog says so; picking Existing folder again keeps the checkout.

Each command check is **quick** (tests, build, lint) or **full** (an end-to-end run); full
checks only run when every quick check passed. Optionally a **reviewer** (a stronger model,
such as Fable) reads the change and the evidence once every check passes; its concerns go back
to the agent as a round, and only its approval lets the goal be met.

Then the agent gets the goal, the criteria and the checks with its first prompt. After every
turn SyncAiDync runs the checks itself:

- All pass, and the manual ones are ticked: **Goal met**.
- Some fail: the failures and their output go back to the agent as the next **round**.
- The round limit, cost limit or time limit is reached, the agent changed a path a check relies
  on, only manual checks are left, or the session ended: the goal is **paused** and the session
  waits in Your move with the reason. A cost or time limit reached mid-turn interrupts the agent.

While a goal is driving, the session is not in Your move: its card says what the goal is doing
(Checking, Reviewer reading) and shows a pipeline (Agent → Quick checks → Full checks →
Reviewer → Your checks → Goal met) with the running stage lit. It joins Your move, with an
alert, when the goal is met or pauses.

A paused goal continues with **Keep going** (after raising the limit or ticking the check).
A goal paused because the agent changed a guarded path carries on by itself once you accept the
last change (**Accept all** takes every flagged check): the same round's checks run again. When
the agent *created* a guarded path that did not exist at the start, the Goal section says so and
accepting asks first, since the accepted file becomes what the check is judged by. **Drop goal**
turns it back into a plain session. The checks cannot be edited once the session has started.

When the goal is met, the Goal section shows a **finish report**: the changed files (with Open
Changes), a short summary and anything worth a second look, written by a one-shot call that
reads the diff and the transcript, and a drafted commit message. Then:

- **Commit…** shows the branch and the files and commits every uncommitted change in the
  workspace with the message (editable). In a folder that is your own checkout it warns that your
  own changes go in too. The workspace's `.syncaidync/` sidecar is never committed. Nothing is
  pushed.
  When the agent already committed its own work, the report says so instead.
- **Merge into <base>…** (worktree sessions) merges the session's branch into its base in the
  repository's own folder with a merge commit. It lists the commits first and refuses, with
  the reason, when the worktree has uncommitted files, the folder has another branch checked
  out, or the folder has uncommitted changes a conflict could lose. A conflict is aborted.
  Nothing is pushed.
- **Keep working…** sends another instruction and lets the goal drive again with room for more
  rounds (two more by default).
- **Close out** opens the usual close-out (takeaways, archive).

## 3. The board

The home screen. Top to bottom:

### 3.1 Top bar

- Brand mark and name on the left.
- Load chip (live, not stored): SyncAiDync CPU and memory, the sum for live agent process
  groups, and total reported cost when known. Hidden on narrow widths.
- "Your move" count: a pill that shows how many sessions need you. Clicking it jumps to the
  first one.
- "New session" button.
- "Pull requests" button (opens the pull request list, section 7).
- "Import" button (history import, section 8.1).
- Layout switch: Rows, Grid, Free.
- Settings button. Appearance (Auto, Day, Night), the accent theme and theme cycling live in
  Settings, not in the top bar.
- On a session's workspace, right after the brand: back to the Board, the state dot and label,
  the title (click to rename), agent and model, usage, then Stop/Resume/Archive and "Reset
  layout". There is no separate workspace header. When space is short the agent, usage and
  state label hide first and the title shortens; below that the top bar wraps to a second row.

### 3.2 Your-move queue

Sessions that are `blocked`, or `ready`, and not snoozed. Blocked first, then ready; within each,
the one that has waited longest first. This order is the same everywhere it appears (the Your-move
menu, keyboard cycling, the browser tab title).

The Your-move control in the top bar is a menu of those sessions. On the board, picking one jumps
to that card. In a workspace (or anywhere else), picking one opens that session.

A `shell` session at its prompt is `ready` but is not in the queue: it gets no ready highlight
and no ready alert. A shell joins the queue only as `blocked` with reason `question`, when a
turn ends on a line that asks for an answer (a password, `[y/N]`, a line ending in `?`, "press
enter"). Typing a reply with Return starts a new turn.

### 3.3 Cards and strips

Each session is a **card**:

- Header: state dot and label, title (click to rename), agent and model, how long it has been in
  this state, cost and context percentage when known. With a goal: a pill with checks passed,
  the round, or the reason it paused.
- When blocked: a highlighted line with the reason and message.
- Body: the live terminal. You can type in it right on the board.
- Footer: workspace path or branch, the latest tool the agent used (name and a short detail),
  and action buttons: Open (workspace view), Snooze/Wake, Stop or Resume, Remove.
- Border colour follows state: warm for blocked, accent for ready, quiet for the rest.

A card can be collapsed to a **strip**: one line with the dot, title, state and time. Strips keep
their place in the layout. A strip that becomes blocked gets the highlight but does not expand on
its own.

Snoozed sessions are shown as strips in a "Snoozed" group at the bottom, dimmed.

### 3.4 Layouts

- **Rows:** cards stacked full width, oldest first. A card stays put when its state changes;
  the queue pill and the highlight show who needs you.
- **Grid:** as many columns as fit; cards fill the shortest column first.
- **Free:** every card has its own position and size on a large scrolling canvas. Drag the header
  to move it, drag any edge or corner to resize. Positions are remembered. "Tidy" lays the cards
  back out in a grid.

Exact sizes and rules are in `03-design-system.md` section 7.

### 3.5 Keyboard

| Keys | Action |
|---|---|
| `Alt+]` / `Alt+[` | Focus the next / previous session in queue order, then the rest |
| `Alt+N` | New session |
| `Alt+Enter` | Open the focused session's workspace |
| `Alt+Z` | Snooze or wake the focused session |
| `Alt+0` | Back to the board |
| `Escape` | Close the open dialog or menu |
| `Cmd+K` | Clear the focused terminal's screen and scrollback (local view) |

Keys typed while a terminal has focus go to the terminal, except the `Alt` shortcuts above and
`Cmd+K`.

### 3.6 Empty state

With no sessions, the board shows the brand mark, one sentence about what the tool does, a "New
session" button, and which agents were found on this machine (and which were not, with the
command name to install).

## 4. Alerts

When a session enters the queue (becomes blocked, or becomes ready):

- The browser tab title shows the count, for example `(2) SyncAiDync`, and the favicon gets a
  dot.
- A short sound plays. Blocked and ready use different sounds.
- A desktop notification appears if the page is not visible and notifications are allowed.
  Clicking it opens that session.
- If something stays blocked, a reminder repeats every few minutes (default 5) until handled or
  snoozed.

Settings (stored in the browser):

- Sounds on/off, volume.
- Desktop notifications on/off (asks the browser for permission when turned on).
- Reminder interval, or off.
- Alert on "ready" as well as "blocked" (default on).
- Accent theme (section 9).

Alerts never fire for a session you are looking at: the page is visible and that session's
terminal has focus.

## 5. Workspace view

Opening a session replaces the board with that session's workspace. A back button and `Alt+0`
return to the board.

### 5.1 Tools

Four tools, each in a panel:

- **Terminal:** the agent's terminal, full size.
- **Files:** a file tree and an editor (section 5.3).
- **Changes:** what changed since the session began (section 6).
- **Session:** details, activity and close-out (section 5.4).

### 5.2 Panels

- Panels sit on the same kind of canvas as the board's Free layout: move by the header, resize
  by any edge or corner.
- Drop one panel's tab onto another panel to stack them as tabs. Drag a tab out to give it its
  own panel again.
- Each panel has a "pop out" button that opens that tool alone in a new browser window. The
  panel in the main window shows "Open in another window" with a "Bring back" button until the
  window closes.
- Each panel has two opacity sliders in its menu, for seeing the terminal through a diff:
  Active (40–100%, while you work in it or no panel is active) and Inactive (20–100%, while
  you work in another panel). The panel you last clicked or typed in is the active one.
- "Reset layout" restores the default: Terminal on the left, the other three as tabs on the
  right.
- The arrangement is remembered per session.

### 5.3 Files

- Tree of the workspace folder. Folders load when opened. `.git` is never shown. Ignored
  folders such as `node_modules` and `target` are shown collapsed and greyed, and load only if
  opened. **New file** creates an empty file in an existing folder (path relative to the
  workspace) and opens it.
- Clicking a file opens it in the editor. Syntax colouring for common languages.
- Markdown (`.md`, `.markdown`) opens in **View** first: a rendered reading surface. A
  **View | Source** control switches to the usual editor. View shows the current buffer,
  including unsaved edits. The last choice is remembered.
- Relative images in markdown are loaded from the workspace through SyncAiDync (they never
  leave the session folder). Remote `https` image URLs are not shown in the page. Relative
  links to other workspace files open that file in Files.
- **Margins:** line-tied context that is not in the shipped source — markdown, pictures and
  diagrams kept under `.syncaidync/margins/` in the workspace (gitignored by default; you can
  opt in to commit). A gutter mark shows which lines have a margin. Click a mark (or "Add
  margin" on the current line) to read or edit it as rich markdown. Agents see a short index
  file and load a margin body only when they need it.
- `Cmd/Ctrl+S` saves. If the file changed on disk since it was opened (the agent edited it), the
  save is refused and you choose: reload theirs, or overwrite with yours.
- Unsaved files show a dot. Leaving with unsaved changes asks first.
- Files over 1 MiB or not UTF-8 text show a "can't open this here" note with the size.
- A filter box narrows the tree by name.

### 5.4 Session tool

- Facts: agent, model, autonomy, workspace path, branch and base, created, state history of the
  last few changes.
- Links: the GitHub repo, the branch, and its pull request if one exists.
- Activity: the last 20 tools the agent used, newest first.
- Goal (when set): each criterion and its checks with their last result and output, the
  baseline, meters for rounds, cost and time, past rounds, and Run checks now, Keep going,
  Accept change, Drop goal.
- Usage: cost, tokens and context.
- "Send a message": a text box that types into the agent's terminal and presses Enter. Useful
  from the Changes tool or a pop-out.
- Close out (section 8.2).

## 6. Changes

Shows everything the session changed: commits since its base plus uncommitted and untracked
files.

### 6.1 Reading

- Left: the list of changed files, grouped by package (the nearest folder with a manifest such
  as `Cargo.toml` or `package.json`). Each file shows its status (added, modified, deleted,
  renamed) and line counts.
- Right: the diff for the selected file, as hunks with line numbers. Unified view by default; a
  switch gives side-by-side.
- For markdown files, a **View | Diff** control defaults to **View**: the new file rendered as
  prose (same surface as Files). Notes work in View too (hover + beside a block, or note on the
  file). Diff keeps hunks, line numbers and notes. Deleted markdown shows an empty View.
- "Reviewed" checkbox per file. A progress count sits at the top. Reviewed marks are remembered,
  and cleared for a file when its diff changes.
- Next/previous hunk: `J` / `K`. Next/previous file: `Shift+J` / `Shift+K`. Toggle reviewed: `R`.
- "Refresh" re-reads the diff. The view also offers to refresh when the agent finishes a turn.

### 6.2 Change map

Above the file list, a small map: changed files as nodes, lines for imports between them, and
faint nodes for unchanged files that import a changed one ("depends on this"). Clicking a node
selects that file. The map helps you read providers before consumers.

### 6.3 Notes

- Click a line number (or select a range) to write a note on those lines. A file can also have a
  note not tied to a line, and the whole change set can have one overall note.
- Notes are kept with the review. Note text is shown as markdown (bold, lists, links).
- "Send notes to the agent" composes one message from all unsent notes (file, lines, the quoted
  lines, your text) and types it into the session. Sent notes are marked sent.

### 6.4 Read-through

"Plan my read-through" asks a model (any installed agent, your choice) to propose a reading
order: up to 8 **checkpoints**, each with a title, why it matters, what to look for, and the
hunks that belong to it. The view then walks you through them in order, with "Next checkpoint".

- Hunks the plan did not mention are collected in a final "Everything else" checkpoint, so
  nothing is hidden.
- If the diff changes after planning, the plan is marked out of date with a "Plan again" button.
- The plan is advice. It never marks anything reviewed for you.

## 7. Pull requests

Read someone else's pull request with the same tools.

- **Open:** paste a GitHub PR URL or `owner/repo#123`. SyncAiDync finds the matching local
  clone, fetches the PR, and shows it in the Changes view (map, checkpoints, notes, margins)
  with the PR's title, author and description on top. Margins live on that clone under
  `.syncaidync/margins/` (same sidecar as a session workspace).
- **List:** opened PRs are kept in a list with status `active` or `done`, with their review
  progress. Remove takes one off the list.
- **Draft review:** "Save as draft review on GitHub" posts your notes as a *pending* review on
  the PR: line notes as line comments, the overall note as the body. It is not submitted;
  you finish and submit it on GitHub.
- **Start a session on it:** creates a worktree at the PR's head and starts an agent there, for
  when you want to run or probe the code.
- Needs the `gh` CLI signed in. If `gh` is missing or signed out, the dialog says so and what to
  run.

## 8. Around the edges

### 8.1 History import

Lists past agent conversations found on this machine that SyncAiDync did not start (for the
agents that keep a readable history). Each row shows the folder, the first prompt and when it
was last active. "Import" adds it to the board as an `ended` session that can be resumed.

### 8.2 Close out and takeaways

When a session's work is done:

1. "Suggest takeaways" asks a model to read a condensed version of the session's transcript and
   propose up to 5 lessons worth keeping: a title, a one-paragraph body, and a suggested scope.
2. You edit, drop or keep each one and pick its scope:
   - **Everywhere:** the agent's global instructions file.
   - **Project:** the repo's instructions file.
   - **Note:** a notes file in the SyncAiDync home, not read by any agent.
3. "Save" appends the kept ones to those files under a "Takeaways (SyncAiDync)" heading, dated.
4. Then choose: stop the session, remove it, or leave it.

You can close out without takeaways.

### 8.3 Update notice

Once a day the server checks whether a newer release tag exists. If so, Settings shows "Version
X is available" with the command to update. Nothing updates itself.

## 9. Look and feel

- Night appearance by default, with Day and Auto (follows the time of day).
- Glass surfaces over a soft two-colour glow, rounded corners, an accent gradient on primary
  buttons. Details in `03-design-system.md`.
- Accent themes: the default pair, plus any pair from the bundled gradient list. A theme changes
  only the two accent colours; text contrast on accent fills is computed.
- Motion is brief and respects the reduced-motion setting.

## 10. macOS app

An optional wrapper app: the same UI in its own window, with a Dock badge for the queue count,
native notifications, and a global hotkey (⌃⌥⌘S) that brings it forward. Closing the window keeps
sessions running. Details in work order WO-19.

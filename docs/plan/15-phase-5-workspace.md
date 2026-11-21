# Phase 5: The workspace

## WO-10: Workspace view and tool panels

**Goal.** Opening a session shows its workspace: four tools in panels you can move, resize,
stack as tabs, fade, and pop out.

**Depends on.** WO-08.

**Read first.** `01-product.md` sections 5.1, 5.2; `03-design-system.md` section 8;
`02-architecture.md` sections 13.2, 13.4.

**Build.**

- `web/js/lib/tabgroups.js` (pure) as specified in the design doc, section 8.
- `dync-workspace`: route `#/s/<id>`. Its session controls (back button, the session's state
  dot and label, title (rename in place), agent and model, usage, Stop/Resume, "Reset layout")
  render into the top bar's session slot (`dync-topbar.sessionSlot()`), not a header of its
  own. The workspace itself is a `dync-canvas` holding one `dync-panel` per tab group. If the session id is unknown, show
  "That session is gone" with a link to the board.
- `dync-panel`: tab strip, the active tool's element, a menu with the opacity sliders (Active 40–100%, Inactive 20–100%,
  applied to the panel's background and content, not its header) and "Pop out", plus the move
  and resize behaviour from `dync-canvas`. All four tool elements stay mounted while their
  panel exists; inactive tabs are hidden with the `hidden` attribute, not removed (so a terminal
  or editor keeps its state).
- Tab drag using `chipDrag`: reorder within a strip, move to another group, tear out to a new
  panel. Layout saved to `syncaidync:workspace:<id>` after every change, read through
  `normalize`.
- Tool elements:
  - `dync-tool-terminal`: a `dync-terminal` in edit mode, filling the panel.
  - `dync-tool-files`, `dync-tool-changes`, `dync-tool-session`: placeholders for now, each a
    centred `.empty` naming the work order that fills it in. Register them so later work orders
    only replace the file.
- Pop-out: `window.open` on `#/s/<id>/<tool>?solo=1` sized 900 × 700, with a window name unique
  to session and tool so a second click focuses the existing window. The solo route renders
  only that tool, full window, with a slim header (session title, state, "Close"). The panel in
  the main window shows "Open in another window" and "Bring back" (which closes the pop-out).
  Track the window with a `BroadcastChannel` named `syncaidync`: the pop-out announces `open`
  and `closed` (`pagehide`); the main window updates its panel. Do not move DOM between windows.
- While the workspace is open, the board is not rendered (its terminals are disconnected), so
  each session has one edit terminal per window.
- `Alt+0` goes back to the board. `Alt+1`…`Alt+4` focus Terminal, Files, Changes, Session
  (activating the tab if it is behind another).
- `web/css/workspace.css`.

**Tests.** `tabgroups.test.js`: default layout proportions; `moveTab` within and across groups;
`tearOut`; emptied groups are dropped and `active` is repaired; `normalize` on junk input
(missing tool, duplicate tool, unknown tool, out-of-range rectangle, not an object) yields a
valid layout.

**Manual check, with screenshots.** Default layout; drag Changes out into its own panel; drop it
onto the Terminal panel as a tab; reorder tabs; resize panels; opacity at 40% over another
panel; reload (the arrangement is kept); pop out the terminal, type in the pop-out, bring it
back; "Reset layout". Both appearances.

**Done when.** A terminal's content and scroll position survive every tab and panel operation
except pop-out (which re-attaches and replays).

**Not in this order.** The contents of Files, Changes and Session.

---

## WO-11: Files: tree and editor

**Goal.** Browse and edit the workspace's files safely.

**Depends on.** WO-03 (server part), WO-10 (UI part).

**Read first.** `01-product.md` section 5.3; `02-architecture.md` sections 8 ("Files"), 11;
`00-ground-rules.md` section 7.

**Build: server.**

- `files.rs`: `resolve(root, rel) -> Result<PathBuf, ApiError>` implementing the containment
  rules exactly, then `list`, `find`, `read`, `write`. Run file-system work on blocking threads.
- `api/files.rs`: the four routes. They work for ended sessions too. A session whose workspace
  folder no longer exists → 404 with "This session's folder is gone."

**Build: UI.**

1. **Vendor the editor.** `scripts/vendor-codemirror.sh`:
   - In `.vendor-tmp/`: `npm init -y`, then install pinned exact versions of `codemirror`,
     `@codemirror/lang-javascript`, `@codemirror/lang-rust`, `@codemirror/lang-python`,
     `@codemirror/lang-json`, `@codemirror/lang-markdown`, `@codemirror/lang-html`,
     `@codemirror/lang-css`, `@codemirror/lang-yaml`, `@codemirror/lang-go`,
     `@codemirror/legacy-modes`, and `esbuild`.
   - Walk every installed package's `package.json` and fail unless its `license` is exactly
     `MIT` (print the offenders). Do this before bundling.
   - Write an entry module that re-exports what the app needs: `EditorView`, `basicSetup`,
     `EditorState`, `Compartment`, `keymap`, `indentWithTab`, `HighlightStyle`,
     `syntaxHighlighting`, `tags`, `StreamLanguage`, the language functions above, and the
     `shell` and `toml` legacy modes.
   - `esbuild entry.js --bundle --format=esm --minify --target=es2022 --outfile=web/vendor/codemirror/codemirror.js`.
   - Write `web/vendor/codemirror/LICENSES.txt` (each bundled package's name, version and full
     licence text) and `VERSION` (the pinned versions and date).
   - Run it, commit the output, add rows to `THIRD_PARTY_NOTICES.md`.
2. `web/js/lib/filetree.js` (pure): tree state (`expanded` set, loaded children by path),
   `flatten(state) -> rows` for rendering, `languageFor(path)` (by extension and a few file
   names such as `Dockerfile`, `Makefile`).
3. `dync-tool-files`:
   - Left: filter box and tree (`role="tree"`, arrow keys to move, right/left to open/close,
     `Enter` to open a file). Folders load on first open. Ignored folders are greyed and stay
     closed until asked. Typing in the filter switches to a flat result list from `/find`
     (debounced 200 ms).
   - Right: the editor, with a small header (path, unsaved dot, Save button, "Reload").
     One CodeMirror view; switching files swaps its state. Keep up to 10 files' states (content,
     scroll, selection, unsaved text) in memory so going back to a file is instant.
   - Theme: a CodeMirror theme and highlight style built from the tokens (background
     transparent, `--font-mono` 13px, gutters `--text-muted`, selection 30% accent; comments
     muted, strings `--ok`, keywords `--accent-ink`, numbers `--warn`, with the rest in
     `--text`). Rebuilt when appearance or accent changes, through a `Compartment`.
   - Save: `PUT` with the revision. On 409 `stale`, show a bar above the editor: "This file
     changed on disk." with "Reload theirs" and "Overwrite with mine" (which sends `force`).
   - When the store reports that the session's turn ended (`working` → `ready`), re-check the
     open file's revision; if it changed and there are no unsaved edits, reload quietly; if
     there are, show the same bar.
   - Leaving a file or the workspace with unsaved edits asks first. `beforeunload` is set while
     anything is unsaved.
   - Files that cannot be opened show the server's message.
   - The split between tree and editor is draggable (min 160px each) and remembered.
4. `web/css/files.css`.

**Tests.**

- Rust unit tests for `resolve`: `..`, an absolute path, empty segments, NUL, `.git` at any
  depth (`a/.git/config`), a symlinked folder pointing outside, a symlinked file as the last
  segment, a path through a symlinked folder that points inside (allowed), a root that is
  itself a symlink.
- Integration: list, read, write, a stale write (409), `force`, a file over 1 MiB (422), a
  non-UTF-8 file (422), writing a new file in an existing folder, writing into a missing folder
  (404), a file mode kept after writing, `find` in a git repo and in a plain folder.
- `filetree.test.js`: flatten with nested expansion; `languageFor`.

**Manual check, with screenshots.** Open a Rust file and a TypeScript file (colours in both
appearances). Edit and save; have the agent (or `echo` in the terminal) change the same file,
then save from the editor: the stale bar appears and both choices work. Filter finds a deep
file. Keyboard-only tree navigation.

**Done when.** No request can read or write outside the workspace or inside `.git` (the tests
prove it), and editing a file feels immediate.

**Not in this order.** Renaming or deleting files from the UI (the agent and the terminal do
that). Creating folders from the UI. Search inside files.

## Field notes

### 2026-11-20

- Files UI **New file**: creates via existing `PUT …/file` (empty text + revision) into an
  existing folder, reveals it in the tree, and opens it. Renaming, deleting, and new folders
  stay out.

### 2026-11-18

- WO-11 shipped: `files.rs` containment (raw `/`-split so mid-path `.` is rejected), tree/find/file
  routes, CodeMirror 6 vendored via `scripts/vendor-codemirror.sh` (MIT gate before bundle),
  `filetree.js` + `dync-tool-files` with stale bar, cache, and split remember.

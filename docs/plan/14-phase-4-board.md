# Phase 4: The board

## WO-07: Session card and strip

**Goal.** The real session card: everything you need to know about a session at a glance, and
the actions on it.

**Depends on.** WO-04, WO-06.

**Read first.** `01-product.md` sections 3.3, 3.6; `03-design-system.md` sections 1, 2, 6, 9, 14;
`00-ground-rules.md` section 5 (the words on screen).

**Build.**

- `web/js/lib/labels.js` (pure): `stateLabel(session)`, `blockedLabel(blocked)`,
  `agentLabel(meta, id)`, `usageLabel(usage)` (`$0.42 · 31%`, `1.2k tokens`, or empty),
  `workspaceLabel(workspace)` (branch for worktrees, last two path segments otherwise),
  `toolLabel(tool)`.
- `dync-session-card`: the anatomy in the design doc. Properties: `session`, `meta`,
  `collapsed`, `focused`. It renders its frame once and updates fields in place, so the
  terminal inside is never re-created by a state change. Events (bubbling): `dync-open`,
  `dync-snooze` (`detail.snoozed`), `dync-stop`, `dync-resume`, `dync-remove`,
  `dync-collapse` (`detail.collapsed`), `dync-rename` (`detail.title`), `dync-focus`.
  - Title: click, or `F2` when the card has focus, turns it into an input; `Enter` saves,
    `Escape` cancels, blur saves.
  - The `⋯` menu uses the shared menu helper (`web/js/lib/menu.js`: open below the button,
    close on outside click, `Escape`, or when another opens; arrow keys move, `Enter` picks).
  - Footer buttons: Open; Snooze or Wake (only when the session is in the queue or snoozed);
    Stop when live, Resume when ended.
  - Ended sessions show "Ended" with the exit code when it is not zero, and the terminal shows
    the replay, dimmed.
  - For agents that observe by activity, the state label has a small "approximate" hint in its
    `title`.
- `dync-session-strip`: the collapsed form. Same events.
- `dync-board` now uses these and handles their events by calling the API. Failures show a
  toast (`web/js/lib/toast.js`) with the server's message. Still a simple vertical list; layout
  comes next.
- `dync-empty-board`: the empty state from the product doc, section 3.6.
- `web/css/board.css`.

**Tests.** `labels.test.js`: every state and blocked kind; usage with one or both numbers;
workspace labels for the three kinds; long details cut.

**Manual check, with screenshots (night and day).** A card in each state (use the dev shell and
post signals by hand with `curl` to `/agent/v1/signal` to force states, or use a real agent).
Rename by click and by `F2`. Collapse and expand: the terminal keeps its content. The blocked
banner wraps long messages to two lines and ends with an ellipsis. Tab order through a card is
sensible.

**Done when.** A state change from the stream updates the card without the terminal flickering
or losing focus.

**Not in this order.** Layouts, the queue pill, alerts.

---

## WO-08: Session canvas: rows, grid, free placement

**Goal.** The three board layouts, with move and resize in Free.

**Depends on.** WO-07.

**Read first.** `01-product.md` sections 3.4, 3.5; `03-design-system.md` sections 7, 8;
`02-architecture.md` section 13.4.

**Build.**

- `web/js/lib/layout.js` (pure): `orderSessions(sessions)` (keyboard focus order),
  `boardOrder(sessions)` (card placement: oldest first, snoozed last), `rowsLayout(input)`,
  `gridLayout(input)`, `freeLayout(input)`, `nextFreeSlot(rects, width)`, `clampRect(rect)`,
  `raise(rects, id)`, `tidy(input)`. Input and output are plain objects; output is a list of
  `{ id, x, y, w, h, z, collapsed }` plus the canvas size.
- `web/js/lib/drag.js`: `startMove(options)`, `startResize(options)`, and `chipDrag(options)`
  (used in WO-10), as specified in the design doc, section 8. They take callbacks
  (`onFrame(rect)`, `onCommit(rect)`, `onCancel()`) and know nothing about sessions.
- `dync-canvas`: positions its children absolutely from a list of rectangles, animates
  position changes when not dragging (`transition: translate 0.28s var(--ease-out)`; none under
  reduced motion), and hosts the resize handles and the size chip. It is generic: WO-10 reuses
  it for tool panels.
- `dync-board` rebuilt on `dync-canvas`: computes rectangles from the chosen layout on session
  changes and container resizes (`ResizeObserver`). Cards are keyed by session id and moved, not
  re-created, when the order changes.
- Layout switch in the top bar (Rows, Grid, Free) and the "Tidy" button (only in Free). Stored
  in `syncaidync:board`. Stale ids are pruned.
- Keyboard: `Alt+]` and `Alt+[` cycle focus in the order from `orderSessions` and scroll the
  card into view; `Alt+Enter` opens; `Alt+Z` snoozes or wakes. The focused card gets a visible
  ring. These work while a terminal has focus: register them on `window` in the capture phase
  and stop the event so the terminal does not also receive it.
- `web/css/canvas.css`.

**Tests.** `layout.test.js`:

- Rows: order, heights at small, medium and large viewports, strips at 44px.
- Grid: column count at widths 300, 900, 1400, 2600 and with fewer cards than columns; the
  width clamp; shortest-column placement with mixed cards and strips.
- Free: the default slot sequence and wrapping; clamping at every limit; `raise` and the
  renumbering past 10,000; `tidy` equals the grid result; a collapsed card keeps its saved
  height.
- `orderSessions`: blocked, ready, working, launching, ended, snoozed last; ties by age.
- `boardOrder`: age order regardless of state; snoozed last.

**Manual check, with screenshots.** Six sessions in each layout. In Free: move a card, resize
from each of the eight handles, `Escape` mid-drag restores, auto-scroll at the edge, the
arrangement survives a reload. Dragging over a terminal does not select text or lose the drag.
Arrow-key move and `Alt`+arrow resize on a focused header. With reduced motion on, nothing
animates.

**Done when.** Dragging stays smooth with six live terminals on screen (no visible lag), and
switching layouts never re-creates a terminal.

**Not in this order.** Tool panels and tabs (WO-10).

---

## WO-09: Your-move queue, snooze, alerts, settings, themes

**Goal.** The board tells you when you are needed, in the way you chose.

**Depends on.** WO-07.

**Read first.** `01-product.md` sections 3.1, 3.2, 4, 9; `03-design-system.md` sections 3, 10,
12, 13; `02-architecture.md` section 13.4.

**Build.**

- `web/js/lib/queue.js` grows (pure): `diffQueue(previous, next) -> [{ id, kind }]` where kind is
  `blocked` or `ready`. An alert is due when a session enters `blocked` from any state, or
  enters `ready` from `working`. Not when it goes `launching` → `ready`, and not on the first
  `state` snapshot after page load.
- `web/js/lib/alerts.js`: subscribes to the store, uses `diffQueue`, and for each due alert:
  skips it if the page is visible and that session's terminal has focus; otherwise plays the
  sound, and shows a desktop notification when the page is hidden (title: the session title;
  body: "Blocked on you: …" or "Ready for you"; clicking focuses the window and opens the
  session). Keeps the document title (`(2) SyncAiDync`) and the favicon dot in step with the
  count. Reminder timer: every `remind_minutes`, if anything is still blocked and not snoozed,
  play `remind`. In a pop-out window (`solo=1`) alerts are off, so they do not double up.
  When `native.isNative()` is true, send notifications and the badge count through the native
  bridge instead of the browser's.
- `web/js/lib/sound.js` as specified.
- `web/js/lib/themes.js` (pure helpers plus `applyTheme`) and
  `scripts/vendor-uigradients.sh`: fetch `gradients.json` and the licence file from the
  `ghosh/uiGradients` repository on GitHub at a pinned commit (with `curl` from
  `raw.githubusercontent.com`), fail unless the licence text contains "MIT License", copy to
  `web/vendor/uigradients/` with a `VERSION` file. Add the row to `THIRD_PARTY_NOTICES.md`.
- Top bar: the Your-move pill. Clicking it, when the count is above zero, opens a menu of the
  queue. On the board, a pick focuses that card; elsewhere it opens that session's workspace.
- Snooze: the card button and `Alt+Z` call the API; snoozed sessions render in the "Snoozed"
  group.
- `dync-settings` dialog: sounds on/off, volume slider with a "Test" button, desktop
  notifications on/off (requests permission; shows "Blocked by the browser" with a hint when
  denied), reminder interval (Off, 2, 5, 10, 15 minutes), alert on ready, accent theme picker,
  and an "About" line with the version and home folder.

**Tests.**

- `queue.test.js`: every transition pair for `diffQueue`; first snapshot yields nothing;
  snoozed sessions never alert; a session that was blocked on approval and becomes blocked on a
  question alerts again.
- `themes.test.js`: `parseHex` (3 and 6 digits, bad input), `contrast` against known pairs
  (black/white is 21), `pickOnAccent` for a light and a dark accent, `readableInk` reaches 4.5
  or stops after 10 steps.
- `alerts.test.js`: the title text for counts 0, 1, 12; the reminder decision function.

**Manual check, with screenshots.** Force a session to `blocked` while another tab is in front:
sound, notification, title count and favicon dot. Click the notification: the session opens.
Snooze it: the count drops and no reminder comes. Three themes in both appearances: text on
primary buttons stays readable. Volume at zero is silent.

**Done when.** You can leave the board in a background tab and trust it to call you.

**Not in this order.** The mac app's native alerts (WO-19).

## Field notes

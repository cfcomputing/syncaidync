# Design system

SyncAiDync shares its look with the CF Computing / Sync The World apps: a near-black night
palette, violet-to-indigo accent, glass surfaces over a soft glow, rounded corners, and panels
you can move, resize and stack as tabs. The owner of that design has approved reusing it here.
Everything needed is on this page.

If `.local/STYLE-SOURCES.md` exists on your machine it points at the existing implementation of
these same pieces, for a closer match. This page is still the spec.

## 1. Tokens (`web/css/tokens.css`)

Night is the default. Component CSS uses these variables and nothing else for colour, radius,
shadow and easing.

```css
:root {
  --bg: #06060b;
  --surface: #101018;
  --surface-2: #181822;
  --text: #e8e8ed;
  --text-muted: #9a9aac;
  --accent: #a78bfa;
  --accent-2: #818cf8;
  --accent-ink: #a78bfa;          /* accent used as text or icon colour */
  --on-accent: #0b0716;           /* text on an accent fill */
  --danger: #ef4444;
  --on-danger: #0b0716;
  --ok: #34d399;
  --warn: #f5b84a;
  --on-warn: #1a1202;
  --border: rgba(255, 255, 255, 0.08);
  --border-width: 1px;
  --radius: 12px;
  --radius-sm: 8px;
  --radius-lg: 18px;
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.28);
  --shadow: 0 24px 48px rgba(0, 0, 0, 0.5);
  --shadow-lg: 0 32px 72px rgba(0, 0, 0, 0.55);
  --shadow-hover: 0 28px 56px rgba(0, 0, 0, 0.55);
  --shadow-inset: inset 0 1px 0 rgba(255, 255, 255, 0.04);
  --btn-shadow: 0 1px 2px color-mix(in srgb, var(--accent) 16%, transparent),
                0 2px 6px color-mix(in srgb, var(--accent) 10%, transparent);
  --grid-cell: 44px;
  --grid-line: color-mix(in srgb, var(--text) 5%, transparent);
  --grid-lit: 32%;
  --beam: 13%;
  --topbar: 56px;

  --glass: color-mix(in srgb, var(--surface) 74%, transparent);
  --glass-strong: color-mix(in srgb, var(--surface) 86%, transparent);
  --sheen: linear-gradient(180deg, color-mix(in srgb, #fff 9%, transparent), transparent 46%);
  --edge-light: inset 0 1px 0 color-mix(in srgb, #fff 16%, transparent);
  --glow: 0 10px 28px -10px color-mix(in srgb, var(--accent) 42%, transparent);
  --ring: 0 0 0 3px color-mix(in srgb, var(--accent) 24%, transparent);
  --hairline: linear-gradient(90deg, transparent,
              color-mix(in srgb, var(--accent) 55%, transparent) 22%,
              color-mix(in srgb, var(--accent-2) 45%, transparent) 62%, transparent);
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1);
  --spot: color-mix(in srgb, var(--accent) 13%, transparent);

  --font: "DM Sans", ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-mono: "IBM Plex Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace;

  accent-color: var(--accent);
  color-scheme: dark;
  scrollbar-color: color-mix(in srgb, var(--text-muted) 38%, transparent) transparent;
}

html[data-appearance='day'] {
  --bg: #f6f5fb;
  --surface: #ffffff;
  --surface-2: #efedf7;
  --text: #17152a;
  --text-muted: #5b5870;
  --accent: #6d4fe0;
  --accent-2: #4f5fe8;
  --accent-ink: #5a3fd0;
  --on-accent: #ffffff;
  --danger: #d92d3a;
  --on-danger: #ffffff;
  --ok: #0f9f6e;
  --warn: #a8650a;
  --on-warn: #ffffff;
  --border: rgba(20, 16, 50, 0.12);
  --shadow-sm: 0 1px 2px rgba(20, 16, 50, 0.08);
  --shadow: 0 24px 48px rgba(20, 16, 50, 0.14);
  --shadow-lg: 0 32px 72px rgba(20, 16, 50, 0.18);
  --shadow-hover: 0 28px 56px rgba(20, 16, 50, 0.16);
  --shadow-inset: inset 0 1px 0 rgba(255, 255, 255, 0.6);
  --grid-line: color-mix(in srgb, var(--text) 7%, transparent);
  --grid-lit: 26%;
  --beam: 7%;
  --sheen: linear-gradient(180deg, color-mix(in srgb, #fff 55%, transparent), transparent 52%);
  --edge-light: inset 0 1px 0 color-mix(in srgb, #fff 80%, transparent);
  --spot: color-mix(in srgb, var(--accent) 9%, transparent);
  color-scheme: light;
}
```

Fonts: no font files ship with the app. The stacks name DM Sans and IBM Plex Mono first so a
machine that has them installed matches the sibling apps; everyone else gets the system fonts.

State colours, used for dots, borders and labels:

| State | Colour |
|---|---|
| `blocked` | `var(--warn)` |
| `ready` | `var(--accent)` |
| `working` | `var(--ok)` |
| `launching` | `var(--text-muted)` |
| `ended` | `color-mix(in srgb, var(--text-muted) 55%, transparent)` |

## 2. Base (`web/css/base.css`)

- `* { box-sizing: border-box }`. `html, body`: margin 0, full height, `--bg` background,
  `--text` colour, `--font`, 15px base size, line height 1.45.
- **Inputs** (`input, select, textarea`): `--surface-2` fill, 1px `--border`, `--radius-sm`,
  padding `0.55rem 0.75rem`, full width. Focus: border `--accent` and
  `box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent)`. Hover: border
  mixes 32% accent.
- **Buttons.** A plain `<button>` is the primary action: `--accent` fill, `--on-accent` text, no
  border, weight 600, `--radius-sm`, `--btn-shadow`, plus a top-lit gradient
  (`linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent), transparent 55%, color-mix(in srgb, #000 6%, transparent))`)
  and `inset 0 1px 0 color-mix(in srgb, #fff 28%, transparent)`. Hover: `filter: brightness(1.06)`,
  lift 1px, add `--glow`. Active: `scale: 0.97`. Disabled: 50% opacity, no motion.
  - `button.ghost`: `--glass` fill, `--text-muted` text, 1px `--border`, `--edge-light`. Hover:
    `--text`, border mixes 40% accent.
  - `button.danger`: `--danger` fill, `--on-danger` text.
  - `button.sm`: padding `0.35rem 0.7rem`, font 0.8rem.
  - `button.icon`: 32px round, transparent, `--text-muted`; hover `--accent-ink` on `--bg`.
    Always has an `aria-label` and a `title`.
  - Focus-visible on buttons, links and tabs: `outline: 2px solid var(--accent-ink); outline-offset: 2px`.
- **Labels:** `label span` is 0.78rem, weight 600, `--text-muted`.
- **Card surface** (`.card`): `--glass` fill, `--sheen` as background image, 1px `--border`,
  `--radius`, `box-shadow: var(--shadow-sm), var(--edge-light)`. No `backdrop-filter` on cards
  (there can be many; it is slow). A pointer spot follows the mouse:
  `radial-gradient(22rem circle at var(--mx, -40rem) var(--my, -40rem), var(--spot), transparent 62%)`
  layered over the sheen; `spot.js` sets `--mx`/`--my` on the card under the pointer, throttled
  to one update per animation frame, and does nothing for touch or reduced motion.
- **Badge** (`.badge`): pill, 0.7rem uppercase, weight 600, letter-spacing 0.04em, 1px
  `--border`, `--text-muted`. `.badge.ok`, `.badge.warn`, `.badge.danger` colour the text and mix
  40% of that colour into the border.
- **Empty state** (`.empty`): centred, padding `2.2rem 1rem`, `--text-muted`, one sentence and
  at most one button.
- **Atmosphere:** a fixed, non-interactive layer behind everything. Do not use
  `filter: blur()`. The shapes are inline SVG drawn once at start-up by
  `web/js/lib/backdrop.js` (a fresh random seed each load); colours, fades and motion are CSS.
  The mesh: points on a 96px grid (1600×1000 view, one cell of overhang, scaled to cover),
  each nudged up to 38% of a cell, every cell split along a random diagonal (never one that
  leaves a sliver), so it reads as irregular triangles with no gaps. Layers, back to front:
  - `.mesh`: the mesh edges, 1px `--grid-line`, masked to fade down the page with a few
    fainter patches lower down.
  - `.mesh-lit`: the same edges at `--grid-lit` accent, masked to scattered patches (strong at
    the top right, faint elsewhere).
  - `.mesh-glints`: about 5% of triangles filled with `--poly-fill` accent and about 4.5% of
    corners as accent dots at `--spark`, each breathing between 15% and full opacity on its
    own 5–14s timing.
  - `.mesh-sparks`: seven short accent dashes at `--spark`, each running the length of one
    row or column of mesh points (36–70s a lap).
  - `.beam`: a 118° band from `--accent` to `--accent-2` at `--beam`, masked toward the top
    right, drifting ±4% sideways over 26s.
  Reduced motion stops every animation.
- `::selection`: 30% accent.

## 3. Chrome (`web/css/chrome.css`)

- **Top bar:** sticky, height `--topbar`, glass
  (`linear-gradient(180deg, color-mix(in srgb, var(--surface) 82%, transparent), color-mix(in srgb, var(--surface) 62%, transparent))`
  with `backdrop-filter: blur(22px) saturate(185%)`), `--edge-light`, and a 1px `--hairline`
  along the bottom edge at 75% opacity.
- **Pill cluster:** groups of icon buttons sit in a pill: 1px `--border`, 999px radius, 3px
  padding, 2px gap. The active one has a 12% accent fill and `--accent-ink` text. Used for the
  layout switch and the workspace tool switch. (Appearance and the accent theme are in
  Settings: Appearance is a segmented Auto/Day/Night switch, then a "Cycle through themes"
  checkbox above the theme picker.)
- **Your-move pill:** count plus label; opens the Your-move menu. With a count above zero: `--warn` text, border mixing 45%
  warn, and a slow pulse on the dot (2.4s, off for reduced motion). At zero: muted, reads
  "All clear".

## 4. Floating surfaces (`web/css/dialogs.css`)

- **Dialog:** use the native `<dialog>` element with `showModal()`. Backdrop:
  `color-mix(in srgb, var(--bg) 55%, rgba(0,0,0,0.45))` with `backdrop-filter: blur(6px) saturate(140%)`.
  Surface: `--glass-strong` plus `--sheen`, `--radius-lg`, `box-shadow: var(--shadow-lg), var(--edge-light)`,
  width `min(34rem, calc(100vw - 2rem))`, padding 1.25rem. Title is an `<h2>` at 1rem.
  Actions are right-aligned: ghost "Cancel" then the primary. Arrive:
  `from { opacity: 0; scale: 0.94; translate: 0 10px }` over 0.22s `--ease-out`.
- **Menu / popover:** `color-mix(in srgb, var(--surface) 82%, transparent)` with
  `backdrop-filter: blur(18px) saturate(180%)`, `--radius-sm`, `--shadow-lg`. Items are full-width
  rows; hover is a 12% accent fill. Arrive: `from { opacity: 0; scale: 0.96; translate: 0 -4px }`
  over 0.2s. Closes on outside click, `Escape`, and when another menu opens (one shared helper).
- **Toast:** bottom centre, glass pill, 4 seconds, `role="status"`.
- All arrive animations sit inside `@media (prefers-reduced-motion: no-preference)`.

## 5. The base element (`web/js/lib/element.js`)

```js
export class DyncElement extends HTMLElement {
  connectedCallback()    // calls this.mount()
  disconnectedCallback() // runs every cleanup, then this.unmount()
  mount() {}             // override: render and subscribe
  unmount() {}           // override if needed
  cleanup(fn)            // register something to undo on disconnect
  on(target, type, handler, options)   // addEventListener + cleanup
  every(ms, fn)          // setInterval + cleanup
  render(markup)         // this.innerHTML = markup (markup must come from html``)
}
export function escapeText(value)      // & < > " ' escaped; null/undefined become ''
export function html(strings, ...values)
```

`html` is a tagged template that escapes every interpolated value with `escapeText`, except
values wrapped by `raw()` (which only `html` results and `markdown.js` produce). Arrays are
joined. This makes escaping the default instead of something to remember.

Rules for elements:

- Light DOM only. Styles live in the css files, keyed by the element's tag name or classes.
- Data in through properties (`el.session = …`) or attributes for simple strings; data out
  through bubbling `CustomEvent`s named `dync-<thing>` (for example `dync-open`,
  `dync-snooze`).
- An element that holds something heavy (a terminal, an editor) must not re-create it on every
  render. Render the frame once in `mount()`, then update text and attributes in a separate
  `update()`.

## 6. The terminal element

`<dync-terminal session="<id>" mode="edit|view">` wraps the vendored terminal library.

- Font: `--font-mono`, 14px, line height 1.25. The terminal uses `--term-bg` and
  `--term-fg`, tinted slightly toward the accent, with the accent for the cursor and selection.
  The ANSI colours lean toward the accent (blue and magenta most; red, green and yellow least,
  so errors, success and warnings stay recognisable), and each is lightened until it reaches
  4.5:1 contrast on the terminal background. The palette is rebuilt on `dync-appearance` and
  `dync-theme`.
- Fit to its box with the fit addon, driven by a `ResizeObserver`, debounced 80 ms. In `edit`
  mode send `resize` to the server, including after the server reports its size on connect, so
  the grid never has rows below the visible box. Live Board previews also request a grid that
  fits the card after replay and when the card changes size. They send keystrokes only after
  entering edit mode. Ended sessions use the replay grid, shrinking it to fit narrow cards and
  keeping the latest prompt or the bottom of the alternate screen in view.
- Reconnect the socket with backoff (0.5s, 1s, 2s, then 5s). On reconnect, reset the terminal
  before the replay so text is not doubled.
- Board cards use `view` mode until clicked; a click switches that card to `edit` and gives it
  focus. Starting to resize a card does the same, so its terminal refits and reflows while the
  card is resized (a `view` terminal may not resize the PTY). Only one terminal per session is in `edit` mode in a window at a time.
- Scrollback up to 100,000 lines in the session Terminal and 5,000 on the Board.

## 7. Board layouts (`web/js/lib/layout.js`)

Pure functions: given the container width, viewport height, and the list of sessions (with
collapsed flags), return rectangles. Gap is 16px everywhere. A strip is 44px tall.

**Rows.** Every card is full width. Card height = `clamp(42% of viewport height, 260, 380)`.
Order: by `created_at`, oldest first, whatever the state, so a card never moves because its
session changed state. Snoozed sessions last, as strips. (Keyboard focus still cycles in queue
order; see section 3.2 of the product doc.)

**Grid.**

- Columns = `clamp(floor((W + 16) / (440 + 16)), 1, n)` where `n` is the number of cards.
- Column width = `(W − 16 × (columns − 1)) / columns`, clamped to 320…960. If clamping leaves
  spare width, centre the grid.
- Card height = `clamp((viewport height − top bar − 3 × 16) / 2, 300, 640)`, so two rows of
  cards fit on screen.
- Placement: in order, each card (or strip) goes to the currently shortest column; ties go left.

**Free.**

- Each card has `{ x, y, w, h, z }` in pixels on a canvas that scrolls both ways.
- Limits: `x`, `y` in 0…5000; `w` in 320…3000; `h` in 240…3000. Values are whole pixels.
- A card with no saved rectangle gets the next free slot: start at (24, 24), size 560 × 380, step
  right by `w + 16`; wrap to a new row when the next card would pass `max(W, 1200)`.
- Grabbing a card raises it: `z` becomes the highest `z` plus one. When `z` passes 10,000,
  renumber all cards from 1 keeping order.
- The canvas size is the bounding box of all cards plus 400px of room right and below.
- "Tidy" replaces every rectangle with the Grid result at the current width.
- A collapsed card in Free keeps `x`, `y`, `w` and shows at strip height; expanding restores `h`.

## 8. Move, resize and tabs (`web/js/lib/drag.js`, `web/css/canvas.css`)

Used by Free layout on the board and by the workspace panels.

**Move.** Pointer down on the header's drag area (not on a button, input or the title while it
is being edited) captures the pointer. While moving: the card follows the pointer with
`transform` updates batched to one per animation frame; it gets a stronger shadow, 97% opacity
and `cursor: grabbing`; `body` gets a class that turns off text selection and sets
`pointer-events: none` on terminals and editors so they do not eat pointer events. Near the
canvas viewport's edge (within 48px) the canvas auto-scrolls, faster the closer to the edge, up
to 24px per frame. On release, clamp to the limits and save. `Escape` cancels and restores.

**Resize.** Eight handles: four edges (12px thick, inset 12px from the corners) and four corners
(14px square). Handles are transparent until hovered; then a 3px rounded accent bar (edges) or a
small rounded square (corners) fades in. Cursors: `ew-resize`, `ns-resize`, `nwse-resize`,
`nesw-resize`. While resizing: updates batched per frame, a size chip at the top centre of the
panel shows `W × H` (accent pill, 0.72rem), `body` gets the matching cursor. West and north
edges move the origin as well as the size. Respect the minimums. On release, save. `Escape`
cancels.

**Keyboard.** A focused panel header moves with the arrow keys (16px; 1px with `Shift`), and
resizes with `Alt` + arrows. This is the accessible path; keep it.

**Tabs.** Model in `web/js/lib/tabgroups.js`:

```js
// a workspace layout
{ groups: [ { id, tabs: ["terminal"], active: "terminal", rect: { x, y, w, h, z }, opacity: 1, idleOpacity: 1 }, ... ] }
```

Pure operations, each returning a new layout: `moveTab(layout, tool, toGroupId, index)`,
`tearOut(layout, tool, rect)` (a new group at `rect`), `setActive`, `setRect`, `setOpacity`,
`tidy(layout)` (drops empty groups, fixes `active`), `defaultLayout(width, height)`,
`normalize(layout)` (every tool appears exactly once; unknown tools dropped; missing tools added
to the last group; rectangles clamped). `normalize` runs on everything read from storage.

Tab strip: tabs are text buttons with a 2px accent underline on the active one. Dragging a tab
starts after 5px of movement and shows a small accent chip with the tool's name following the
pointer. Drop targets: another group's tab strip (an insertion bar shows before or after the tab
under the pointer) or its body (dashed accent outline) to join that group; anywhere else on the
canvas tears the tool out into its own panel at that point, 520 × 360. `Escape` cancels. A click
that ends a drag does not also activate the tab.

Default workspace layout for a canvas of `W × H` (16px margins, 16px gap): Terminal in one group
on the left at 58% width, full height; Files, Changes, Session as tabs in one group on the right
with Changes active.

## 9. Session card anatomy

```text
┌──────────────────────────────────────────────────────────────┐
│ ● Blocked on you   Fix the login redirect        3m   ⌄  ⋯   │  header (drag area)
│   Claude Code · opus     $0.42 · 31%                          │
├──────────────────────────────────────────────────────────────┤
│ ⚠ Needs approval: Bash: rm -rf build                         │  only when blocked
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  terminal                                                    │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│ dync/fix-login-redirect   Bash · cargo test     Open  Snooze │  footer
└──────────────────────────────────────────────────────────────┘
```

- Border: 1px. Blocked: border mixes 60% `--warn` and the card gets
  `box-shadow: 0 0 0 3px color-mix(in srgb, var(--warn) 18%, transparent)`. Ready: border mixes
  55% `--accent`. Working (not snoozed): border mixes 30% `--accent`, a soft accent glow, and
  a 2px ring (1.5px on a strip) of a conic gradient from `--accent` to `--accent-2` with gaps,
  turning once every 3.2s (an `@property` angle). Reduced motion leaves the ring still.
  Others: `--border`.
- The state dot pulses only for `working` (slow) and `blocked` (a little faster). No pulse under
  reduced motion.
- `⌄` collapses to a strip, `⋯` opens the menu (Rename, Stop or Resume, Remove).
- The time is how long the session has been in its state, refreshed every 10 seconds from
  `state_since`: `45s`, `3m`, `1h 12m`, `2d 4h`.
- Long titles and paths end with an ellipsis; the full text is in `title`.

Strip: `● Fix the login redirect · Blocked on you · 3m` and an expand button, 44px tall, same
border rules.

## 10. Accent themes (`web/js/lib/themes.js`)

- A theme is a name and two colours. "Default" uses the token values. The rest come from the
  vendored gradient list: use entries with at least two colours, taking the first and last.
- Applying a theme sets `--accent`, `--accent-2`, `--accent-ink` and `--on-accent` on
  `document.documentElement`.
- `--on-accent` is whichever of `#0b0716` and `#ffffff` has the higher WCAG contrast against
  `--accent`. If neither reaches 4.5:1, the `--accent` fill is nudged away from that ink in 2%
  steps until it does (`accentFill`).
- `--accent-ink` must reach 4.5:1 against the current `--bg`, `--surface` and `--surface-2`. If the accent does not, mix it
  toward `--text` in 8% steps until it does (at most 10 steps).
- The picker shows swatches (a two-colour gradient chip and the name) with a search box. It
  skips themes whose first colour is within 1.3:1 of `--bg`.
- Pure helpers with tests: `parseHex`, `luminance`, `contrast`, `mix`, `pickOnAccent`,
  `readableInk`.

## 11. Appearance (`web/js/lib/appearance.js`)

- Preference: `auto`, `day`, `night`. Sets `data-appearance="day|night"` on `<html>`.
- `auto` is day from 07:00 to 19:00 local time, re-checked every minute.
- Set the attribute before first paint to avoid a flash: a small blocking script file in
  `<head>` (`<script src="/js/boot-appearance.js">`). It must be a file, not inline, so the
  content security policy holds.
- Changing it adds a class on `<html>` for 300 ms that transitions `background-color`, `color`
  and `border-color` everywhere, so the switch eases.

## 12. Sound (`web/js/lib/sound.js`)

No audio files. Tones are made with the Web Audio API: for each note, a sine oscillator into a
gain node with a 10 ms attack and an exponential decay.

| Sound | Notes (Hz, start offset s, length s) | When |
|---|---|---|
| `blocked` | 587.33 @0 0.16, 440.00 @0.14 0.24 | A session becomes blocked |
| `ready` | 523.25 @0 0.12, 783.99 @0.10 0.22 | A session becomes ready |
| `remind` | 440.00 @0 0.10, 440.00 @0.22 0.10 | Reminder |

Peak gain is `0.12 × volume`. Create the `AudioContext` on the first user gesture; before that,
sounds are skipped silently.

## 13. Brand mark (`web/favicon.svg` and inline in the top bar)

A new mark, drawn for this app. 32 × 32 viewBox, no background rectangle:

- A ring of radius 12 centred at (16, 16), stroke width 2.5, round caps, drawn as two arcs with
  a gap at the upper right and the lower left (each gap about 40°), stroked with a linear
  gradient from `--accent` (top left) to `--accent-2` (bottom right). It reads as two arrows
  chasing each other without arrowheads: "sync".
- Inside it, a terminal prompt: a `>` chevron (points (11, 12), (15, 16), (11, 20), stroke 2.5,
  round joins) and a short underscore from (17, 20) to (21, 20), both in `currentColor`.

In `favicon.svg` use literal colours (`#a78bfa`, `#818cf8`, and `#e8e8ed` for the prompt).
The alert favicon is generated at runtime: the same SVG plus a filled circle of radius 5 at
(26, 26) in the warn colour, set as a `data:` URL on the `<link rel="icon">`.

Wordmark: "SyncAiDync" in `--font`, weight 700, letter-spacing −0.02em, with "Ai" in
`--accent-ink`.

## 14. Accessibility floor

- Everything works by keyboard. Dialogs trap focus (native `<dialog>` does) and return focus to
  the button that opened them.
- State is never colour alone: every dot has a text label next to it.
- The queue count is in an `aria-live="polite"` region.
- Text contrast is at least 4.5:1 in both appearances; check with the helpers in `themes.js`.
  `web/test/contrast.test.js` checks every colour token pair, including state text on its
  tinted chip. Form fields use `--control-border` (3:1); `--border` is a decorative hairline.
  Do not dim text with `opacity`; use `--text-muted` instead.
- Touch targets and icon buttons are at least 32px.

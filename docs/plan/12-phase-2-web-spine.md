# Phase 2: Web spine

## WO-04: Web spine: tokens, shell, terminal element

**Goal.** The UI's foundation: the look, the base element, sign-in, the live store, routing, and
a working terminal element. At the end the page lists sessions as plain cards with live
terminals. It is not pretty yet; it is correct.

**Depends on.** WO-02. WO-03 is needed to see a live terminal; without it, build against the
API shapes and test what can be tested.

**Read first.** `00-ground-rules.md` sections 2, 6; `02-architecture.md` sections 6 (items 4, 9),
9, 13; `03-design-system.md` sections 1–6, 11, 13, 14.

**Build.**

1. **Vendor the terminal library.** `scripts/vendor-xterm.sh`:
   - Works in `.vendor-tmp/` (ignored by git), removes it at the end.
   - `npm pack @xterm/xterm@6.0.0` and `npm pack @xterm/addon-fit@0.11.0`, unpack.
   - Before copying, read each `package/package.json` and fail unless `license` is exactly `MIT`.
   - Copy to `web/vendor/xterm/`: `lib/xterm.mjs` → `xterm.mjs`, `css/xterm.css` → `xterm.css`,
     `lib/addon-fit.mjs` → `addon-fit.mjs`, and both `LICENSE` files as `LICENSE-xterm` and
     `LICENSE-addon-fit`.
   - Write `web/vendor/xterm/VERSION` with the two package names, versions and the date.
   - **Verify** that the two `.mjs` files have no bare `import` of another package (they must
     load in a browser as they are). If one does, stop and report.
   - Run it and commit the result. Add the rows to `THIRD_PARTY_NOTICES.md`.
2. **CSS:** `tokens.css`, `base.css`, `chrome.css`, `dialogs.css` as specified. `index.html`
   links them in that order, then the vendored `xterm.css`, then the app's other css files
   (empty for now, so later work orders only add rules).
3. **`index.html`:** `<html lang="en" data-appearance="night">`, meta viewport, title, favicon
   link, `<script src="/js/boot-appearance.js"></script>` in the head, the atmosphere layer,
   `<dync-app></dync-app>`, and `<script type="module" src="/js/main.js"></script>`. No inline
   script or style.
4. **`web/favicon.svg`:** the brand mark.
5. **Library modules** (`web/js/lib`): `element.js`, `api.js`, `store.js`, `route.js`,
   `prefs.js`, `appearance.js`, `time.js`, `queue.js` (order and counts only; alerts come in
   WO-09), `markdown.js`, `native.js` (exports `isNative()` returning false and no-op functions
   for now).
   - `api.js`: `api.get(path)`, `api.post(path, body)`, `api.put`, `api.patch`, `api.del`. Sends
     `credentials: 'same-origin'`. On a non-2xx, throws an `ApiError` with `code`, `message`,
     `status` from the error shape. On 401, also fires a `dync-unauthorized` event on `window`.
   - `route.js`: `parse(hash) -> { name, id, tool, solo }`, `format(route) -> hash`,
     `go(route)`, and a `route` change event. Unknown hashes parse as the board.
   - `prefs.js`: `get(key, fallback)`, `set(key, value)`, JSON-encoded, never throws (storage
     can be full or blocked), validates the shape on read with a per-key function.
6. **`main.js`:** read `#key=`, strip it from the address bar, `POST /auth`; then `GET /auth`.
   If not signed in, render the "open from the printed link" screen (brand mark, one sentence,
   and the command `syncaidync` to run). Otherwise load Meta, start the store, define the
   elements, mount the app.
7. **Elements** (`web/js/components`):
   - `dync-app`: listens to the route, shows the board or a placeholder for other routes; shows
     "Reconnecting…" in the top bar when the stream is down.
   - `dync-topbar`: brand mark and wordmark, the appearance switch (three icon buttons in a
     pill). Other buttons arrive in later work orders; leave named slots (`data-slot="…"`).
   - `dync-terminal`: as specified in the design doc, section 6.
   - `dync-board` (temporary, simple): a list of `.card`s, one per session, each with title,
     state label, and a `dync-terminal` in view mode that switches to edit on click. A "New
     shell session" button appears when Meta lists the `shell` agent; it creates a scratch
     session. This element is rebuilt in WO-07 and WO-08; keep it short.
8. **Icons:** `web/js/lib/icons.js` exports small inline SVG strings drawn for this project
   (24 × 24, `stroke="currentColor"`, stroke width 1.75, round caps and joins, no fill): plus,
   sun, moon, auto (half-filled circle), gear, grid, rows, free (overlapping rectangles),
   chevron-down, chevron-right, dots, play, stop, trash, external, back, bell, snooze (crescent
   with a "z"), file, folder, diff, terminal, info, search, check, close, pull-request, import.
   Draw them yourself from simple lines, circles and arcs. Do not copy paths from an icon set.

**Tests** (`web/test`).

- `route.test.js`: every route in the architecture doc parses and formats back; junk parses as
  the board.
- `queue.test.js`: order (blocked before ready, longest wait first, snoozed excluded), counts.
- `time.test.js`: `45s`, `3m`, `1h 12m`, `2d 4h`, and zero or negative input.
- `element.test.js`: `escapeText` and the `html` tag (escapes values, joins arrays, passes
  `raw()` through, treats null as empty). Keep `element.js` importable under Node by not touching
  `HTMLElement` at module load when it is undefined (define the class against a stand-in base).
- `prefs.test.js`: a fake storage that throws; bad JSON; wrong shape falls back.

**Manual check, with screenshots (night and day).** Open the printed URL: the key disappears
from the address bar; reload works (cookie). Open `http://127.0.0.1:7717/` in a private window:
the "open from the printed link" screen. Create a shell session, type `ls`, resize the window
(the terminal refits), reload (the replay restores the screen), stop the server and start it
again (the top bar shows "Reconnecting…" then recovers, and after signing in again the session
is listed as ended).

**Done when.** The checks above pass, there are no console errors, and the browser's network
panel shows no request to any other host.

**Not in this order.** The real card, layouts, dialogs, alerts.

## Field notes

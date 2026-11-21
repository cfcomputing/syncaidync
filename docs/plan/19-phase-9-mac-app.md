# Phase 9: macOS app

## WO-19: macOS app

**Goal.** An optional native wrapper: the same UI in its own window, with a Dock badge, native
notifications and a global hotkey. Written in Swift with only Apple's frameworks.

**Depends on.** WO-09.

**Read first.** `01-product.md` sections 4, 10; `02-architecture.md` sections 4, 6;
`00-ground-rules.md` sections 2, 4.

**Build** (in `mac/`).

- `mac/SyncAiDync.swift` (one file is fine; split if it passes about 600 lines), using AppKit,
  WebKit, UserNotifications and Carbon (for the hotkey). No packages.
- **Starting the server.** On launch: pick a free loopback port (bind port 0, read it, close),
  make a 64-hex-character key from `SecRandomCopyBytes`, and start the bundled binary
  (`Contents/Resources/syncaidync`) with `serve --no-open --port <p> --parent-pid <own pid>`
  and `SYNCAIDYNC_KEY` in its environment. Give it a login-shell `PATH` so agent CLIs are
  found: run the user's shell once as `$SHELL -l -c 'printf %s "$PATH"'` (2 s timeout) and use
  the result, falling back to the current `PATH` plus `/opt/homebrew/bin:/usr/local/bin`.
  Send the server's output to `~/Library/Logs/SyncAiDync/server.log` (keep the last 1 MiB).
- **Window.** One `NSWindow` (1280 × 820, remembered frame, title "SyncAiDync", full-size
  content view with a transparent title bar) holding a `WKWebView`. Poll
  `http://127.0.0.1:<p>/api/v1/auth` until the server answers (up to 10 s), then load
  `http://127.0.0.1:<p>/#key=<key>`. If the server does not come up, show a local HTML page
  with the reason and the last 40 lines of the log, and a "Try again" button.
- **Navigation policy.** Allow only `http://127.0.0.1:<p>/`. Any other link opens in the
  default browser. `window.open` for the app's own origin (pop-out tools) opens a second
  `NSWindow` with its own `WKWebView` sharing the same data store (so the cookie is shared).
- **Bridge.** Add a script message handler named `syncaidync`. Messages from the page:
  - `{ type: "badge", count }` → Dock badge (empty string for zero).
  - `{ type: "notify", id, title, body }` → a `UNUserNotificationCenter` notification
    (request permission on first use). Clicking it activates the app and dispatches
    `window.dispatchEvent(new CustomEvent("syncaidync:open", { detail: { id } }))` in the page.
  - `{ type: "ready" }` → the page is up (used to hide the loading state).
  Inject a user script at document start that sets `window.__syncaidyncNative = true`.
  In the web app, fill in `web/js/lib/native.js`: `isNative()`, `setBadge(count)`,
  `notify({ id, title, body })`, and a listener for `syncaidync:open` that routes to the
  session. `alerts.js` already calls these when native.
- **Hotkey.** ⌃⌥⌘S registered with Carbon's `RegisterEventHotKey`: bring the app forward and
  focus the window; if it is already frontmost, hide the app.
- **Lifecycle.** Closing the window hides it; sessions keep running; clicking the Dock icon
  shows it again. ⌘Q asks "Quit SyncAiDync? Running sessions will be stopped." when any session
  is live (ask the page for the count), then terminates the server (SIGTERM, wait up to 4 s)
  and exits. If the server process dies on its own, show the error page.
- **Menus.** App menu (About, Hide, Quit), Edit (Undo, Redo, Cut, Copy, Paste, Select All; these
  are required for those keys to work in a web view), View (Reload, Actual Size, Zoom In, Zoom
  Out), Window (Minimize, Zoom, Bring All to Front).
- **Icon.** `mac/icon.svg`: the brand mark on a rounded-square night-palette tile. `build.sh`
  renders it to an `.icns` with `sips` and `iconutil` (both ship with macOS). If rendering SVG
  with `sips` does not work on this macOS version, draw the icon in Swift with Core Graphics in
  a small `mac/make-icon.swift` instead.
- `mac/Info.plist`: bundle id `com.cfcomputing.syncaidync`, name SyncAiDync, version from
  `Cargo.toml`, minimum macOS 13, `NSAppTransportSecurity` allowing local networking only.
- `mac/build.sh`: `cargo build --release`, `swiftc -O` the Swift file with the frameworks it
  needs, assemble `SyncAiDync.app` (binary, `Info.plist`, icon, the server in `Resources`),
  ad-hoc sign (`codesign --force --deep --sign -`), and install to `~/Applications/`. Print the
  path. Works offline.
- README section "macOS app": how to build it, and that it is unsigned (ad-hoc), built locally.

**Tests.** `swiftc -typecheck` on the Swift source as part of `mac/build.sh`. No UI tests.
The web side: `native.test.js` for the message shapes `native.js` posts (inject a fake
`window.webkit`).

**Manual check, with screenshots.** Build and open the app. A session becomes blocked while the
app is in the background: Dock badge and a notification; clicking the notification opens that
session. The hotkey toggles the app. Copy and paste work in a terminal and in the editor. A
pop-out tool opens as a second window and stays signed in. Close the window, reopen from the
Dock: sessions are still running. ⌘Q with a live session asks first; after quitting, no
`syncaidync` process remains (`pgrep -fl syncaidync`).

**Done when.** The app can be the only way you use SyncAiDync for a day.

**Not in this order.** Notarization, auto-update, a menu-bar item, Linux or Windows wrappers.

## Field notes

- The app bundle builds and passes the local ad-hoc signature check. This macOS version needed the Core Graphics icon fallback.
- A demo verified startup, session launch, workspace layout, Close out, and the quit confirmation. A full day of use, notification delivery and activation, and the complete keyboard and pop-out checks are still open.
- Notifications now fire when the main window is hidden (⌘W) even if the app process is still active. Remaining: live notification click-through and a full day as the only SyncAiDync surface.

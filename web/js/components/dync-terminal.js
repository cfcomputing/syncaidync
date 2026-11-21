import { Terminal } from "/vendor/xterm/xterm.mjs";
import { FitAddon } from "/vendor/xterm/addon-fit.mjs";
import { DyncElement, html } from "../lib/element.js";
import { store } from "../lib/store.js";
import { api } from "../lib/api.js";
import { showToast } from "../lib/toast.js";
import { contrast, mix, parseHex } from "../lib/themes.js";

const BACKOFF = [500, 1000, 2000, 5000];
const editOwners = new Map(); // sessionId -> element

function cssVar(name, fallback) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

function hexVar(name, fallback) {
  const v = cssVar(name, fallback);
  return parseHex(v) ? v : fallback;
}

function rgba(hex, alpha) {
  // xterm needs a concrete colour, not color-mix().
  const [r, g, b] = parseHex(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

// Lighten until the colour reads on the terminal background.
function readable(color, bg, min = 4.5) {
  let c = color;
  for (let i = 0; i < 12 && contrast(c, bg) < min; i++) c = mix(c, "#ffffff", 0.14);
  return c;
}

const ANSI = {
  red: "#f87171",
  green: "#4ade80",
  yellow: "#fbbf24",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#22d3ee",
};

// How far each ANSI hue leans toward the theme. Red, green and yellow lean
// least so errors, success and warnings still look like themselves.
const LEAN = { red: 0.12, green: 0.14, yellow: 0.12, cyan: 0.28 };

/** Terminal palette built from the current theme tokens. */
export function themeFromTokens() {
  const accent = hexVar("--accent", "#a78bfa");
  const accent2 = hexVar("--accent-2", "#818cf8");
  const bg = mix(hexVar("--term-bg", "#0c0c14"), accent, 0.06);
  const fg = mix(hexVar("--term-fg", "#ececf2"), accent, 0.05);
  const dim = readable(mix(hexVar("--term-dim", "#a4a4b8"), accent, 0.12), bg);

  const normal = {
    red: readable(mix(ANSI.red, accent, LEAN.red), bg),
    green: readable(mix(ANSI.green, accent, LEAN.green), bg),
    yellow: readable(mix(ANSI.yellow, accent, LEAN.yellow), bg),
    blue: readable(mix(ANSI.blue, accent2, 0.55), bg),
    magenta: readable(mix(ANSI.magenta, accent, 0.65), bg),
    cyan: readable(mix(ANSI.cyan, accent2, LEAN.cyan), bg),
  };
  const bright = (c) => readable(mix(c, "#ffffff", 0.3), bg, 7);

  return {
    background: bg,
    foreground: fg,
    cursor: readable(accent, bg, 3),
    cursorAccent: bg,
    selectionBackground: rgba(accent, 0.38),
    selectionInactiveBackground: rgba(accent, 0.22),
    black: mix("#1a1a24", accent, 0.12),
    ...normal,
    white: fg,
    brightBlack: dim,
    brightRed: bright(normal.red),
    brightGreen: bright(normal.green),
    brightYellow: bright(normal.yellow),
    brightBlue: bright(normal.blue),
    brightMagenta: bright(normal.magenta),
    brightCyan: bright(normal.cyan),
    brightWhite: "#ffffff",
  };
}

export class DyncTerminal extends DyncElement {
  static get observedAttributes() {
    return ["session", "mode", "fit-preview"];
  }

  #term = null;
  #fit = null;
  #ws = null;
  #host = null;
  #screen = null;
  #ro = null;
  #fitTimer = null;
  #reconnectTimer = null;
  #backoff = 0;
  #closed = false;
  #serverCols = 80;
  #serverRows = 24;
  #encoder = new TextEncoder();
  #decoder = new TextDecoder();
  #viewQuery = null;
  #replayReady = false;
  #stopped = null; // the overlay shown while the session is not running
  #resuming = false;

  get sessionId() {
    return this.getAttribute("session") || "";
  }

  get mode() {
    return this.getAttribute("mode") === "edit" ? "edit" : "view";
  }

  get #viewOnly() {
    return this.mode === "view" && !this.hasAttribute("fit-preview");
  }

  attributeChangedCallback(name, oldVal, newVal) {
    if (oldVal === newVal || !this.#term) return;
    if (name === "session") {
      this.#resuming = false;
      this.#paintStopped();
      this.#teardownSocket();
      this.#term.reset();
      this.#connect();
      return;
    }

    if (name === "mode") this.#applyMode();
    if (this.#ws && this.#viewQuery !== this.#viewOnly) {
      this.#teardownSocket();
      this.#term.reset();
      this.#connect();
    }
    this.#scheduleFit();
  }

  mount() {
    this.#closed = false;
    this.render(html`<div class="xterm-host" part="host"></div>`);
    this.style.position = "relative";
    this.#host = this.querySelector(".xterm-host");

    this.#stopped = document.createElement("div");
    this.#stopped.className = "term-stopped";
    this.#stopped.hidden = true;
    this.#stopped.innerHTML = `<div class="term-stopped-card">
        <span class="term-stopped-text">${this.hasAttribute("archive-action") ? "Session Stopped" : "Ended"}</span>
        <div class="term-stopped-actions">
          <button type="button" class="primary sm" data-el="stopped-resume">Resume</button>
          <button type="button" class="ghost sm" data-el="stopped-archive" hidden>Archive</button>
        </div>
      </div>`;
    this.appendChild(this.#stopped);
    this.on(this.#stopped, "click", (ev) => {
      // Keep the click from switching the terminal underneath into edit mode.
      ev.stopPropagation();
      if (ev.target.closest("[data-el=stopped-resume]")) this.#resume();
      if (ev.target.closest("[data-el=stopped-archive]")) {
        this.dispatchEvent(new CustomEvent("dync-archive", {
          bubbles: true,
          detail: { id: this.sessionId, archived: true },
        }));
      }
    });
    this.cleanup(store.subscribe(() => this.#paintStopped()));
    this.#paintStopped();

    this.#term = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: true,
      fontFamily: cssVar("--font-mono", "ui-monospace, monospace"),
      fontSize: 14,
      lineHeight: 1.25,
      scrollback: this.hasAttribute("full-scrollback") ? 100_000 : 5000,
      theme: themeFromTokens(),
    });
    this.#fit = new FitAddon();
    this.#term.loadAddon(this.#fit);
    this.#term.open(this.#host);
    this.#screen = this.#host.querySelector(".xterm-screen");

    // xterm uses its own disposable subscriptions, not DOM addEventListener.
    const dataSub = this.#term.onData((data) => {
      if (this.mode !== "edit" || !this.#ws || this.#ws.readyState !== WebSocket.OPEN) return;
      this.#ws.send(this.#encoder.encode(data));
    });
    this.cleanup(() => dataSub.dispose());
    const parsedSub = this.#term.onWriteParsed(() => {
      if (this.mode === "view") this.#positionPreview();
    });
    this.cleanup(() => parsedSub.dispose());

    // Terminal.app ⌘K: clear the local screen and scrollback. Meta chords are not
    // PTY input; Ctrl+K stays with the shell (kill-line).
    this.#term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      if (!ev.metaKey || ev.ctrlKey || ev.altKey || ev.shiftKey) return true;
      if (ev.key !== "k" && ev.key !== "K") return true;
      if (this.mode !== "edit") return true;
      ev.preventDefault();
      this.#term.clear();
      return false;
    });

    this.on(this, "click", () => {
      if (this.mode === "view") {
        this.dispatchEvent(
          new CustomEvent("dync-terminal-focus", {
            bubbles: true,
            composed: true,
            detail: { session: this.sessionId },
          }),
        );
        this.setAttribute("mode", "edit");
      }
      this.#term?.focus();
    });

    // Day/night and accent theme changes both repaint the terminal.
    const retheme = () => {
      if (!this.#term) return;
      const theme = themeFromTokens();
      this.#term.options.theme = theme;
      // CSS around xterm paints with --term-bg; keep it on the tinted colour.
      this.style.setProperty("--term-bg", theme.background);
    };
    this.on(window, "dync-appearance", retheme);
    this.on(window, "dync-theme", retheme);
    this.on(window, "focus", () => {
      if (this.mode === "view" && !this.#viewOnly) this.#scheduleFit();
    });

    this.#ro = new ResizeObserver(() => this.#scheduleFit());
    this.#ro.observe(this);
    this.cleanup(() => this.#ro?.disconnect());

    retheme();
    this.#applyMode();
    this.#scheduleFit();
    this.#connect();
  }

  /** Dim the terminal and offer Resume while its session is ended. */
  #paintStopped() {
    const el = this.#stopped;
    if (!el) return;
    const s = store.sessions.get(this.sessionId);
    const stopped = !!s && !s.live && s.state === "ended";
    this.classList.toggle("is-stopped", stopped);
    el.hidden = !stopped;
    if (!stopped) {
      this.#resuming = false;
      return;
    }
    const archive = el.querySelector("[data-el=stopped-archive]");
    archive.hidden = !this.hasAttribute("archive-action") || !!s.archived;
    archive.disabled = this.#resuming;
    const btn = el.querySelector("[data-el=stopped-resume]");
    btn.hidden = !!s.archived;
    btn.disabled = this.#resuming;
    btn.textContent = this.#resuming ? "Resuming…" : "Resume";
  }

  async #resume() {
    const id = this.sessionId;
    if (!id || this.#resuming) return;
    this.#resuming = true;
    this.#paintStopped();
    try {
      await api.post(`/sessions/${id}/resume`);
    } catch (err) {
      this.#resuming = false;
      this.#paintStopped();
      showToast(err?.message || "Could not resume the session.", { kind: "error" });
    }
  }

  unmount() {
    this.#closed = true;
    this.#teardownSocket();
    clearTimeout(this.#fitTimer);
    clearTimeout(this.#reconnectTimer);
    if (editOwners.get(this.sessionId) === this) editOwners.delete(this.sessionId);
    this.#term?.dispose();
    this.#term = null;
    this.#fit = null;
  }

  #applyMode() {
    const id = this.sessionId;
    if (this.mode === "edit") {
      const prev = editOwners.get(id);
      if (prev && prev !== this && prev.isConnected) {
        prev.setAttribute("mode", "view");
      }
      editOwners.set(id, this);
      this.#clearHostLayout();
      this.#term?.focus();
    } else {
      if (editOwners.get(id) === this) editOwners.delete(id);
      this.#scaleView();
    }
  }

  #scheduleFit() {
    clearTimeout(this.#fitTimer);
    this.#fitTimer = setTimeout(() => this.#doFit(), 80);
  }

  #doFit() {
    if (!this.#term || !this.#fit) return;
    if (this.mode === "edit") {
      if (this.hasAttribute("full-scrollback") && !this.#replayReady) return;
      this.#clearHostLayout();
      this.#fit.fit();
      const cols = this.#term.cols;
      const rows = this.#term.rows;
      if (
        this.#ws &&
        this.#ws.readyState === WebSocket.OPEN &&
        (cols !== this.#serverCols || rows !== this.#serverRows)
      ) {
        this.#ws.send(JSON.stringify({ t: "resize", cols, rows }));
        this.#serverCols = cols;
        this.#serverRows = rows;
      }
    } else {
      this.#scaleView();
      if (!this.#viewOnly && this.#replayReady) this.#fitPreview();
    }
  }

  #fitPreview() {
    const screen = this.#screen;
    const ws = this.#ws;
    if (!document.hasFocus() || ws?.readyState !== WebSocket.OPEN ||
        !screen.offsetWidth || !screen.offsetHeight) return;
    const padding = getComputedStyle(this.#term.element);
    const width = this.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight);
    const height = this.clientHeight - parseFloat(padding.paddingTop) - parseFloat(padding.paddingBottom);
    if (width <= 0 || height <= 0) return;

    const cols = Math.max(2, Math.floor(width * this.#term.cols / screen.offsetWidth));
    const rows = Math.max(1, Math.floor(height * this.#term.rows / screen.offsetHeight));
    if (cols !== this.#serverCols || rows !== this.#serverRows) {
      ws.send(JSON.stringify({ t: "resize", cols, rows }));
    }
  }

  #fitToHost() {
    if (!this.#term || !this.#fit) return;
    this.#clearHostLayout();
    const dims = this.#fit.proposeDimensions();
    if (dims?.cols > 0 && dims?.rows > 0) {
      this.#serverCols = dims.cols;
      this.#serverRows = dims.rows;
    }
  }

  #clearHostLayout() {
    if (!this.#host) return;
    this.#host.style.transform = "";
    this.#host.style.width = "";
    this.#host.style.height = "";
  }

  // Keep the PTY dimensions and the latest rows visible without enlarging text.
  #scaleView() {
    if (!this.#term || !this.#host || !this.clientWidth || !this.clientHeight) return;
    if (this.#term.cols !== this.#serverCols || this.#term.rows !== this.#serverRows) {
      this.#term.resize(this.#serverCols, this.#serverRows);
    }

    const screen = this.#screen;
    if (!screen?.offsetWidth || !screen.offsetHeight) return;
    const padding = getComputedStyle(this.#term.element);
    const width = screen.offsetWidth + parseFloat(padding.paddingLeft) + parseFloat(padding.paddingRight);
    const height = screen.offsetHeight + parseFloat(padding.paddingTop) + parseFloat(padding.paddingBottom);
    this.#host.style.width = `${width}px`;
    this.#host.style.height = `${height}px`;
    this.#positionPreview();
  }

  #positionPreview() {
    const width = this.#host.offsetWidth;
    const height = this.#host.offsetHeight;
    if (!width || !height || !this.clientWidth || !this.clientHeight) return;

    this.#term.scrollToBottom();
    const buffer = this.#term.buffer.active;
    const scale = Math.min(1, this.clientWidth / width);
    // Alternate screens use the entire grid. A shell prompt may sit above empty rows.
    const bottom = buffer.type === "alternate"
      ? height
      : height - (this.#term.rows - 1 - buffer.cursorY) * this.#screen.offsetHeight / this.#term.rows;
    const offset = Math.min(0, this.clientHeight - bottom * scale);
    this.#host.style.transform = `translateY(${offset}px) scale(${scale})`;
  }

  #teardownSocket() {
    clearTimeout(this.#reconnectTimer);
    if (this.#ws) {
      this.#ws.onopen = null;
      this.#ws.onmessage = null;
      this.#ws.onclose = null;
      this.#ws.onerror = null;
      try {
        this.#ws.close();
      } catch (_) {
        /* ignore */
      }
      this.#ws = null;
    }
  }

  #connect() {
    if (this.#closed || !this.sessionId) return;
    this.#teardownSocket();
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const wantView = this.#viewOnly;
    this.#replayReady = false;
    this.#decoder = new TextDecoder();
    this.#viewQuery = wantView;
    const q = wantView ? "?mode=view" : "";
    const url = `${proto}//${location.host}/api/v1/sessions/${encodeURIComponent(this.sessionId)}/terminal${q}`;
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    this.#ws = ws;

    ws.onopen = () => {
      this.#backoff = 0;
    };

    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        let msg;
        try {
          msg = JSON.parse(ev.data);
        } catch {
          return;
        }
        if (msg?.t === "size") {
          if (msg.cols && msg.rows) {
            this.#serverCols = msg.cols;
            this.#serverRows = msg.rows;
          } else {
            // An ended session from before sizes were recorded: replay at this element's grid.
            this.#fitToHost();
          }
          if (this.mode === "view") {
            this.#scaleView();
          } else if (this.hasAttribute("full-scrollback") && !this.#replayReady) {
            this.#term.resize(this.#serverCols, this.#serverRows);
          } else {
            this.#doFit();
          }
        } else if (msg?.t === "replay_done" && !this.#viewOnly) {
          // Apply the original grid until xterm has finished parsing the replay.
          this.#term.write("", () => {
            if (this.#ws !== ws) return;
            this.#replayReady = true;
            this.#scheduleFit();
          });
        }
        return;
      }
      const bytes = new Uint8Array(ev.data);
      this.#term?.write(this.#decoder.decode(bytes, { stream: true }));
    };

    ws.onclose = () => {
      this.#ws = null;
      if (this.#closed) return;
      const delay = BACKOFF[Math.min(this.#backoff, BACKOFF.length - 1)];
      this.#backoff += 1;
      this.#reconnectTimer = setTimeout(() => {
        this.#term?.reset();
        this.#connect();
      }, delay);
    };
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-terminal")) {
  customElements.define("dync-terminal", DyncTerminal);
}

import { DyncElement, html, raw } from "../lib/element.js";
import { brandMark, wordmark } from "../lib/brand.js";
import { store } from "../lib/store.js";
import * as route from "../lib/route.js";
import "./dync-topbar.js";
import "./dync-board.js";
import "./dync-workspace.js";
import "./dync-pulls.js";
import "./dync-pull.js";

export class DyncApp extends DyncElement {
  #signedIn = false;
  #boardFocus = null;

  mount() {
    this.#signedIn = this.hasAttribute("signed-in");
    if (!this.#signedIn) {
      this.renderGate();
      return;
    }
    this.renderApp();
  }

  renderGate() {
    this.render(
      html`<div class="gate">
        <div class="gate-card card">
          ${raw(brandMark(48))}
          <h1>${raw(wordmark())}</h1>
          <p>Open SyncAiDync from the link it printed in your terminal.</p>
          <p class="cmd"><code>syncaidync</code></p>
        </div>
      </div>`,
    );
  }

  renderApp() {
    this.render(
      html`<dync-topbar></dync-topbar>
        <main class="shell-main" data-main></main>`,
    );
    const topbar = this.querySelector("dync-topbar");
    const unsub = store.subscribe(() => {
      topbar?.setConnected(store.connected);
    });
    this.cleanup(unsub);
    topbar?.setConnected(store.connected);

    this.on(topbar, "dync-navigate", async (ev) => {
      const { destination, focusId } = ev.detail;
      const ws = this.querySelector("dync-workspace");
      if (ws && !(await ws.confirmLeave())) return;
      this.#boardFocus = focusId ?? null;
      route.go(destination);
    });
    this.on(topbar, "dync-workspace-tool", (ev) => {
      this.querySelector("dync-workspace")?.focusTool(ev.detail);
    });

    this.on(window, route.ROUTE_EVENT, () => this.#paintRoute());
    this.#paintRoute();
  }

  #paintRoute() {
    const main = this.querySelector("[data-main]");
    if (!main) return;
    const r = route.current();
    this.querySelector("dync-topbar").setRoute(r);
    if (r.name === "board") {
      if (!main.querySelector("dync-board")) {
        main.innerHTML = "";
        main.appendChild(document.createElement("dync-board"));
      }
      if (this.#boardFocus) {
        const id = this.#boardFocus;
        this.#boardFocus = null;
        window.dispatchEvent(new CustomEvent("dync-focus-session", { detail: { id } }));
      }
      return;
    }
    if (r.name === "session" && r.id) {
      const existing = main.querySelector("dync-workspace");
      const wantSolo = !!r.solo && !!r.tool;
      const same =
        existing &&
        existing.getAttribute("data-session") === r.id &&
        existing.hasAttribute("data-solo") === wantSolo &&
        existing.getAttribute("data-tool") === (r.tool || null);
      if (same) return;
      main.innerHTML = "";
      const ws = document.createElement("dync-workspace");
      ws.setAttribute("data-session", r.id);
      if (r.tool) ws.setAttribute("data-tool", r.tool);
      if (wantSolo) ws.setAttribute("data-solo", "");
      main.appendChild(ws);
      return;
    }
    if (r.name === "pulls") {
      if (!main.querySelector("dync-pulls")) {
        main.innerHTML = "";
        main.appendChild(document.createElement("dync-pulls"));
      }
      return;
    }
    if (r.name === "pull" && r.id) {
      const existing = main.querySelector("dync-pull");
      if (existing && existing.getAttribute("pull-id") === r.id) return;
      main.innerHTML = "";
      const el = document.createElement("dync-pull");
      el.setAttribute("pull-id", r.id);
      main.appendChild(el);
      return;
    }
    main.innerHTML = "";
    const placeholder = document.createElement("div");
    placeholder.className = "empty";
    placeholder.textContent = "Nothing here yet.";
    main.appendChild(placeholder);
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-app")) {
  customElements.define("dync-app", DyncApp);
}

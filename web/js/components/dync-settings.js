import { DyncElement, html } from "../lib/element.js";
import { openDialog } from "../lib/dialog.js";
import * as prefs from "../lib/prefs.js";
import * as themes from "../lib/themes.js";
import * as appearance from "../lib/appearance.js";
import { playTest } from "../lib/sound.js";
import { store } from "../lib/store.js";
import { DEFAULT_ALERTS } from "../lib/alerts.js";

export class DyncSettings extends DyncElement {
  #dialog = null;

  mount() {
    this.render(
      html`<dialog class="settings-dialog">
        <form method="dialog" class="dialog-body" data-form>
          <h2>Settings</h2>

          <fieldset>
            <legend>Board</legend>
            <label class="row">
              <input type="checkbox" name="show_archived_sessions" />
              <span>Show archived sessions</span>
            </label>
          </fieldset>

          <fieldset>
            <legend>Alerts</legend>
            <label class="row">
              <input type="checkbox" name="sound" />
              <span>Sounds</span>
            </label>
            <label>
              <span>Volume</span>
              <div class="row gap">
                <input type="range" name="volume" min="0" max="1" step="0.05" />
                <button type="button" class="ghost sm" data-action="test-sound">Test</button>
              </div>
            </label>
            <label class="row">
              <input type="checkbox" name="notify" />
              <span>Desktop notifications</span>
            </label>
            <p class="muted sm" data-notify-hint hidden></p>
            <label>
              <span>Reminder</span>
              <select name="remind_minutes">
                <option value="off">Off</option>
                <option value="2">Every 2 minutes</option>
                <option value="5">Every 5 minutes</option>
                <option value="10">Every 10 minutes</option>
                <option value="15">Every 15 minutes</option>
              </select>
            </label>
            <label class="row">
              <input type="checkbox" name="on_ready" />
              <span>Alert when ready</span>
            </label>
          </fieldset>

          <fieldset>
            <legend>Appearance</legend>
            <div class="segmented" role="radiogroup" aria-label="Appearance">
              <label><input type="radio" name="appearance" value="auto" /><span>Auto</span></label>
              <label><input type="radio" name="appearance" value="day" /><span>Day</span></label>
              <label><input type="radio" name="appearance" value="night" /><span>Night</span></label>
            </div>
          </fieldset>

          <fieldset>
            <legend>Accent theme</legend>
            <label class="row">
              <input type="checkbox" name="theme_cycle" />
              <span>Cycle through themes</span>
            </label>
            <label>
              <span>Search</span>
              <input type="search" name="theme-search" placeholder="Filter themes" autocomplete="off" />
            </label>
            <div class="theme-grid" data-themes role="listbox" aria-label="Accent themes"></div>
          </fieldset>

          <section data-update hidden>
            <h3 data-update-title></h3>
            <code data-update-command></code>
            <button class="ghost sm" type="button" data-action="copy-update">Copy command</button>
            <span role="status" data-update-status></span>
          </section>
          <p class="muted sm" data-about></p>

          <div class="dialog-actions">
            <button type="submit" value="cancel" class="ghost">Close</button>
          </div>
        </form>
      </dialog>`,
    );

    this.#dialog = this.querySelector("dialog");
    const form = this.querySelector("[data-form]");

    this.on(form, "change", (ev) => {
      if (ev.target.name === "theme-search") {
        this.#paintThemes(ev.target.value);
        return;
      }
      if (ev.target.name === "appearance") {
        appearance.set(ev.target.value);
        // The accent shades differ between day and night; repaint the chips.
        this.#paintThemes(form.elements["theme-search"]?.value || "");
        return;
      }
      if (ev.target.name === "theme_cycle") {
        const on = ev.target.checked;
        themes.setCycleEnabled(on);
        if (on) themes.startCycle();
        else themes.pauseCycle(24 * 60 * 60_000);
        return;
      }
      if (ev.target.name === "show_archived_sessions") {
        prefs.set("show-archived-sessions", ev.target.checked);
        window.dispatchEvent(new Event("dync-show-archived-sessions"));
        return;
      }
      this.#saveFromForm();
    });

    this.on(form, "input", (ev) => {
      if (ev.target.name === "volume") this.#saveFromForm();
      if (ev.target.name === "theme-search") this.#paintThemes(ev.target.value);
    });

    this.on(window, "dync-update", () => this.#paintUpdate());
    this.on(form, "click", async (ev) => {
      if (ev.target.closest('[data-action="copy-update"]')) {
        const status = this.querySelector("[data-update-status]");
        try {
          await navigator.clipboard.writeText(store.update.command);
          status.textContent = "Copied.";
        } catch {
          status.textContent = "Could not copy the command.";
        }
        return;
      }
      if (ev.target.closest('[data-action="test-sound"]')) {
        ev.preventDefault();
        this.#saveFromForm();
        playTest();
        return;
      }
      const swatch = ev.target.closest("[data-theme]");
      if (swatch) {
        themes.pauseCycle();
        themes.setCycleEnabled(false);
        form.elements.theme_cycle.checked = false;
        themes.applyTheme(swatch.getAttribute("data-theme"));
        this.#paintThemes(form.elements["theme-search"]?.value || "");
      }
    });
  }

  open() {
    this.#hydrate();
    this.#paintUpdate();
    openDialog(this.#dialog);
  }

  #paintUpdate() {
    const update = store.update;
    this.querySelector("[data-update]").hidden = !update?.newer;
    if (!update?.newer) return;
    this.querySelector("[data-update-title]").textContent = `Version ${update.latest} is available`;
    this.querySelector("[data-update-command]").textContent = update.command;
  }

  #hydrate() {
    const form = this.querySelector("[data-form]");
    form.elements.show_archived_sessions.checked = prefs.get("show-archived-sessions", false);
    const a = prefs.get("alerts", DEFAULT_ALERTS) || DEFAULT_ALERTS;
    form.elements.sound.checked = !!a.sound;
    form.elements.volume.value = String(a.volume ?? 0.7);
    form.elements.notify.checked = !!a.notify;
    form.elements.on_ready.checked = a.on_ready !== false;
    const remind = a.remind_minutes === false ? "off" : String(a.remind_minutes ?? 5);
    form.elements.remind_minutes.value = remind;

    const mode = appearance.current();
    for (const r of form.elements.appearance) r.checked = r.value === mode;
    form.elements.theme_cycle.checked = themes.cycleEnabled();

    this.#paintNotifyHint();
    this.#paintThemes("");
    form.elements["theme-search"].value = "";

    const about = this.querySelector("[data-about]");
    const meta = store.meta;
    if (about) {
      about.textContent = meta
        ? `SyncAiDync ${meta.version} · home ${meta.home}`
        : "SyncAiDync";
    }
  }

  #paintNotifyHint() {
    const hint = this.querySelector("[data-notify-hint]");
    const form = this.querySelector("[data-form]");
    if (!hint || !form) return;
    if (!form.elements.notify.checked) {
      hint.hidden = true;
      return;
    }
    if (typeof Notification === "undefined") {
      hint.hidden = false;
      hint.textContent = "Notifications are not available in this browser.";
      return;
    }
    if (Notification.permission === "denied") {
      hint.hidden = false;
      hint.textContent =
        "Blocked by the browser. Allow notifications for this site in browser settings.";
      return;
    }
    hint.hidden = true;
  }

  async #saveFromForm() {
    const form = this.querySelector("[data-form]");
    const remindRaw = form.elements.remind_minutes.value;
    const remind_minutes = remindRaw === "off" ? false : Number(remindRaw);
    let notify = !!form.elements.notify.checked;

    if (notify && typeof Notification !== "undefined" && Notification.permission === "default") {
      try {
        const perm = await Notification.requestPermission();
        if (perm !== "granted") notify = false;
      } catch {
        notify = false;
      }
      form.elements.notify.checked = notify;
    }
    if (notify && typeof Notification !== "undefined" && Notification.permission === "denied") {
      notify = false;
      form.elements.notify.checked = false;
    }

    prefs.set("alerts", {
      sound: !!form.elements.sound.checked,
      volume: Number(form.elements.volume.value) || 0,
      notify,
      remind_minutes,
      on_ready: !!form.elements.on_ready.checked,
    });
    this.#paintNotifyHint();
    window.dispatchEvent(new CustomEvent("dync-alerts-prefs"));
  }

  #paintThemes(filter) {
    const host = this.querySelector("[data-themes]");
    if (!host) return;
    const q = String(filter || "").trim().toLowerCase();
    const current = themes.currentName();
    host.innerHTML = "";
    for (const t of themes.usableThemes()) {
      if (q && !t.name.toLowerCase().includes(q)) continue;
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "theme-chip";
      btn.dataset.theme = t.name;
      btn.setAttribute("role", "option");
      btn.setAttribute("aria-selected", t.name === current ? "true" : "false");
      if (t.name === current) btn.classList.add("active");
      btn.title = t.name;
      btn.innerHTML = `<span class="theme-chip-swatch" style="background:linear-gradient(135deg,${t.accent},${t.accent2})"></span><span class="theme-chip-name"></span>`;
      btn.querySelector(".theme-chip-name").textContent = t.name;
      host.appendChild(btn);
    }
  }
}

let singleton = null;

export function openSettings() {
  if (!singleton) {
    singleton = document.createElement("dync-settings");
    document.body.appendChild(singleton);
  }
  singleton.open();
}

if (typeof customElements !== "undefined" && !customElements.get("dync-settings")) {
  customElements.define("dync-settings", DyncSettings);
}

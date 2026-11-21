import { DyncElement, html } from "../lib/element.js";
import { api, ApiError } from "../lib/api.js";
import { openDialog, setBusy } from "../lib/dialog.js";

export class DyncRemoveSession extends DyncElement {
  #dialog = null;
  #id = null;

  mount() {
    this.render(
      html`<dialog class="remove-session-dialog">
        <form method="dialog" class="dialog-body" data-form>
          <h2>Remove session</h2>
          <p data-copy>Remove this session from the board?</p>
          <label class="check" data-delete-row hidden>
            <input type="checkbox" name="delete_folder" />
            <span data-delete-label>Also delete the folder</span>
          </label>
          <p class="error" data-error hidden></p>
          <div class="dialog-actions">
            <button type="submit" value="cancel" class="ghost">Cancel</button>
            <button type="submit" value="remove" class="danger">Remove</button>
          </div>
        </form>
      </dialog>`,
    );
    this.#dialog = this.querySelector("dialog");
    const form = this.querySelector("[data-form]");
    this.on(form, "submit", async (ev) => {
      if (ev.submitter?.value === "cancel") return;
      ev.preventDefault();
      await this.#remove(form);
    });
  }

  async open(sessionId) {
    this.#id = sessionId;
    this.#setError("");
    const row = this.querySelector("[data-delete-row]");
    const label = this.querySelector("[data-delete-label]");
    const copy = this.querySelector("[data-copy]");
    const form = this.querySelector("[data-form]");
    form.delete_folder.checked = false;
    row.hidden = true;
    copy.textContent = "Remove this session from the board?";
    openDialog(this.#dialog);

    try {
      const info = await api.get(`/sessions/${encodeURIComponent(sessionId)}/removal`);
      if (info.owned) {
        row.hidden = false;
        let note = "Also delete the folder";
        if (info.dirty) {
          note = "Also delete the folder (it has uncommitted changes)";
          form.delete_folder.checked = false;
        } else if (info.unpushed > 0) {
          note = `Also delete the folder (it has ${info.unpushed} commit${info.unpushed === 1 ? "" : "s"} that are not pushed)`;
          form.delete_folder.checked = false;
        } else {
          form.delete_folder.checked = true;
        }
        label.textContent = note;
        copy.textContent = `Remove this session? Folder: ${info.path}`;
      }
    } catch {
      /* keep simple copy */
    }
  }

  #setError(msg) {
    const el = this.querySelector("[data-error]");
    if (!el) return;
    el.hidden = !msg;
    el.textContent = msg || "";
  }

  async #remove(form) {
    const id = this.#id;
    if (!id) return;
    const removeFolder = !!form.delete_folder?.checked;
    const q = removeFolder ? "?workspace=remove" : "?workspace=keep";
    setBusy(form, true);
    try {
      await api.del(`/sessions/${encodeURIComponent(id)}${q}`);
      this.#dialog.close();
    } catch (err) {
      this.#setError(err instanceof ApiError ? err.message : "Could not remove the session.");
    } finally {
      setBusy(form, false);
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-remove-session")) {
  customElements.define("dync-remove-session", DyncRemoveSession);
}

export function openRemoveSession(sessionId) {
  let el = document.querySelector("dync-remove-session");
  if (!el) {
    el = document.createElement("dync-remove-session");
    document.body.appendChild(el);
  }
  el.open(sessionId);
}

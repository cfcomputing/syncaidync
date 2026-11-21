import { DyncElement, html } from "../lib/element.js";
import { api } from "../lib/api.js";
import { openDialog } from "../lib/dialog.js";
import { agentLabel } from "../lib/labels.js";
import { store } from "../lib/store.js";
import { relativeTime } from "../lib/time.js";

class DyncImport extends DyncElement {
  #items = [];

  mount() {
    this.render(html`<dialog class="import-dialog">
      <div class="dialog-body">
        <h2>Import history</h2>
        <label>Search past sessions <input type="search" autocomplete="off" placeholder="Prompt, folder, or agent" /></label>
        <p role="status"></p>
        <div class="history-results"></div>
        <div class="dialog-actions"><form method="dialog"><button class="ghost">Close</button></form></div>
      </div>
    </dialog>`);
    const dialog = this.querySelector("dialog");
    this.on(dialog, "close", () => this.remove());
    this.on(dialog, "input", () => this.#paint());
    openDialog(dialog);
    this.#load();
  }

  async #load() {
    const status = this.querySelector("[role=status]");
    status.textContent = "Loading past sessions…";
    try {
      this.#items = (await api.get("/history")).items;
      this.#paint();
    } catch (err) {
      status.textContent = err.message;
    }
  }

  #paint() {
    const query = this.querySelector("input").value.trim().toLowerCase();
    const list = this.querySelector(".history-results");
    const status = this.querySelector("[role=status]");
    const items = this.#items.filter((item) =>
      `${item.cwd} ${item.first_prompt} ${agentLabel(store.meta, item.agent)}`.toLowerCase().includes(query),
    );
    list.replaceChildren();
    status.textContent = items.length ? "" : query
      ? "No past sessions match your search."
      : "No past sessions were found on this machine.";
    for (const item of items) {
      const row = document.createElement("div");
      row.className = "history-row";
      const facts = document.createElement("div");
      facts.className = "history-row-facts";
      const prompt = document.createElement("p");
      prompt.className = "history-row-prompt";
      prompt.textContent = item.first_prompt || "Untitled session";
      prompt.title = item.first_prompt || "";
      const detail = document.createElement("p");
      detail.className = "history-row-meta";
      const meta = [item.cwd, agentLabel(store.meta, item.agent), relativeTime(item.last_active)]
        .filter(Boolean)
        .join(" · ");
      detail.textContent = meta;
      detail.title = meta;
      facts.append(prompt, detail);
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = "Import";
      button.className = "sm";
      button.addEventListener("click", async () => {
        for (const control of list.querySelectorAll("button")) control.disabled = true;
        const search = this.querySelector("input");
        search.disabled = true;
        try {
          await api.post("/history/import", { agent: item.agent, agent_session: item.agent_session });
          this.querySelector("dialog").close();
        } catch (err) {
          status.textContent = err.message;
          for (const control of list.querySelectorAll("button")) control.disabled = false;
          search.disabled = false;
        }
      });
      row.append(facts, button);
      list.append(row);
    }
  }
}

customElements.define("dync-import", DyncImport);

export function openImport() {
  if (!document.querySelector("dync-import")) {
    document.body.append(document.createElement("dync-import"));
  }
}

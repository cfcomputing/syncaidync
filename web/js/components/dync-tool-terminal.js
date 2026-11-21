import { DyncElement, html } from "../lib/element.js";
import "./dync-terminal.js";

export class DyncToolTerminal extends DyncElement {
  static get observedAttributes() {
    return ["session"];
  }

  mount() {
    this.classList.add("tool", "tool-terminal");
    this.render(html`<dync-terminal mode="edit" full-scrollback></dync-terminal>`);
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  #sync() {
    const term = this.querySelector("dync-terminal");
    const id = this.getAttribute("session");
    if (term && id) {
      term.setAttribute("session", id);
      term.setAttribute("mode", "edit");
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-tool-terminal")) {
  customElements.define("dync-tool-terminal", DyncToolTerminal);
}

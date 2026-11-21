import { DyncElement, html } from "../lib/element.js";
import "./dync-changes-view.js";

export class DyncToolChanges extends DyncElement {
  static get observedAttributes() {
    return ["session"];
  }

  mount() {
    this.classList.add("tool", "tool-changes");
    this.render(html`<dync-changes-view></dync-changes-view>`);
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  #sync() {
    const view = this.querySelector("dync-changes-view");
    const id = this.getAttribute("session");
    if (view && id) {
      view.source = { kind: "session", id };
    }
  }
}

if (typeof customElements !== "undefined" && !customElements.get("dync-tool-changes")) {
  customElements.define("dync-tool-changes", DyncToolChanges);
}

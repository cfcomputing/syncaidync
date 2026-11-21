/** Base custom-element helpers and a safe html tagged template. */

const ESC = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

const RAW = Symbol("raw");

export function escapeText(value) {
  if (value == null) return "";
  return String(value).replace(/[&<>"']/g, (ch) => ESC[ch]);
}

export function raw(value) {
  return { [RAW]: true, value: value == null ? "" : String(value) };
}

function isRaw(value) {
  return value && typeof value === "object" && value[RAW] === true;
}

function stringify(value) {
  if (value == null || value === false) return "";
  if (isRaw(value)) return value.value;
  if (Array.isArray(value)) return value.map(stringify).join("");
  return escapeText(value);
}

export function html(strings, ...values) {
  let out = strings[0] ?? "";
  for (let i = 0; i < values.length; i++) {
    out += stringify(values[i]);
    out += strings[i + 1] ?? "";
  }
  return raw(out);
}

const Base =
  typeof HTMLElement !== "undefined"
    ? HTMLElement
    : class {
        constructor() {
          this.innerHTML = "";
        }
      };

export class DyncElement extends Base {
  #cleanups = [];

  connectedCallback() {
    this.mount();
  }

  disconnectedCallback() {
    for (const fn of this.#cleanups.splice(0)) {
      try {
        fn();
      } catch (_) {
        /* ignore cleanup errors */
      }
    }
    this.unmount();
  }

  mount() {}
  unmount() {}

  cleanup(fn) {
    if (typeof fn === "function") this.#cleanups.push(fn);
  }

  on(target, type, handler, options) {
    target.addEventListener(type, handler, options);
    this.cleanup(() => target.removeEventListener(type, handler, options));
  }

  every(ms, fn) {
    const id = setInterval(fn, ms);
    this.cleanup(() => clearInterval(id));
    return id;
  }

  render(markup) {
    this.innerHTML = isRaw(markup) ? markup.value : String(markup ?? "");
  }
}

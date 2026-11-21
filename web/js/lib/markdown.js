/** Only place server HTML is inserted. WO-04: pass through already-sanitized html. */

import { raw } from "./element.js";

/** Accept the server's sanitized `html` field and mark it safe for html``. */
export function fromServerHtml(html) {
  return raw(html == null ? "" : String(html));
}

/** Escape plain text into a raw fragment (no markdown yet). */
export function fromPlainText(text) {
  const esc = String(text ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
  return raw(esc);
}

/** In-app confirm and text prompts (modal dialog or popover at the anchor). */

import { openDialog } from "./dialog.js";
import { closeMenu, openPopoverBelow } from "./menu.js";

/**
 * @param {{
 *   title: string,
 *   message?: string,
 *   confirmLabel?: string,
 *   cancelLabel?: string,
 *   danger?: boolean,
 * }} opts
 * @returns {Promise<boolean>}
 */
export function askConfirm({
  title,
  message = "",
  confirmLabel = "OK",
  cancelLabel = "Cancel",
  danger = false,
} = {}) {
  return new Promise((resolve) => {
    const dlg = document.createElement("dialog");
    dlg.className = "ask-dialog";
    const msg = message
      ? `<p class="ask-dialog-msg">${escapeHtml(message)}</p>`
      : "";
    dlg.innerHTML = `<form method="dialog" class="dialog-body">
      <h2>${escapeHtml(title || "Confirm")}</h2>
      ${msg}
      <div class="dialog-actions">
        <button type="submit" value="cancel" class="ghost">${escapeHtml(cancelLabel)}</button>
        <button type="submit" value="ok" class="${danger ? "danger" : ""}">${escapeHtml(confirmLabel)}</button>
      </div>
    </form>`;
    document.body.appendChild(dlg);
    dlg.addEventListener(
      "close",
      () => {
        const ok = dlg.returnValue === "ok";
        dlg.remove();
        resolve(ok);
      },
      { once: true },
    );
    openDialog(dlg, {
      focus: dlg.querySelector(danger ? "button[value='cancel']" : "button[value='ok']"),
    });
  });
}

/**
 * Small form anchored under a control (notes, titles).
 * @param {HTMLElement} anchor
 * @param {{
 *   title?: string,
 *   label?: string,
 *   value?: string,
 *   placeholder?: string,
 *   multiline?: boolean,
 *   rows?: number,
 *   confirmLabel?: string,
 *   cancelLabel?: string,
 *   required?: boolean,
 *   requiredMessage?: string,
 * }} opts
 * @returns {Promise<string|null>} trimmed string, or null if cancelled
 */
export function askTextNear(
  anchor,
  {
    title = "",
    label = "",
    value = "",
    placeholder = "",
    multiline = false,
    rows = 3,
    confirmLabel = "Save",
    cancelLabel = "Cancel",
    required = true,
    requiredMessage = "Enter something first.",
  } = {},
) {
  return new Promise((resolve) => {
    if (!(anchor instanceof HTMLElement)) {
      resolve(null);
      return;
    }
    const form = document.createElement("form");
    form.className = "ask-pop";
    form.setAttribute("novalidate", "");
    const field = multiline
      ? `<textarea name="text" rows="${Math.max(2, rows | 0)}" placeholder="${escapeAttr(placeholder)}">${escapeHtml(value)}</textarea>`
      : `<input type="text" name="text" value="${escapeAttr(value)}" placeholder="${escapeAttr(placeholder)}" autocomplete="off" />`;
    form.innerHTML = `${title ? `<div class="ask-pop-title">${escapeHtml(title)}</div>` : ""}
      ${label ? `<label class="ask-pop-label">${escapeHtml(label)}${field}</label>` : `<div class="ask-pop-field">${field}</div>`}
      <p class="ask-pop-err" data-err hidden></p>
      <div class="ask-pop-actions">
        <button type="button" class="ghost sm" data-cancel>${escapeHtml(cancelLabel)}</button>
        <button type="submit" class="sm">${escapeHtml(confirmLabel)}</button>
      </div>`;

    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      closeMenu();
      resolve(result);
    };

    form.addEventListener("submit", (ev) => {
      ev.preventDefault();
      const raw = form.elements.namedItem("text")?.value ?? "";
      if (required && !String(raw).trim()) {
        const err = form.querySelector("[data-err]");
        if (err) {
          err.hidden = false;
          err.textContent = requiredMessage;
        }
        form.querySelector("[name=text]")?.focus();
        return;
      }
      finish(String(raw));
    });
    form.querySelector("[data-cancel]")?.addEventListener("click", () => finish(null));
    form.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        finish(null);
      }
    });

    openPopoverBelow(anchor, form, () => finish(null));
  });
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function escapeAttr(s) {
  return escapeHtml(s).replace(/'/g, "&#39;");
}

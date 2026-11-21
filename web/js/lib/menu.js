/** One shared popover menu at a time. */

let openMenu = null;

function closeCurrent() {
  if (!openMenu) return;
  const closing = openMenu;
  openMenu.remove();
  openMenu = null;
  closing._onClose?.();
  document.removeEventListener("pointerdown", onDocPointer, true);
  document.removeEventListener("keydown", onDocKey, true);
}

function onDocPointer(ev) {
  if (!openMenu) return;
  if (openMenu.contains(ev.target) || openMenu._anchor?.contains?.(ev.target)) return;
  closeCurrent();
}

function onDocKey(ev) {
  if (!openMenu) return;
  if (ev.key === "Escape") {
    ev.preventDefault();
    const anchor = openMenu._anchor;
    closeCurrent();
    anchor?.focus();
    return;
  }
  const items = [...openMenu.querySelectorAll("[role='menuitem']:not([disabled])")];
  if (!items.length) return;
  const i = items.indexOf(document.activeElement);
  if (ev.key === "ArrowDown") {
    ev.preventDefault();
    items[(i + 1) % items.length].focus();
  } else if (ev.key === "ArrowUp") {
    ev.preventDefault();
    items[(i - 1 + items.length) % items.length].focus();
  } else if (ev.key === "Enter" || ev.key === " ") {
    if (document.activeElement?.getAttribute("role") === "menuitem") {
      ev.preventDefault();
      document.activeElement.click();
    }
  }
}

/**
 * @param {HTMLElement} anchor
 * @param {{ id: string, label: string, danger?: boolean }[]} items
 * @param {(id: string) => void} onPick
 * @param {Promise<{ id: string, label: string, danger?: boolean }[]>} [leadingItems]
 */
export function openMenuBelow(anchor, items, onPick, leadingItems) {
  closeCurrent();
  const menu = document.createElement("div");
  menu.className = "menu-popover";
  menu.setAttribute("role", "menu");
  const makeItem = (item) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = item.danger ? "menu-item danger" : "menu-item";
    btn.setAttribute("role", "menuitem");
    btn.textContent = item.label;
    btn.addEventListener("click", () => {
      closeCurrent();
      onPick(item.id);
    });
    return btn;
  };
  menu.append(...items.map(makeItem));
  document.body.appendChild(menu);
  const position = () => {
    const rect = anchor.getBoundingClientRect();
    const mw = menu.offsetWidth;
    const mh = menu.offsetHeight;
    const left = Math.max(8, rect.right - mw);
    const below = rect.bottom + 6;
    const top = below + mh > window.innerHeight - 8 ? Math.max(8, rect.top - mh - 6) : below;
    menu.style.left = `${Math.round(left)}px`;
    menu.style.top = `${Math.round(top)}px`;
  };
  position();
  menu._anchor = anchor;
  openMenu = menu;
  document.addEventListener("pointerdown", onDocPointer, true);
  document.addEventListener("keydown", onDocKey, true);
  menu.querySelector("[role='menuitem']")?.focus();
  leadingItems?.then((extra) => {
    if (openMenu !== menu || !anchor.isConnected || !extra.length) return;
    menu.prepend(...extra.map(makeItem));
    position();
  }).catch(() => {});
  return menu;
}

/**
 * Show custom content (for example sliders) in the shared popover below `anchor`. It closes
 * like a menu: Escape, or a pointer down outside it. `onClose` runs when it closes.
 * @param {HTMLElement} anchor
 * @param {HTMLElement} content
 * @param {() => void} [onClose]
 */
export function openPopoverBelow(anchor, content, onClose) {
  closeCurrent();
  const pop = document.createElement("div");
  pop.className = "menu-popover menu-popover-custom";
  pop.setAttribute("role", "dialog");
  pop.appendChild(content);
  document.body.appendChild(pop);
  const rect = anchor.getBoundingClientRect();
  const left = Math.max(8, rect.right - pop.offsetWidth);
  const below = rect.bottom + 6;
  const top =
    below + pop.offsetHeight > window.innerHeight - 8
      ? Math.max(8, rect.top - pop.offsetHeight - 6)
      : below;
  pop.style.left = `${Math.round(left)}px`;
  pop.style.top = `${Math.round(top)}px`;
  pop._anchor = anchor;
  pop._onClose = onClose;
  openMenu = pop;
  document.addEventListener("pointerdown", onDocPointer, true);
  document.addEventListener("keydown", onDocKey, true);
  pop.querySelector("input, textarea, button, select")?.focus();
  return pop;
}

export function closeMenu() {
  closeCurrent();
}

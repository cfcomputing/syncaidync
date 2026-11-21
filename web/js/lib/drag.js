/** Pointer move / resize / chip-drag helpers. Session-agnostic. */

function clamp(n, lo, hi) {
  return Math.min(hi, Math.max(lo, n));
}

function speed(dist) {
  return clamp(Math.round(dist / 2), 1, 24);
}

/**
 * @param {{
 *   pointerId: number,
 *   startX: number, startY: number,
 *   rect: { x: number, y: number, w: number, h: number },
 *   scrollEl?: HTMLElement | null,
 *   onFrame: (rect: object) => void,
 *   onCommit: (rect: object) => void,
 *   onCancel: () => void,
 *   clampRect?: (rect: object) => object,
 * }} options
 */
export function startMove(options) {
  const start = { ...options.rect };
  const originX = options.startX;
  const originY = options.startY;
  const sc = options.scrollEl || null;
  const startScrollX = sc ? sc.scrollLeft : 0;
  const startScrollY = sc ? sc.scrollTop : 0;
  const applyClamp = options.clampRect || ((r) => r);
  let current = applyClamp({ ...start });
  let px = originX;
  let py = originY;
  let raf = 0;
  let cancelled = false;
  let finished = false;

  document.body.classList.add("is-dragging");

  const compute = (clientX, clientY) => {
    const sx = sc ? sc.scrollLeft - startScrollX : 0;
    const sy = sc ? sc.scrollTop - startScrollY : 0;
    return applyClamp({
      ...start,
      x: start.x + (clientX - originX) + sx,
      y: start.y + (clientY - originY) + sy,
    });
  };

  const autoScroll = () => {
    if (!sc) return;
    const bounds = sc.getBoundingClientRect();
    const edge = 48;
    let dx = 0;
    let dy = 0;
    if (px < bounds.left + edge) dx = -speed(bounds.left + edge - px);
    else if (px > bounds.right - edge) dx = speed(px - (bounds.right - edge));
    if (py < bounds.top + edge) dy = -speed(bounds.top + edge - py);
    else if (py > bounds.bottom - edge) dy = speed(py - (bounds.bottom - edge));
    if (dx || dy) {
      sc.scrollLeft += dx;
      sc.scrollTop += dy;
    }
  };

  const tick = () => {
    raf = 0;
    if (cancelled || finished) return;
    autoScroll();
    current = compute(px, py);
    options.onFrame(current);
  };

  const onMove = (ev) => {
    if (cancelled || finished || ev.pointerId !== options.pointerId) return;
    px = ev.clientX;
    py = ev.clientY;
    if (!raf) raf = requestAnimationFrame(tick);
  };

  const finish = (commit) => {
    if (finished) return;
    finished = true;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    window.removeEventListener("keydown", onKey, true);
    document.body.classList.remove("is-dragging");
    if (raf) cancelAnimationFrame(raf);
    if (cancelled || !commit) options.onCancel();
    else options.onCommit(applyClamp(current));
  };

  const onUp = (ev) => {
    if (ev.pointerId !== options.pointerId) return;
    finish(true);
  };

  const onKey = (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      ev.stopPropagation();
      cancelled = true;
      current = applyClamp({ ...start });
      options.onFrame(current);
      finish(false);
    }
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
  window.addEventListener("keydown", onKey, true);
  options.onFrame(current);

  return {
    cancel: () => {
      cancelled = true;
      finish(false);
    },
  };
}

/**
 * @param {{
 *   pointerId: number,
 *   edge: 'n'|'s'|'e'|'w'|'ne'|'nw'|'se'|'sw',
 *   startX: number, startY: number,
 *   rect: { x: number, y: number, w: number, h: number },
 *   minW?: number, minH?: number,
 *   onFrame: (rect: object) => void,
 *   onCommit: (rect: object) => void,
 *   onCancel: () => void,
 *   clampRect?: (rect: object) => object,
 * }} options
 */
export function startResize(options) {
  const start = { ...options.rect };
  const origin = {
    x: options.startX,
    y: options.startY,
    rectX: options.rect.x,
    rectY: options.rect.y,
    w: options.rect.w,
    h: options.rect.h,
  };
  const applyClamp = options.clampRect || ((r) => r);
  const minW = options.minW ?? 320;
  const minH = options.minH ?? 240;
  const edge = options.edge;
  let current = applyClamp({ ...start });
  let raf = 0;
  let cancelled = false;
  let finished = false;

  document.body.classList.add("is-resizing");
  document.body.dataset.resizeCursor = cursorFor(edge);

  const tick = () => {
    raf = 0;
    if (!cancelled && !finished) options.onFrame(current);
  };

  const onMove = (ev) => {
    if (cancelled || finished || ev.pointerId !== options.pointerId) return;
    const dx = ev.clientX - origin.x;
    const dy = ev.clientY - origin.y;
    let x = origin.rectX;
    let y = origin.rectY;
    let w = origin.w;
    let h = origin.h;
    if (edge.includes("e")) w = origin.w + dx;
    if (edge.includes("s")) h = origin.h + dy;
    if (edge.includes("w")) {
      w = origin.w - dx;
      x = origin.rectX + dx;
    }
    if (edge.includes("n")) {
      h = origin.h - dy;
      y = origin.rectY + dy;
    }
    if (w < minW) {
      if (edge.includes("w")) x = origin.rectX + origin.w - minW;
      w = minW;
    }
    if (h < minH) {
      if (edge.includes("n")) y = origin.rectY + origin.h - minH;
      h = minH;
    }
    current = applyClamp({ ...start, x, y, w, h });
    if (!raf) raf = requestAnimationFrame(tick);
  };

  const finish = (commit) => {
    if (finished) return;
    finished = true;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    window.removeEventListener("keydown", onKey, true);
    document.body.classList.remove("is-resizing");
    delete document.body.dataset.resizeCursor;
    if (raf) cancelAnimationFrame(raf);
    if (cancelled || !commit) options.onCancel();
    else options.onCommit(applyClamp(current));
  };

  const onUp = (ev) => {
    if (ev.pointerId !== options.pointerId) return;
    finish(true);
  };

  const onKey = (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      ev.stopPropagation();
      cancelled = true;
      current = applyClamp({ ...start });
      options.onFrame(current);
      finish(false);
    }
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
  window.addEventListener("keydown", onKey, true);
  options.onFrame(current);

  return {
    cancel: () => {
      cancelled = true;
      finish(false);
    },
  };
}

function cursorFor(edge) {
  if (edge === "e" || edge === "w") return "ew-resize";
  if (edge === "n" || edge === "s") return "ns-resize";
  if (edge === "nw" || edge === "se") return "nwse-resize";
  return "nesw-resize";
}

/**
 * Chip drag that starts after 5px. Used later for tabs (WO-10).
 */
export function chipDrag(options) {
  const threshold = 5;
  let dragging = false;
  let cancelled = false;
  const startX = options.startX;
  const startY = options.startY;
  const pointerId = options.pointerId;

  const onMove = (ev) => {
    if (cancelled || ev.pointerId !== pointerId) return;
    const dx = ev.clientX - startX;
    const dy = ev.clientY - startY;
    if (!dragging && Math.hypot(dx, dy) >= threshold) {
      dragging = true;
      options.onStart?.({ x: ev.clientX, y: ev.clientY });
    }
    if (dragging) options.onMove?.({ x: ev.clientX, y: ev.clientY });
  };

  const end = (ev) => {
    if (ev && ev.pointerId !== pointerId) return;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);
    window.removeEventListener("keydown", onKey, true);
    if (cancelled) {
      options.onCancel?.();
      return;
    }
    if (dragging) {
      options.onDrop?.({ x: ev?.clientX ?? startX, y: ev?.clientY ?? startY });
      const swallow = (ce) => {
        ce.preventDefault();
        ce.stopPropagation();
        window.removeEventListener("click", swallow, true);
      };
      window.addEventListener("click", swallow, true);
    } else {
      options.onCancel?.();
    }
  };

  const onKey = (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      cancelled = true;
      dragging = false;
      options.onCancel?.();
      end({ pointerId });
    }
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", end);
  window.addEventListener("keydown", onKey, true);
}

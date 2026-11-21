/** Bottom-right toasts. */

let host = null;

function ensureHost() {
  if (host?.isConnected) return host;
  host = document.querySelector(".toast-host");
  if (!host) {
    host = document.createElement("div");
    host.className = "toast-host";
    document.body.appendChild(host);
  }
  return host;
}

export function showToast(message, { kind = "", href, hrefLabel } = {}) {
  const text = String(message || "").trim();
  if (!text && !href) return;
  const el = document.createElement("div");
  el.className = kind ? `toast ${kind}` : "toast";
  el.setAttribute("role", "status");
  if (text) el.append(text);
  if (href) {
    if (text) el.append(" ");
    const a = document.createElement("a");
    a.href = href;
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    a.textContent = hrefLabel || "Open on GitHub";
    el.appendChild(a);
  }
  ensureHost().appendChild(el);
  setTimeout(() => el.remove(), 5000);
}

export function listenForToastEvents() {
  window.addEventListener("dync-toast", (ev) => {
    showToast(ev.detail?.message, { kind: ev.detail?.kind || "" });
  });
}

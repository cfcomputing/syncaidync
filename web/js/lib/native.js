/** Bridge to the optional macOS app. */

export function isNative() {
  return globalThis.window?.__syncaidyncNative === true;
}

function post(message) {
  if (isNative()) window.webkit?.messageHandlers?.syncaidync?.postMessage(message);
}

export function setBadge(count) {
  post({ type: "badge", count });
}

export function notify({ id, title, body }) {
  post({ type: "notify", id, title, body });
}

export function ready() {
  post({ type: "ready" });
}

export function startNative({ openSession, liveCount }) {
  if (!isNative()) return;
  window.__syncaidyncLiveCount = liveCount;
  window.addEventListener("syncaidync:open", (event) => {
    const id = event.detail?.id;
    if (typeof id === "string" && id) openSession(id);
  });
}

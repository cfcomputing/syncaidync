import { api, ApiError } from "./lib/api.js";
import * as appearance from "./lib/appearance.js";
import * as themes from "./lib/themes.js";
import * as route from "./lib/route.js";
import * as native from "./lib/native.js";
import { startStore, store } from "./lib/store.js";
import { startAlerts } from "./lib/alerts.js";
import { listenForToastEvents } from "./lib/toast.js";
import { mountBackdrop } from "./lib/backdrop.js";
import "./components/dync-app.js";
import "./components/dync-new-session.js";
import "./components/dync-remove-session.js";
import "./components/dync-close-out.js";
import "./components/dync-settings.js";

function stripKeyFromHash() {
  // Printed link is `/#key=<hex>`. Also accept `#/?key=` and `#/&key=`.
  const hash = location.hash || "";
  const m = hash.match(/[#/?&]key=([^&]+)/);
  if (!m) return null;
  let key = m[1];
  try {
    key = decodeURIComponent(key);
  } catch {
    /* keep raw */
  }
  // Drop the key fragment entirely; land on the board hash.
  history.replaceState(null, "", `${location.pathname}${location.search}#/`);
  return key;
}

function mountApp(signedIn) {
  const el = document.querySelector("dync-app") || document.createElement("dync-app");
  if (signedIn) el.setAttribute("signed-in", "");
  else el.removeAttribute("signed-in");
  if (!el.isConnected) {
    document.body.appendChild(el);
  } else {
    // Force remount with the right mode.
    el.remove();
    document.body.appendChild(el);
  }
}

async function signInWithKey(key) {
  try {
    await api.post("/auth", { key });
  } catch (err) {
    console.warn("auth failed", err);
    mountApp(false);
    return false;
  }
  return true;
}

async function enterApp() {
  try {
    await api.get("/auth");
  } catch (err) {
    if (err instanceof ApiError && err.status === 401) {
      mountApp(false);
      return;
    }
    mountApp(false);
    return;
  }

  try {
    await startStore();
  } catch (err) {
    console.error(err);
    mountApp(false);
    return;
  }

  startAlerts();
  mountApp(true);
  native.ready();
}

async function boot() {
  // Strip the launch key before any routing so it never lingers in the bar.
  const key = stripKeyFromHash();

  appearance.init();
  mountBackdrop();
  await themes.initThemes();
  route.start();
  native.startNative({
    openSession: (id) => route.go({ name: "session", id }),
    liveCount: () => store.connected ? [...store.sessions.values()].filter((s) => s.live).length : null,
  });

  window.addEventListener("dync-unauthorized", () => {
    mountApp(false);
  });

  listenForToastEvents();

  // Same-document navigations to `/#key=…` (e.g. pasted while already open) must still auth.
  window.addEventListener("hashchange", () => {
    const late = stripKeyFromHash();
    if (!late) return;
    signInWithKey(late).then((ok) => {
      if (ok) enterApp();
    });
  });

  if (key) {
    const ok = await signInWithKey(key);
    if (!ok) return;
  }

  await enterApp();
}

boot();

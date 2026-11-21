/** Hash routing for the board and workspace screens. */

const CHANGE = "dync-route";

export function parse(hash) {
  const raw = String(hash || "").replace(/^#/, "");
  const qIndex = raw.indexOf("?");
  const path = (qIndex >= 0 ? raw.slice(0, qIndex) : raw).replace(/^\/+|\/+$/g, "");
  const query = qIndex >= 0 ? raw.slice(qIndex + 1) : "";
  const solo = /(^|&)solo=1(&|$)/.test(query);
  const parts = path ? path.split("/") : [];

  if (parts.length === 0) {
    return { name: "board", id: null, tool: null, solo: false };
  }
  if (parts[0] === "pulls" && parts.length === 1) {
    return { name: "pulls", id: null, tool: null, solo: false };
  }
  if (parts[0] === "p" && parts[1]) {
    return { name: "pull", id: parts[1], tool: null, solo: false };
  }
  if (parts[0] === "s" && parts[1]) {
    return {
      name: "session",
      id: parts[1],
      tool: parts[2] || null,
      solo: solo && !!parts[2],
    };
  }
  return { name: "board", id: null, tool: null, solo: false };
}

export function format(route) {
  if (!route || route.name === "board") return "#/";
  if (route.name === "pulls") return "#/pulls";
  if (route.name === "pull" && route.id) return `#/p/${route.id}`;
  if (route.name === "session" && route.id) {
    let h = `#/s/${route.id}`;
    if (route.tool) {
      h += `/${route.tool}`;
      if (route.solo) h += "?solo=1";
    }
    return h;
  }
  return "#/";
}

export function go(route) {
  const next = format(route);
  if (location.hash !== next) {
    location.hash = next;
  } else {
    emit();
  }
}

export function current() {
  return parse(location.hash);
}

function emit() {
  window.dispatchEvent(new CustomEvent(CHANGE, { detail: current() }));
}

export function start() {
  window.addEventListener("hashchange", emit);
  if (!location.hash || location.hash === "#") {
    history.replaceState(null, "", `${location.pathname}${location.search}#/`);
  }
  emit();
}

export { CHANGE as ROUTE_EVENT };

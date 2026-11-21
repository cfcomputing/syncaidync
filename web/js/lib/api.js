/** Thin fetch wrapper for /api/v1. */

export class ApiError extends Error {
  constructor({ code, message, status }) {
    super(message || code || "Request failed");
    this.name = "ApiError";
    this.code = code || "error";
    this.message = message || this.message;
    this.status = status ?? 0;
  }
}

async function request(method, path, body) {
  const url = path.startsWith("/") ? `/api/v1${path}` : `/api/v1/${path}`;
  const headers = { Accept: "application/json" };
  const init = { method, credentials: "same-origin", headers };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  const res = await fetch(url, init);
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent("dync-unauthorized"));
  }
  if (res.status === 204) return null;
  const text = await res.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  if (!res.ok) {
    const err = data?.error || {};
    throw new ApiError({
      code: err.code || "http_error",
      message: err.message || res.statusText || "Request failed",
      status: res.status,
    });
  }
  return data;
}

export const api = {
  get: (path) => request("GET", path),
  post: (path, body) => request("POST", path, body),
  put: (path, body) => request("PUT", path, body),
  patch: (path, body) => request("PATCH", path, body),
  del: (path) => request("DELETE", path),
};

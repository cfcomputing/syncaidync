// WCAG 2.2 AA contrast for the colour tokens in web/css/tokens.css, in both appearances.
//
// Text needs 4.5:1 (SC 1.4.3). Control boundaries, focus indicators and state marks need
// 3:1 (SC 1.4.11). Translucent tokens are composited over the surface they sit on, and
// tinted fills (a state colour mixed into a surface) are checked at their strongest tint.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { accentFill, contrast, mix, readableInk } from "../js/lib/themes.js";

const here = dirname(fileURLToPath(import.meta.url));
const cssDir = join(here, "..", "css");
const tokensCss = readFileSync(join(cssDir, "tokens.css"), "utf8");

const TEXT = 4.5;
const UI = 3;

/** Declarations of one rule block, keyed by custom property name (with the leading --). */
function block(selector) {
  const start = tokensCss.indexOf(`${selector} {`);
  assert.ok(start >= 0, `tokens.css has a ${selector} block`);
  const end = tokensCss.indexOf("\n}", start);
  const body = tokensCss.slice(start, end);
  const out = {};
  for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

const night = block(":root");
const themes = {
  night,
  day: { ...night, ...block("html[data-appearance='day']") },
};

/** Parse #hex or rgb()/rgba() into [r, g, b, a]. */
function rgba(value) {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) h = [...h].map((c) => c + c).join("");
    const n = Number.parseInt(h, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const fn = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(value);
  if (fn) return [Number(fn[1]), Number(fn[2]), Number(fn[3]), fn[4] == null ? 1 : Number(fn[4])];
  throw new Error(`unsupported colour ${value}`);
}

function hex([r, g, b]) {
  return `#${[r, g, b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
}

/** Composite a (possibly translucent) colour over an opaque hex background. */
function over(value, bgHex) {
  const [r, g, b, a] = rgba(value);
  const [br, bg, bb] = rgba(bgHex);
  return hex([r * a + br * (1 - a), g * a + bg * (1 - a), b * a + bb * (1 - a)]);
}

/** `color-mix(in srgb, tint pct%, base)`, as the stylesheets write tinted fills. */
function tint(color, pct, base) {
  return mix(base, color, pct / 100);
}

/**
 * Pairs every colour token must meet. `fg` and `bg` are token names; `tint` mixes a state
 * colour into the background at the strongest percentage the stylesheets use with it.
 */
const SURFACES = ["--bg", "--surface", "--surface-2"];
const pairs = [];
for (const surface of SURFACES) {
  for (const fg of ["--text", "--text-muted", "--accent-ink", "--danger", "--ok", "--warn"]) {
    pairs.push({ fg, bg: surface, min: TEXT, why: "text" });
  }
  pairs.push({ fg: "--control-border", bg: surface, min: UI, why: "form control boundary" });
  pairs.push({ fg: "--accent", bg: surface, min: UI, why: "focused field border" });
  pairs.push({ fg: "--accent-ink", bg: surface, min: UI, why: "focus outline" });
}
// Status text on its own tinted chip (badges, notices, diff markers).
for (const [state, pct] of [["--ok", 12], ["--warn", 16], ["--danger", 12]]) {
  for (const surface of SURFACES) {
    pairs.push({ fg: state, bg: surface, tint: [state, pct], min: TEXT, why: "status chip" });
    pairs.push({ fg: "--text", bg: surface, tint: [state, pct], min: TEXT, why: "text on chip" });
  }
}
// Selected rows and active choices are tinted with the accent.
for (const surface of SURFACES) {
  pairs.push({ fg: "--text", bg: surface, tint: ["--accent", 40], min: TEXT, why: "selected row" });
  pairs.push({ fg: "--accent-ink", bg: surface, tint: ["--accent", 12], min: TEXT, why: "active choice" });
  pairs.push({ fg: "--accent-ink", bg: surface, tint: ["--accent", 18], min: TEXT, why: "selected file" });
  pairs.push({ fg: "--text-muted", bg: surface, tint: ["--accent", 12], min: TEXT, why: "muted on hover" });
  pairs.push({ fg: "--text", bg: surface, tint: ["--accent", 22], min: TEXT, why: "margin mark" });
  pairs.push({ fg: "--text", bg: surface, tint: ["--accent", 40], min: TEXT, why: "margin mark hover" });
  pairs.push({ fg: "--accent-2", bg: surface, min: UI, why: "working ring" });
}
pairs.push(
  { fg: "--on-accent", bg: "--accent", min: TEXT, why: "primary button" },
  { fg: "--on-danger", bg: "--danger", min: TEXT, why: "danger button" },
  { fg: "--on-warn", bg: "--warn", min: TEXT, why: "warning fill" },
  { fg: "--term-fg", bg: "--term-bg", min: TEXT, why: "terminal text" },
  { fg: "--term-dim", bg: "--term-bg", min: TEXT, why: "terminal dim text" },
);

// Decorative only: hairlines between regions that are already told apart by spacing,
// a surface change or their content. Not a control boundary, so SC 1.4.11 does not apply;
// form controls use --control-border instead. Checked so it stays visible but quiet.
const DECORATIVE = [{ fg: "--border", bg: "--surface", min: 1.1, max: 2 }];

function resolve(theme, name) {
  const v = theme[name];
  assert.ok(v, `${name} is defined`);
  return v;
}

function measure(theme, pair) {
  let bg = resolve(theme, pair.bg);
  if (pair.tint) bg = tint(resolve(theme, pair.tint[0]), pair.tint[1], bg);
  const fg = over(resolve(theme, pair.fg), bg);
  return { fg, bg, ratio: contrast(fg, bg) };
}

for (const [name, theme] of Object.entries(themes)) {
  describe(`contrast: ${name}`, () => {
    for (const pair of pairs) {
      const on = pair.tint ? `${pair.tint[0]} ${pair.tint[1]}% on ${pair.bg}` : pair.bg;
      it(`${pair.fg} on ${on} ≥ ${pair.min} (${pair.why})`, () => {
        const { fg, bg, ratio } = measure(theme, pair);
        assert.ok(ratio >= pair.min, `${pair.fg} ${fg} on ${bg} is ${ratio.toFixed(2)}:1`);
      });
    }
    for (const pair of DECORATIVE) {
      it(`${pair.fg} on ${pair.bg} stays a quiet hairline`, () => {
        const { ratio } = measure(theme, pair);
        assert.ok(ratio >= pair.min && ratio <= pair.max, `${ratio.toFixed(2)}:1`);
      });
    }
    it("every colour token in this theme is checked", () => {
      const checked = new Set();
      for (const p of [...pairs, ...DECORATIVE]) {
        checked.add(p.fg);
        checked.add(p.bg);
        if (p.tint) checked.add(p.tint[0]);
      }
      const colours = Object.entries(theme)
        .filter(([, v]) => /^(#|rgba?\()/i.test(v))
        .map(([k]) => k);
      const missing = colours.filter((k) => !checked.has(k));
      assert.deepEqual(missing, [], `unchecked colour tokens: ${missing.join(", ")}`);
    });
  });
}

describe("contrast: no colour tokens outside tokens.css", () => {
  it("other stylesheets reuse tokens rather than define colours", () => {
    for (const file of readdirSync(cssDir).filter((f) => f.endsWith(".css") && f !== "tokens.css")) {
      const css = readFileSync(join(cssDir, file), "utf8");
      const found = [...css.matchAll(/--([\w-]+)\s*:\s*(#[0-9a-f]{3,8}|rgba?\(|hsla?\(|oklch\()/gi)];
      assert.deepEqual(found.map((m) => m[1]), [], `${file} defines colour tokens`);
    }
  });
});

// Accent themes replace --accent, --accent-2, --accent-ink and --on-accent at runtime.
describe("contrast: accent themes", () => {
  const gradients = JSON.parse(
    readFileSync(join(here, "..", "vendor", "uigradients", "gradients.json"), "utf8"),
  );
  const accents = gradients
    .filter((g) => Array.isArray(g.colors) && g.colors.length >= 2)
    .map((g) => ({ name: g.name, accent: g.colors[0], accent2: g.colors.at(-1) }))
    .filter((t) => /^#[0-9a-f]{6}$/i.test(t.accent) && /^#[0-9a-f]{6}$/i.test(t.accent2));

  it("loads the vendored theme list", () => assert.ok(accents.length > 50));

  for (const [name, theme] of Object.entries(themes)) {
    it(`--accent-ink reaches ${TEXT}:1 on every surface for every theme (${name})`, () => {
      const bad = [];
      for (const t of accents) {
        const ink = readableInk(t.accent, SURFACES.map((s) => theme[s]), theme["--text"]);
        for (const s of SURFACES) {
          const r = contrast(ink, theme[s]);
          if (r < TEXT) bad.push(`${t.name} ${ink} on ${s} ${r.toFixed(2)}`);
        }
      }
      assert.deepEqual(bad, []);
    });
  }

  it(`--on-accent reaches ${TEXT}:1 on the --accent fill for every theme`, () => {
    const bad = [];
    for (const t of accents) {
      const { fill, on } = accentFill(t.accent);
      const r = contrast(on, fill);
      if (r < TEXT) bad.push(`${t.name} ${t.accent} ${r.toFixed(2)}`);
    }
    assert.deepEqual(bad, []);
  });
});

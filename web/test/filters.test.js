// Filters and overlays in the stylesheets that would undercut the token contrast checks in
// contrast.test.js. Those checks measure flat token pairs; a brightness filter or a light
// gloss painted under text changes the colours actually shown.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { contrast } from "../js/lib/themes.js";

const cssDir = join(dirname(fileURLToPath(import.meta.url)), "..", "css");
const sheets = Object.fromEntries(
  readdirSync(cssDir)
    .filter((f) => f.endsWith(".css"))
    .map((f) => [f, readFileSync(join(cssDir, f), "utf8")]),
);

/** Declarations of the first rule whose selector is exactly `selector`. */
function rule(css, selector) {
  const start = css.indexOf(`${selector} {`);
  assert.ok(start >= 0, `found ${selector}`);
  return css.slice(start, css.indexOf("}", start));
}

function tokens(selector) {
  const out = {};
  for (const m of rule(sheets["tokens.css"], selector).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    out[m[1]] = m[2].trim();
  }
  return out;
}

/** grayscale(1) then brightness(b), as CSS filters apply them to sRGB values. */
function dimmed(hex, b) {
  const n = Number.parseInt(hex.slice(1), 16);
  const v = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  const grey = Math.min(255, Math.round((0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]) * b));
  const h = grey.toString(16).padStart(2, "0");
  return `#${h}${h}${h}`;
}

describe("filters keep text contrast", () => {
  it("no stylesheet brightens with a filter", () => {
    for (const [file, css] of Object.entries(sheets)) {
      for (const m of css.matchAll(/brightness\(\s*([\d.]+)\s*\)/g)) {
        assert.ok(Number(m[1]) <= 1, `${file} uses brightness(${m[1]})`);
      }
    }
  });

  it("buttons paint no gloss gradient under the label", () => {
    assert.doesNotMatch(rule(sheets["base.css"], "button"), /background-image/);
  });

  it("an ended terminal's dim replay text still reaches 4.5:1", () => {
    const css = rule(sheets["chrome.css"], "dync-terminal.is-stopped .xterm-host");
    const filter = /filter:\s*([^;]+);/.exec(css)[1];
    assert.match(filter, /^grayscale\(1\) brightness\([\d.]+\)$/, "only grayscale and brightness");
    const b = Number(/brightness\(([\d.]+)\)/.exec(filter)[1]);
    const night = tokens(":root");
    const day = { ...night, ...tokens("html[data-appearance='day']") };
    for (const t of [night, day]) {
      for (const fg of ["--term-fg", "--term-dim"]) {
        const ratio = contrast(dimmed(t[fg], b), dimmed(t["--term-bg"], b));
        assert.ok(ratio >= 4.5, `${fg} at brightness(${b}) is ${ratio.toFixed(2)}:1`);
      }
    }
  });
});

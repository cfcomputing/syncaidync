import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { escapeText, html, raw } from "../js/lib/element.js";

describe("escapeText", () => {
  it("escapes special characters and nulls", () => {
    assert.equal(escapeText(`a&b<c>"d"'e`), "a&amp;b&lt;c&gt;&quot;d&quot;&#39;e");
    assert.equal(escapeText(null), "");
    assert.equal(escapeText(undefined), "");
  });
});

describe("html", () => {
  it("escapes values, joins arrays, passes raw through, null as empty", () => {
    const markup = html`<p class="${'a"b'}">${"<script>"} ${null} ${["x", "y"]} ${raw("<b>ok</b>")}</p>`;
    assert.equal(markup.value, `<p class="a&quot;b">&lt;script&gt;  xy <b>ok</b></p>`);
  });

  it("nested html results are raw", () => {
    const inner = html`<em>${"hi"}</em>`;
    const outer = html`<div>${inner}</div>`;
    assert.equal(outer.value, "<div><em>hi</em></div>");
  });
});

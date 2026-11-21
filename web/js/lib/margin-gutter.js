/**
 * CodeMirror gutter for margin marks.
 * Not pure (imports the vendored editor); keep DOM clicks out of unit tests.
 */

import {
  GutterMarker,
  gutter,
  StateEffect,
  StateField,
  RangeSetBuilder,
} from "/vendor/codemirror/codemirror.js";
import { byStartLine } from "./margins.js";

function emptySet() {
  return new RangeSetBuilder().finish();
}

export const setMarginMarks = StateEffect.define();

class MarginMark extends GutterMarker {
  /**
   * @param {object[]} entries
   * @param {(entries: object[]) => void} onOpen
   */
  constructor(entries, onOpen) {
    super();
    this.entries = entries;
    this.onOpen = onOpen;
  }

  eq(other) {
    return (
      other instanceof MarginMark &&
      other.entries.length === this.entries.length &&
      other.entries.every((e, i) => e.id === this.entries[i]?.id)
    );
  }

  toDOM() {
    const btn = document.createElement("button");
    btn.type = "button";
    const outdated = this.entries.some((e) => e.status === "outdated");
    btn.className = `cm-margin-mark${outdated ? " is-outdated" : ""}`;
    const title =
      this.entries.length === 1
        ? this.entries[0].title || "Open margin"
        : `${this.entries.length} margins`;
    btn.title = outdated ? `${title} (outdated)` : title;
    btn.textContent = this.entries.length > 1 ? String(this.entries.length) : "¶";
    btn.addEventListener("mousedown", (ev) => {
      ev.preventDefault();
      ev.stopPropagation();
      this.onOpen?.(this.entries);
    });
    return btn;
  }
}

/**
 * @param {(entries: object[]) => void} onOpen
 */
export function createMarginGutter(onOpen) {
  const field = StateField.define({
    create() {
      return emptySet();
    },
    update(set, tr) {
      set = set.map(tr.changes);
      for (const e of tr.effects) {
        if (e.is(setMarginMarks)) set = e.value;
      }
      return set;
    },
  });

  const gutterExt = gutter({
    class: "cm-margin-gutter",
    markers: (view) => view.state.field(field),
    initialSpacer: () => new MarginMark([], onOpen),
  });

  return {
    extensions: [field, gutterExt],
    /** @param {import("/vendor/codemirror/codemirror.js").EditorView} view */
    apply(view, margins) {
      if (!view) return;
      const grouped = byStartLine(margins);
      const builder = new RangeSetBuilder();
      const lines = [...grouped.keys()].sort((a, b) => a - b);
      const doc = view.state.doc;
      for (const line of lines) {
        if (line < 1 || line > doc.lines) continue;
        const pos = doc.line(line).from;
        builder.add(pos, pos, new MarginMark(grouped.get(line), onOpen));
      }
      view.dispatch({ effects: setMarginMarks.of(builder.finish()) });
    },
  };
}

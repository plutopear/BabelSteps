// Pure helpers for the formatting buttons. They work on the restricted inline
// Markdown that core/src/model.rs documents and html.rs converts: bold `**`,
// italic `*`, underline `<u>..</u>` and inline code (backticks).
// No imports and no framework code, so `node --test` can run the tests.

export const BOLD = "**";
export const ITALIC = "*";
export const UNDERLINE = "<u>";
export const CODE = "`";
export type Marker = typeof BOLD | typeof ITALIC | typeof UNDERLINE | typeof CODE;

export interface Wrapped {
  text: string;
  selStart: number;
  selEnd: number;
}

function closer(marker: Marker): string {
  return marker === UNDERLINE ? "</u>" : marker;
}

function runBefore(text: string, at: number, ch: string): number {
  let n = 0;
  while (at - n > 0 && text[at - n - 1] === ch) n++;
  return n;
}

function runAfter(text: string, at: number, ch: string): number {
  let n = 0;
  while (at + n < text.length && text[at + n] === ch) n++;
  return n;
}

/**
 * How many marker characters sit on each side of [a, b) (0 when the marker is
 * not there). `outside` looks at the text just around the range; otherwise at
 * the edges inside the range. `*` and `**` share characters: `***x***` is bold
 * and italic, `**x**` is only bold, so italic is present only when the run of
 * asterisks is odd.
 */
function markerLength(text: string, a: number, b: number, marker: Marker, outside: boolean): [number, number] {
  const open = marker;
  const close = closer(marker);
  if (marker === ITALIC || marker === BOLD) {
    const left = outside ? runBefore(text, a, "*") : runAfter(text, a, "*");
    const right = outside ? runAfter(text, b, "*") : runBefore(text, b, "*");
    // Inside the range, a range made only of asterisks is not a marker pair.
    if (!outside && a + left >= b) return [0, 0];
    const run = Math.min(left, right);
    const n = marker === ITALIC ? (run % 2 === 1 ? 1 : 0) : run >= 2 ? 2 : 0;
    return [n, n];
  }
  if (outside) {
    return text.slice(0, a).endsWith(open) && text.slice(b).startsWith(close) ? [open.length, close.length] : [0, 0];
  }
  const inner = text.slice(a, b);
  return inner.length >= open.length + close.length && inner.startsWith(open) && inner.endsWith(close)
    ? [open.length, close.length]
    : [0, 0];
}

/** Narrow [a, b) so it does not start or end with whitespace (`**word **` is not bold in most renderers). */
function trimRange(text: string, a: number, b: number): [number, number] {
  let s = a;
  let e = b;
  while (s < e && /\s/.test(text[s])) s++;
  while (e > s && /\s/.test(text[e - 1])) e--;
  return s < e ? [s, e] : [a, b];
}

/**
 * Toggle `marker` around the selection [selStart, selEnd) of `text`.
 * - Selection not marked: wrap it. An empty selection inserts a pair with the
 *   caret between the markers.
 * - Selection already marked (markers just outside it, or at its edges):
 *   remove them.
 * The returned selection covers the same characters as before (without the markers).
 */
export function wrapSelection(text: string, selStart: number, selEnd: number, marker: Marker): Wrapped {
  let a = Math.max(0, Math.min(selStart, selEnd, text.length));
  let b = Math.min(text.length, Math.max(selStart, selEnd));
  [a, b] = trimRange(text, a, b);
  const open = marker;
  const close = closer(marker);

  if (a === b) {
    return { text: text.slice(0, a) + open + close + text.slice(a), selStart: a + open.length, selEnd: a + open.length };
  }

  const [oo, oc] = markerLength(text, a, b, marker, true);
  if (oo > 0) {
    return {
      text: text.slice(0, a - oo) + text.slice(a, b) + text.slice(b + oc),
      selStart: a - oo,
      selEnd: b - oo,
    };
  }
  const [io, ic] = markerLength(text, a, b, marker, false);
  if (io > 0) {
    return {
      text: text.slice(0, a) + text.slice(a + io, b - ic) + text.slice(b),
      selStart: a,
      selEnd: b - io - ic,
    };
  }
  return {
    text: text.slice(0, a) + open + text.slice(a, b) + close + text.slice(b),
    selStart: a + open.length,
    selEnd: b + open.length,
  };
}

/**
 * Like `wrapSelection`, but each line of a multi-line selection is formatted on
 * its own, for list items (one item per line). Empty lines are skipped. If every
 * line is already marked they are all unmarked, otherwise the unmarked lines get
 * marked. A single-line selection behaves exactly like `wrapSelection`.
 */
export function wrapLines(text: string, selStart: number, selEnd: number, marker: Marker): Wrapped {
  const s = Math.max(0, Math.min(selStart, selEnd, text.length));
  const e = Math.min(text.length, Math.max(selStart, selEnd));
  if (!text.slice(s, e).includes("\n")) return wrapSelection(text, s, e, marker);

  const lines: [number, number][] = [];
  let at = s;
  for (const part of text.slice(s, e).split("\n")) {
    const [a, b] = trimRange(text, at, at + part.length);
    if (a < b) lines.push([a, b]);
    at += part.length + 1;
  }
  if (lines.length === 0) return { text, selStart: s, selEnd: e };
  const marked = lines.map(
    ([a, b]) => markerLength(text, a, b, marker, true)[0] > 0 || markerLength(text, a, b, marker, false)[0] > 0,
  );
  const all = marked.every(Boolean);

  // Work from the last line up so earlier offsets stay valid.
  let cur = text;
  let start = s;
  let lastEnd = e;
  let above = 0; // length change from lines above the last one
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!all && marked[i]) continue;
    const before = cur.length;
    const r = wrapSelection(cur, lines[i][0], lines[i][1], marker);
    cur = r.text;
    if (i === lines.length - 1) lastEnd = r.selEnd;
    else above += cur.length - before;
    if (i === 0) start = r.selStart;
  }
  return { text: cur, selStart: start, selEnd: lastEnd + above };
}

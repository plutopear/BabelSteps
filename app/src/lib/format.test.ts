// Run with `npm test` (plain Node 22, no extra packages). Excluded from svelte-check.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BOLD, CODE, ITALIC, UNDERLINE, wrapLines, wrapSelection } from "./format.ts";

const sel = (r: { text: string; selStart: number; selEnd: number }) => r.text.slice(r.selStart, r.selEnd);

test("wraps a selection and keeps it selected", () => {
  const r = wrapSelection("say hello now", 4, 9, BOLD);
  assert.equal(r.text, "say **hello** now");
  assert.equal(sel(r), "hello");
});

test("each marker uses the syntax the exporters read", () => {
  assert.equal(wrapSelection("a", 0, 1, ITALIC).text, "*a*");
  assert.equal(wrapSelection("a", 0, 1, UNDERLINE).text, "<u>a</u>");
  assert.equal(wrapSelection("a", 0, 1, CODE).text, "`a`");
  assert.equal(sel(wrapSelection("a", 0, 1, UNDERLINE)), "a");
});

test("toggles off when the markers are just outside the selection", () => {
  for (const m of [BOLD, ITALIC, UNDERLINE, CODE]) {
    const on = wrapSelection("x word y", 2, 6, m);
    const off = wrapSelection(on.text, on.selStart, on.selEnd, m);
    assert.equal(off.text, "x word y", m);
    assert.equal(sel(off), "word", m);
  }
});

test("toggles off when the selection includes the markers", () => {
  const r = wrapSelection("a **b** c", 2, 7, BOLD);
  assert.equal(r.text, "a b c");
  assert.equal(sel(r), "b");
  const u = wrapSelection("a <u>b</u> c", 2, 10, UNDERLINE);
  assert.equal(u.text, "a b c");
});

test("empty selection inserts a pair with the caret between", () => {
  const r = wrapSelection("ab", 1, 1, BOLD);
  assert.equal(r.text, "a****b");
  assert.deepEqual([r.selStart, r.selEnd], [3, 3]);
  const u = wrapSelection("", 0, 0, UNDERLINE);
  assert.equal(u.text, "<u></u>");
  assert.equal(u.selStart, 3);
});

test("italic and bold do not mistake each other's asterisks", () => {
  // Bold text made italic: ***b*** , then italic off again.
  const it = wrapSelection("**b**", 2, 3, ITALIC);
  assert.equal(it.text, "***b***");
  assert.equal(wrapSelection(it.text, it.selStart, it.selEnd, ITALIC).text, "**b**");
  // Italic text made bold, then bold off.
  const b = wrapSelection("*i*", 1, 2, BOLD);
  assert.equal(b.text, "***i***");
  assert.equal(wrapSelection(b.text, b.selStart, b.selEnd, BOLD).text, "*i*");
});

test("whitespace at the edges of the selection stays outside the markers", () => {
  const r = wrapSelection("say hello now", 3, 10, BOLD);
  assert.equal(r.text, "say **hello** now");
  assert.equal(sel(r), "hello");
});

test("reversed and out of range selections are clamped", () => {
  assert.equal(wrapSelection("abc", 3, 1, BOLD).text, "a**bc**");
  assert.equal(wrapSelection("abc", 1, 99, BOLD).text, "a**bc**");
});

test("a selection of only whitespace is wrapped as is, not dropped", () => {
  assert.equal(wrapSelection("a  b", 1, 3, CODE).text, "a`  `b");
});

test("wrapLines formats each line on its own", () => {
  const r = wrapLines("one\ntwo\n\nthree", 0, 14, BOLD);
  assert.equal(r.text, "**one**\n**two**\n\n**three**");
  assert.equal(r.selStart, 2);
  assert.equal(r.selEnd, r.text.length - 2);
});

test("wrapLines marks the unmarked lines, and unmarks only when all are marked", () => {
  const mixed = wrapLines("**one**\ntwo", 0, 11, BOLD);
  assert.equal(mixed.text, "**one**\n**two**");
  const off = wrapLines(mixed.text, 0, mixed.text.length, BOLD);
  assert.equal(off.text, "one\ntwo");
});

test("wrapLines on one line is the same as wrapSelection", () => {
  assert.deepEqual(wrapLines("a b", 0, 1, ITALIC), wrapSelection("a b", 0, 1, ITALIC));
});

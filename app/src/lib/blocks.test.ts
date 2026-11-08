import { test } from "node:test";
import assert from "node:assert/strict";
import { canMergeWithNext, mergeSteps, mergeWithNext } from "./blocks.ts";
import type { Block, StepBlock } from "./types.ts";

function step(id: string, description: string, over: Partial<StepBlock> = {}): StepBlock {
  return {
    type: "step",
    id,
    action: { kind: "click", button: "left", x: 1, y: 2 },
    title: `Title ${id}`,
    description,
    screenshot: `images/${id}.png`,
    target: null,
    redactions: [{ x: 1, y: 2, w: 3, h: 4 }],
    captured_unix_ms: 5,
    screenshot_withheld: false,
    ...over,
  };
}

test("merge keeps the first step's screenshot and description and appends the second description", () => {
  const m = mergeSteps(step("a", "First."), step("b", "Second.", { redactions: [{ x: 9, y: 9, w: 9, h: 9 }] }));
  assert.equal(m.id, "a");
  assert.equal(m.title, "Title a");
  assert.equal(m.screenshot, "images/a.png");
  assert.equal(m.description, "First.\nSecond.");
  // The dropped screenshot's redaction boxes go with it.
  assert.deepEqual(m.redactions, [{ x: 1, y: 2, w: 3, h: 4 }]);
  assert.equal(m.captured_unix_ms, 5);
});

test("an empty description on either side adds no blank line", () => {
  assert.equal(mergeSteps(step("a", ""), step("b", "Second.")).description, "Second.");
  assert.equal(mergeSteps(step("a", "First."), step("b", "")).description, "First.");
  assert.equal(mergeSteps(step("a", ""), step("b", "  ")).description, "");
  assert.equal(mergeSteps(step("a", "First.\n"), step("b", "Second.")).description, "First.\nSecond.");
});

test("merge does not change its inputs and does not share redaction boxes", () => {
  const a = step("a", "A");
  const b = step("b", "B");
  const before = JSON.stringify([a, b]);
  const m = mergeSteps(a, b);
  assert.equal(JSON.stringify([a, b]), before);
  m.redactions[0].x = 99;
  assert.equal(a.redactions[0].x, 1);
});

test("mergeWithNext replaces two adjacent steps with one and leaves other blocks alone", () => {
  const blocks: Block[] = [
    { type: "heading", id: "h", level: 1, text: "H" },
    step("a", "A"),
    step("b", "B"),
    { type: "text", id: "t", text: "T" },
  ];
  const out = mergeWithNext(blocks, 1)!;
  assert.deepEqual(out.map((b) => b.id), ["h", "a", "t"]);
  assert.equal((out[1] as StepBlock).description, "A\nB");
  assert.equal(blocks.length, 4);
});

test("mergeWithNext refuses when the next block is not a step, or there is none", () => {
  const blocks: Block[] = [step("a", "A"), { type: "text", id: "t", text: "T" }, step("b", "B")];
  assert.equal(canMergeWithNext(blocks, 0), false);
  assert.equal(mergeWithNext(blocks, 0), null);
  assert.equal(mergeWithNext(blocks, 1), null);
  assert.equal(mergeWithNext(blocks, 2), null);
  assert.equal(mergeWithNext(blocks, 9), null);
});

// Pure block operations for the editor (no framework code, testable with `node --test`).
import type { Block, StepBlock } from "./types";

/** True when block `i` and the block right after it are both steps. */
export function canMergeWithNext(blocks: readonly Block[], i: number): boolean {
  return blocks[i]?.type === "step" && blocks[i + 1]?.type === "step";
}

/**
 * Merge a step with the one after it (B6). The result keeps the first step as
 * it is (id, title, action, screenshot, redaction boxes, time) and adds the
 * second step's description as an extra line. The second step's screenshot,
 * its redaction boxes, title, and recorded action are dropped from the
 * document; its image file stays in the recording folder. Returns a new step;
 * neither argument is changed.
 */
export function mergeSteps(first: StepBlock, second: StepBlock): StepBlock {
  const a = first.description.trim();
  const b = second.description.trim();
  return {
    ...first,
    redactions: first.redactions.map((r) => ({ ...r })),
    description: a && b ? `${first.description.replace(/\s+$/, "")}\n${b}` : a ? first.description : b,
  };
}

/**
 * Merge block `i` with block `i + 1` and return a new block list, or null when
 * the two are not both steps. `blocks` is not changed.
 */
export function mergeWithNext(blocks: readonly Block[], i: number): Block[] | null {
  if (!canMergeWithNext(blocks, i)) return null;
  const merged = mergeSteps(blocks[i] as StepBlock, blocks[i + 1] as StepBlock);
  return [...blocks.slice(0, i), merged, ...blocks.slice(i + 2)];
}

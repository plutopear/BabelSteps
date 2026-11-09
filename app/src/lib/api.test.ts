// Run with `npm test` (plain Node 22, no extra packages). Excluded from svelte-check.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describeError, toAppError } from "./api.ts";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const snake = (s: string) => s.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();

/** The error kinds the Rust side can send, read from its enum. */
function rustKinds(): string[] {
  const src = read("../../src-tauri/src/error.rs");
  const body = /pub enum ErrorKind \{([\s\S]*?)\n\}/.exec(src)?.[1] ?? "";
  return body
    .split("\n")
    .filter((l) => !l.trim().startsWith("//"))
    .map((l) => /^\s*([A-Z][A-Za-z]+),\s*$/.exec(l)?.[1])
    .filter((v): v is string => !!v)
    .map(snake);
}

test("the frontend knows every error kind the backend can send, and no others", () => {
  const rust = rustKinds();
  assert.ok(rust.length >= 9, `read ${rust.length} kinds from error.rs`);
  const api = read("./api.ts");
  const list = /const KINDS: ErrorKind\[\] = \[([\s\S]*?)\];/.exec(api)?.[1] ?? "";
  const inApi = [...list.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  assert.deepEqual([...inApi].sort(), [...rust].sort());
  const types = /export type ErrorKind =([\s\S]*?);/.exec(read("./types.ts"))?.[1] ?? "";
  const inTypes = [...types.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]).filter((k) => k !== "unknown");
  assert.deepEqual([...inTypes].sort(), [...rust].sort());
});

test("toAppError accepts known kinds and falls back to unknown for anything else", () => {
  assert.deepEqual(toAppError({ kind: "busy", message: "x" }), { kind: "busy", message: "x" });
  assert.deepEqual(toAppError({ kind: "from_the_future", message: "x" }), { kind: "unknown", message: "x" });
  assert.deepEqual(toAppError({ kind: "busy", message: 42 }), { kind: "busy", message: "" });
  for (const odd of [null, undefined, "boom", 7, {}, [], new Error("e")]) {
    assert.equal(toAppError(odd).kind, "unknown");
  }
});

test("every error kind gets plain text that does not repeat the backend message", () => {
  const logged: unknown[][] = [];
  const original = console.error;
  console.error = (...args: unknown[]) => void logged.push(args);
  try {
    const seen = new Set<string>();
    for (const kind of [...rustKinds(), "unknown"]) {
      const text = describeError({ kind, message: "SECRET-TITLE C:\\Users\\me\\doc" }, "saving");
      assert.ok(text.length > 20, kind);
      assert.ok(!text.includes("SECRET") && !text.includes("Users"), `${kind} repeats the message`);
      seen.add(text);
    }
    // Each kind has its own advice (only "unknown" shares the fallback text).
    assert.equal(seen.size, rustKinds().length + 1);
    // The console gets the kind only.
    assert.ok(logged.length > 0);
    for (const args of logged) assert.ok(!JSON.stringify(args).includes("SECRET"));
  } finally {
    console.error = original;
  }
});

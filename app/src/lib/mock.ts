// BABELSTEPS_MOCK_BACKEND: dev-only, in-memory stand-in for the Tauri commands.
// Sample data is made up. Never imported in production builds (see api.ts).
import type { Block, Recording, StepBlock, ExportSummary, Template } from "./types";

const FOLDER = "C:\\Sample\\BabelSteps";
const store = new Map<string, Recording>();
let open: string | null = null;
let seq = 0;

function step(n: number, title: string, withheld = false): StepBlock {
  return {
    type: "step",
    id: `s${++seq}`,
    action: { kind: "click", button: "left", x: 100 + n * 10, y: 200 },
    title,
    description: "",
    screenshot: withheld ? null : `images/step-00${n}.png`,
    target: null,
    redactions: [],
    captured_unix_ms: 0,
    screenshot_withheld: withheld,
  };
}

function seed(name: string, blocks: Block[], created: number) {
  const id = name.toLowerCase().replace(/\W+/g, "-");
  store.set(`${FOLDER}\\${id}.bsrec`, {
    format_version: 2, id, title: name, created_unix_ms: created, blocks,
  });
}

// Same sections as core/src/templates.rs (dev-only copy).
function templateBlocks(t: Template): Block[] {
  const h = (id: string, text: string): Block => ({ type: "heading", id, level: 2, text });
  const p = (id: string, text: string): Block => ({ type: "text", id, text });
  const l = (id: string, item: string): Block => ({ type: "list", id, ordered: false, items: [item] });
  if (t === "helpdesk_how_to")
    return [
      h("template-purpose", "Purpose"),
      p("template-purpose-text", "Describe in one sentence what this guide helps someone do."),
      h("template-before", "Before you start"),
      l("template-before-list", "Add anything the reader needs first, or delete this section."),
      h("template-steps", "Steps"),
      h("template-result", "Result"),
      p("template-result-text", "Describe what the reader should have when they finish."),
    ];
  if (t === "training_guide")
    return [
      h("template-goal", "Goal"),
      p("template-goal-text", "Describe in one sentence what the learner will be able to do."),
      { type: "callout", id: "template-tip", kind: "tip", text: "Take your time; you can repeat these steps as often as you like." },
      h("template-needs", "What you need"),
      l("template-needs-list", "Add anything the learner needs first, or delete this section."),
      h("template-steps", "Steps"),
      h("template-expect", "What you should see"),
      p("template-expect-text", "Describe what the learner should see when they finish."),
    ];
  return [];
}

seed("Sample: Reset a password", [
  { type: "heading", id: "h1", level: 1, text: "Reset a password" },
  step(1, "Left-click \"Settings\" button in Sample App"),
  step(2, "Left-click at (120, 340)"),
  { ...step(3, "Left-click \"Password\" edit in Sample App"), screenshot: null, screenshot_withheld: true },
  { type: "callout", id: "c1", kind: "tip", text: "Sample tip text." },
], 1_700_000_000_000);
seed("Sample: Map a network drive", [
  step(1, "Left-click \"This PC\" in Sample Explorer"),
  step(2, "Left-click \"Map network drive\" button"),
], 1_700_100_000_000);
seed("Sample: Empty recording", [], 1_700_200_000_000);

function current(): Recording {
  if (!open) throw { kind: "no_recording_open", message: "" };
  return store.get(open)!;
}

async function placeholder(path: string): Promise<ArrayBuffer> {
  const c = document.createElement("canvas");
  c.width = 1280; c.height = 720;
  const g = c.getContext("2d")!;
  g.fillStyle = "#dfe6ee"; g.fillRect(0, 0, 1280, 720);
  g.fillStyle = "#3b5b84"; g.fillRect(0, 0, 1280, 48);
  g.fillStyle = "#fff"; g.fillRect(80, 120, 760, 420);
  g.fillStyle = "#9db2cc";
  for (let i = 0; i < 6; i++) g.fillRect(110, 150 + i * 60, 300 + (i % 3) * 120, 24);
  g.fillStyle = "#c0392b"; g.fillRect(900, 120, 280, 80);
  g.fillStyle = "#222"; g.font = "28px sans-serif";
  g.fillText("Placeholder image: " + path.split("/").pop(), 110, 600);
  const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), "image/png"));
  return blob.arrayBuffer();
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function summary(rec: Recording): ExportSummary {
  const steps = rec.blocks.filter((b): b is StepBlock => b.type === "step");
  return {
    steps: steps.length,
    imagesWritten: steps.filter((s) => s.screenshot && !s.screenshot_withheld).length,
    imagesWithRedactions: steps.filter((s) => s.redactions.length > 0).length,
    withheld: steps.filter((s) => s.screenshot_withheld).length,
  };
}

// Simulated recorder: one step every ~2 s while active and not paused.
const R = {
  active: false, paused: false, keystrokes: false, path: null as string | null, steps: 0, notes: 0, noteRequests: 0, last: 0,
  notice: null as string | null,
  scope: { kind: "screen", label: "" } as { kind: "screen" | "monitor" | "window"; label: string },
};
const MOCK_TARGETS = {
  monitors: [
    { device: "\\\\.\\DISPLAY1", label: "Main monitor (2560 × 1440)" },
    { device: "\\\\.\\DISPLAY2", label: "Monitor 2 (1920 × 1080)" },
  ],
  windows: [
    { id: 101, label: "Sample document - Notepad (notepad)" },
    { id: 102, label: "Sample page - Edge (msedge)" },
  ],
};
function recStatus() {
  const now = Date.now();
  if (R.active && !R.paused && now - R.last >= 2000) {
    const n = Math.floor((now - R.last) / 2000);
    R.steps += n;
    R.last += n * 2000;
  }
  return {
    active: R.active, paused: R.active && R.paused, keystrokes: R.active && R.keystrokes, path: R.path,
    steps: R.steps, withheld: R.active && R.steps >= 3 ? 1 : 0,
    warnings: R.active && R.steps >= 3 ? ["Sample warning: a slow check delayed one step."] : [],
    noteRequests: R.noteRequests,
    startRequests: 0,
    notice: R.notice,
    scope: R.active ? R.scope : { kind: "screen", label: "" },
    hotkeys: [
      { action: "Start or stop recording", keys: "Ctrl+Alt+Shift+R", registered: true },
      { action: "Pause or resume", keys: "Ctrl+Alt+Shift+P", registered: true },
      { action: "Add a note", keys: "Ctrl+Alt+Shift+N", registered: false },
      { action: "Stop now", keys: "Ctrl+Alt+Shift+X", registered: true },
    ],
  };
}
// Dev only: run __mockNote() in the console to simulate the Add-a-note hotkey.
(globalThis as Record<string, unknown>).__mockNote = () => R.noteRequests++;
let settings = { keepScreenshots: null as number | null, sizeWarningMb: 500, showCaptureBorder: true, hotkeys: { startStop: "Ctrl+Alt+Shift+R", pause: "Ctrl+Alt+Shift+P", note: "Ctrl+Alt+Shift+N", stop: "Ctrl+Alt+Shift+X" }, closeToTray: false, showWindowTitles: false, customWords: [] as string[], customPatterns: [] as string[] };
function busy(): never {
  throw { kind: "busy", message: "" };
}

// Dev-only stand-in for core/src/review.rs: a few simple patterns, and offsets
// in JS string units (the real backend uses bytes). Good enough to try the UI.
type MockField = { kind: string; index?: number };
function mockTexts(rec: Recording): { id: string | null; field: MockField; loc: string; get: () => string; set: (v: string) => void }[] {
  const out: ReturnType<typeof mockTexts> = [];
  out.push({ id: null, field: { kind: "document_title" }, loc: "document title", get: () => rec.title, set: (v) => (rec.title = v) });
  let n = 0;
  rec.blocks.forEach((b, i) => {
    const pos = i + 1;
    if (b.type === "step") {
      n++;
      out.push({ id: b.id, field: { kind: "title" }, loc: `Step ${n} title`, get: () => b.title, set: (v) => (b.title = v) });
      out.push({ id: b.id, field: { kind: "description" }, loc: `Step ${n} description`, get: () => b.description, set: (v) => (b.description = v) });
    } else if (b.type === "list") {
      b.items.forEach((_, k) => out.push({ id: b.id, field: { kind: "list_item", index: k }, loc: `Block ${pos} list item ${k + 1}`, get: () => b.items[k], set: (v) => (b.items[k] = v) }));
    } else if (b.type === "image") {
      out.push({ id: b.id, field: { kind: "caption" }, loc: `Block ${pos} image caption`, get: () => b.caption, set: (v) => (b.caption = v) });
    } else {
      out.push({ id: b.id, field: { kind: b.type }, loc: `Block ${pos} ${b.type}`, get: () => b.text, set: (v) => (b.text = v) });
    }
  });
  return out;
}
function mockMatches(text: string): { start: number; end: number; kind: string; label: string }[] {
  const rules: [RegExp, string, string][] = [
    [/[\w.+-]+@[\w-]+(\.[\w-]+)*\.[A-Za-z]{2,}/g, "email", "email address"],
    [/\b\d{3}[-. ]\d{3}[-. ]\d{4}\b/g, "phone", "phone number"],
    [/\b\d{1,3}(\.\d{1,3}){3}\b/g, "ip_address", "IP address"],
    ...settings.customWords.filter(Boolean).map((w): [RegExp, string, string] => [new RegExp(w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi"), "custom", "custom word or pattern"]),
  ];
  const out: ReturnType<typeof mockMatches> = [];
  for (const [re, kind, label] of rules) {
    for (const m of text.matchAll(re)) {
      const start = m.index ?? 0;
      if (text[start - 1] === "[" && text[start + m[0].length] === "]") continue; // already a placeholder
      out.push({ start, end: start + m[0].length, kind, label });
    }
  }
  return out.sort((a, b) => a.start - b.start);
}
function mockScan(rec: Recording) {
  return mockTexts(rec).flatMap((t) =>
    mockMatches(t.get()).map((m) => {
      const s = t.get().slice(m.start, m.end);
      return {
        blockId: t.id, field: t.field, kind: m.kind, start: m.start, end: m.end,
        kindLabel: m.label, location: t.loc, preview: s.length >= 8 ? `${s[0]}…${s[s.length - 1]}` : "…",
      };
    }),
  );
}

export async function mockInvoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  await wait(60);
  if (R.active && ["save_recording", "open_recording", "create_recording", "close_recording",
    "rename_recording", "delete_recording", "export_markdown", "export_pdf", "export_single_html"].includes(cmd)) busy();
  switch (cmd) {
    case "recording_status":
      return recStatus();
    case "list_capture_targets":
      return structuredClone(MOCK_TARGETS);
    case "get_settings":
      return structuredClone(settings);
    case "scan_recording":
      return mockScan(a.recording as Recording);
    case "hide_finding": {
      const rec = structuredClone(a.recording as Recording);
      const f = a.finding as ReturnType<typeof mockScan>[number];
      const t = mockTexts(rec).find((x) => x.id === f.blockId && x.field.kind === f.field.kind && x.field.index === f.field.index);
      const text = t?.get() ?? "";
      if (!t || !mockMatches(text).some((m) => m.start === f.start && m.end === f.end && m.kind === f.kind)) return null;
      const label = { email: "[email]", phone: "[phone]", ip_address: "[ip address]", custom: "[custom]" }[f.kind] ?? "[hidden]";
      t.set(text.slice(0, f.start) + label + text.slice(f.end));
      return rec;
    }
    case "save_settings": {
      const n = (a.settings as typeof settings);
      if (n.keepScreenshots !== null && n.keepScreenshots < 1) throw { kind: "invalid_argument", message: "" };
      // Like the real app, errors give the position and never quote the entry.
      for (const [list, noun] of [[n.customWords, "word"], [n.customPatterns, "pattern"]] as const) {
        if (list.length > 100) throw { kind: "invalid_argument", message: `too many custom ${noun}s (at most 100)` };
        for (const [i, e] of list.entries()) {
          let bad = e.trim() === "" ? "is empty" : e.length > 200 ? "is longer than 200 characters" : "";
          if (!bad && noun === "pattern") try { new RegExp(e); } catch { bad = "is not valid"; }
          if (bad) throw { kind: "invalid_argument", message: `custom ${noun} ${i + 1} ${bad}` };
        }
      }
      settings = structuredClone(n);
      return Object.values(n.hotkeys).filter((k) => k.split("+").length < 4).map((k) => `${k} may clash with browsers or other apps.`);
    }
    case "export_settings":
      return null;
    case "import_settings": {
      // Dev only: a file name ending in bad.json plays a refused file.
      if (String(a.path).endsWith("bad.json")) throw { kind: "invalid_argument", message: "This file is not a BabelSteps settings export." };
      settings = { ...structuredClone(settings), keepScreenshots: 50, closeToTray: true };
      return [];
    }
    case "clear_notice":
      R.notice = null;
      return;
    case "set_editor_unsaved":
      return null;
    case "start_recording": {
      if (R.active) busy();
      if (!open) {
        open = `${FOLDER}\\recording-${++seq}.bsrec`;
        store.set(open, { format_version: 2, id: `r${seq}`, title: "Recording", created_unix_ms: Date.now(), blocks: [] });
      }
      const t = (a.target ?? { kind: "screen" }) as { kind: string; device?: string; id?: number };
      const scope =
        t.kind === "monitor"
          ? { kind: "monitor" as const, label: MOCK_TARGETS.monitors.find((m) => m.device === t.device)?.label ?? "one monitor" }
          : t.kind === "window"
            ? { kind: "window" as const, label: t.id === 101 ? "notepad" : "msedge" }
            : { kind: "screen" as const, label: "" };
      Object.assign(R, { active: true, paused: false, keystrokes: Boolean(a.keystrokes), path: open, steps: 0, notes: 0, last: Date.now(), scope });
      return recStatus();
    }
    case "pause_recording":
      recStatus();
      R.paused = true;
      return;
    case "resume_recording":
      R.paused = false;
      R.last = Date.now();
      return;
    case "add_comment": {
      const t = String(a.text).trim();
      if (!t || t.length > 2000) throw { kind: "invalid_argument", message: "" };
      R.notes++;
      return;
    }
    case "stop_recording": {
      recStatus();
      if (!R.active) throw { kind: "invalid_argument", message: "" };
      await wait(1500);
      const rec = store.get(R.path!)!;
      if (!rec.blocks.some((b) => b.type === "step") && !rec.blocks.some((b) => b.id === "typed-text-notice")) {
        rec.blocks.unshift({
          type: "text", id: "typed-text-notice",
          text: R.keystrokes ? "Typed text was recorded. Review it before sharing." : "Typed text was not recorded. Keyboard shortcuts were.",
        });
      }
      for (let i = 0; i < R.steps; i++) rec.blocks.push(step(i + 1, `Left-click sample item ${i + 1}`));
      const path = R.path!;
      R.active = false;
      R.notice = R.steps ? `Recorded ${R.steps} step${R.steps === 1 ? "" : "s"}.` : "No steps were recorded.";
      if (settings.keepScreenshots !== null && R.steps > settings.keepScreenshots) {
        R.notice += ` Kept screenshots for the newest ${settings.keepScreenshots} steps. Removed the screenshots of ${R.steps - settings.keepScreenshots} older steps.`;
      }
      open = path;
      return { path, recording: structuredClone(rec) };
    }
    case "get_default_library_folder":
      return FOLDER;
    case "list_library":
      return {
        folder: FOLDER,
        recordings: [...store.entries()].map(([path, r]) => ({
          path, title: r.title, created: r.created_unix_ms, updated: r.created_unix_ms,
          stepCount: r.blocks.filter((b) => b.type === "step").length,
        })),
        skipped: [],
      };
    case "create_recording": {
      const title = String(a.title).trim();
      if (!title) throw { kind: "invalid_argument", message: "" };
      const path = `${FOLDER}\\new-${++seq}.bsrec`;
      const rec: Recording = { format_version: 2, id: `n${seq}`, title, created_unix_ms: Date.now(),
        blocks: templateBlocks((a.template as Template | null | undefined) ?? "blank"),
      };
      store.set(path, rec);
      open = path;
      return { path, recording: structuredClone(rec) };
    }
    case "open_recording": {
      const r = store.get(String(a.path));
      if (!r) throw { kind: "not_found", message: "" };
      open = String(a.path);
      return { path: open, recording: structuredClone(r) };
    }
    case "close_recording":
      open = null;
      return;
    case "save_recording":
      store.set(open!, structuredClone(a.recording as Recording));
      return;
    case "rename_recording":
      current().title = String(a.title).trim();
      return;
    case "delete_recording":
      store.delete(String(a.path));
      if (open === a.path) open = null;
      return;
    case "read_screenshot":
      current();
      return placeholder(String(a.path));
    case "export_markdown":
      await wait(800);
      if (String(a.outDir).includes("Full")) throw { kind: "output_not_empty", message: "" };
      return summary(current());
    case "export_pdf":
      await wait(1200);
      return summary(current());
    case "export_single_html":
      await wait(900);
      return summary(current());
  }
  throw { kind: "invalid_argument", message: "" };
}

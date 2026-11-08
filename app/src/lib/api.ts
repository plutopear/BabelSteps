import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  CaptureTarget,
  CaptureTargets,
  ErrorKind,
  ExportChoices,
  ExportSummary,
  LibraryListing,
  OpenedRecording,
  Recording,
  Settings,
  RecordingStatus,
} from "./types";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// The mock is only reachable in dev builds in a plain browser. In production
// `import.meta.env.DEV` is the constant false, so this branch and the dynamic
// import are removed from the bundle.
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) {
    if (import.meta.env.DEV) {
      const m = await import("./mock");
      return (await m.mockInvoke(cmd, args ?? {})) as T;
    }
    throw { kind: "unknown", message: "The BabelSteps backend is not available." };
  }
  return (await invoke(cmd, args)) as T;
}

const KINDS: ErrorKind[] = [
  "no_recording_open", "invalid_argument", "not_found", "storage", "export",
  "busy", "pdf", "too_new", "output_not_empty",
];

/** Normalise whatever invoke rejected with into {kind, message}. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "kind" in e) {
    const o = e as { kind: unknown; message?: unknown };
    const kind = KINDS.includes(o.kind as ErrorKind) ? (o.kind as ErrorKind) : "unknown";
    return { kind, message: typeof o.message === "string" ? o.message : "" };
  }
  return { kind: "unknown", message: "" };
}

/** Plain-language text: what happened and what to do next. */
export function describeError(e: unknown, doing: string): string {
  const { kind } = toAppError(e);
  // Log the kind only, never content.
  console.error("error kind:", kind);
  switch (kind) {
    case "output_not_empty":
      return "That folder already contains an export. Nothing was changed. Choose an empty folder and try again.";
    case "busy":
      return "BabelSteps is busy with a recording or an export. Stop the recording or wait for the export to finish, then try again.";
    case "too_new":
      return "This recording was made by a newer version of BabelSteps. Update BabelSteps to open it.";
    case "not_found":
      return `Could not find the file or folder while ${doing}. It may have been moved or deleted. Go back to the library and refresh.`;
    case "no_recording_open":
      return "No recording is open. Go back to the library and open one.";
    case "invalid_argument":
      return `BabelSteps could not ${doing} with what was given. Check the name or location and try again.`;
    case "pdf":
      return "PDF export failed. Try again, or export to Markdown instead.";
    case "export":
      return "The export failed before it finished. Check that the folder is writable and try again.";
    case "storage":
      return `A file problem stopped BabelSteps from ${doing}. Check that the disk has space and the folder is writable, then try again.`;
    default:
      return `Something went wrong while ${doing}. Try again.`;
  }
}

export const api = {
  defaultFolder: () => call<string>("get_default_library_folder"),
  list: (folder?: string) => call<LibraryListing>("list_library", { folder: folder ?? null }),
  create: (title: string, folder?: string) =>
    call<OpenedRecording>("create_recording", { folder: folder ?? null, title }),
  open: (path: string) => call<OpenedRecording>("open_recording", { path }),
  close: () => call<void>("close_recording"),
  save: (recording: Recording) => call<void>("save_recording", { recording }),
  rename: (title: string) => call<void>("rename_recording", { title }),
  remove: (path: string) => call<void>("delete_recording", { path }),
  screenshot: (path: string) => call<ArrayBuffer>("read_screenshot", { path }),
  exportMarkdown: (outDir: string, choices: ExportChoices) =>
    call<ExportSummary>("export_markdown", { outDir, choices }),
  exportPdf: (outPath: string, headerFooter: boolean, choices: ExportChoices) =>
    call<ExportSummary>("export_pdf", { outPath, headerFooter, choices }),
  recordingStatus: () => call<RecordingStatus>("recording_status"),
  setEditorUnsaved: (unsaved: boolean) => call<void>("set_editor_unsaved", { unsaved }),
  startRecording: (keystrokes: boolean, target: CaptureTarget = { kind: "screen" }) =>
    call<RecordingStatus>("start_recording", { keystrokes, target }),
  listCaptureTargets: () => call<CaptureTargets>("list_capture_targets"),
  pauseRecording: () => call<void>("pause_recording"),
  resumeRecording: () => call<void>("resume_recording"),
  addComment: (text: string) => call<void>("add_comment", { text }),
  stopRecording: () => call<OpenedRecording>("stop_recording"),
  clearNotice: () => call<void>("clear_notice"),
  getSettings: () => call<Settings>("get_settings"),
  /** Saves and applies; returns warnings about shortcuts that clash. */
  saveSettings: (settings: Settings) => call<string[]>("save_settings", { settings }),
};

// File pickers go through the dialog plugin's commands (already allowed:
// dialog:allow-open and dialog:allow-save), so no extra JS package is needed.
export async function pickSavePath(defaultName: string): Promise<string | null> {
  if (!inTauri) {
    if (import.meta.env.DEV) return "C:\\Sample\\" + defaultName;
    return null;
  }
  const r = await invoke<string | null>("plugin:dialog|save", {
    options: { defaultPath: defaultName, filters: [{ name: "PDF document", extensions: ["pdf"] }] },
  });
  return r ?? null;
}

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) {
    if (import.meta.env.DEV) return "C:\\Sample\\Export";
    return null;
  }
  const r = await invoke<string | string[] | null>("plugin:dialog|open", {
    options: { directory: true, multiple: false, title: "Choose an empty folder for the export" },
  });
  return Array.isArray(r) ? (r[0] ?? null) : (r ?? null);
}

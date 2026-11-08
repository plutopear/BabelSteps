// Mirrors core/src/model.rs and app/src-tauri/src/commands.rs (serde shapes).

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type CalloutKind = "note" | "tip" | "warning";

export type Action =
  | { kind: "click"; button: "left" | "right" | "middle"; x: number; y: number }
  | { kind: "text"; text: string }
  | { kind: "shortcut"; keys: string }
  | { kind: "comment" }
  | { kind: "manual" };

export interface Target {
  name: string;
  control_type: string;
  window_title?: string | null;
  process_name: string;
  is_password: boolean;
  bounds: Rect | null;
}

export interface StepBlock {
  type: "step";
  id: string;
  action: Action;
  title: string;
  description: string;
  screenshot: string | null;
  target: Target | null;
  redactions: Rect[];
  captured_unix_ms: number;
  screenshot_withheld: boolean;
}

export type Block =
  | { type: "heading"; id: string; level: number; text: string }
  | { type: "text"; id: string; text: string }
  | StepBlock
  | { type: "list"; id: string; ordered: boolean; items: string[] }
  | { type: "image"; id: string; image: string; caption: string }
  | { type: "callout"; id: string; kind: CalloutKind; text: string };

export interface Recording {
  format_version: number;
  id: string;
  title: string;
  created_unix_ms: number;
  blocks: Block[];
}

export interface RecordingSummary {
  path: string;
  title: string;
  created: number;
  updated: number;
  stepCount: number;
}

export interface LibraryListing {
  folder: string;
  recordings: RecordingSummary[];
  skipped: { path: string; reason: string }[];
}

export interface OpenedRecording {
  path: string;
  recording: Recording;
}

export interface ExportSummary {
  steps: number;
  imagesWritten: number;
  imagesWithRedactions: number;
  withheld: number;
}

export type ErrorKind =
  | "no_recording_open"
  | "invalid_argument"
  | "not_found"
  | "storage"
  | "export"
  | "busy"
  | "pdf"
  | "too_new"
  | "output_not_empty"
  | "unknown";

export interface AppError {
  kind: ErrorKind;
  message: string;
}

export interface HotkeyInfo {
  action: string;
  keys: string;
  registered: boolean;
}

export interface RecordingStatus {
  active: boolean;
  paused: boolean;
  keystrokes: boolean;
  path: string | null;
  steps: number;
  withheld: number;
  warnings: string[];
  noteRequests: number;
  startRequests: number;
  notice: string | null;
  hotkeys: HotkeyInfo[];
}

export interface Settings {
  /** Keep screenshots for the newest N steps when a recording stops; null keeps all. */
  keepScreenshots: number | null;
  sizeWarningMb: number;
}

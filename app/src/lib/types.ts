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

/** Export options (PSR-2, PSR-6). Both are off by default. */
export interface ExportChoices {
  stepTimes: boolean;
  technicalDetails: boolean;
  /** Minutes east of UTC when the recording was made. */
  utcOffsetMinutes: number;
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
  /** What the running recording covers (A6). Label is a monitor name or an app name, never a window title. */
  scope: { kind: "screen" | "monitor" | "window"; label: string };
}

/** What to record (A6, Q13). The whole screen is the default. */
export type CaptureTarget =
  | { kind: "screen" }
  | { kind: "monitor"; device: string }
  | { kind: "window"; id: number };

export interface CaptureTargets {
  monitors: { device: string; label: string }[];
  /** Labels hold window titles: shown in the picker only, never stored or logged. */
  windows: { id: number; label: string }[];
}

export interface Settings {
  /** Keep screenshots for the newest N steps when a recording stops; null keeps all. */
  keepScreenshots: number | null;
  sizeWarningMb: number;
  showCaptureBorder: boolean;
  hotkeys: { startStop: string; pause: string; note: string; stop: string };
  /** Closing the window keeps BabelSteps running in the tray (default false: closing quits). */
  closeToTray: boolean;
  /** A3: name the full window title in click steps. Off by default for privacy. */
  showWindowTitles: boolean;
  /** B14: words hidden from typed text (whole word, any case). At most 100, 200 characters each. */
  customWords: string[];
  /** B14: regular expressions hidden from typed text. Same limits. */
  customPatterns: string[];
}

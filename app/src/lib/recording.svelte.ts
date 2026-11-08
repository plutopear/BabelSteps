// Live recording state, polled from the backend. Recording can also start or
// stop from global hotkeys and the tray, so the UI cannot rely on its own calls.
import { api } from "./api";
import { say } from "./announce.svelte";
import type { RecordingStatus } from "./types";

const IDLE: RecordingStatus = {
  active: false, paused: false, keystrokes: false, path: null, steps: 0,
  withheld: 0, warnings: [], noteRequests: 0, startRequests: 0, notice: null, hotkeys: [],
};

export const rs = $state({
  status: IDLE as RecordingStatus,
  /** True while the Stop button's own call is running (hides the hotkey-stop handling). */
  stopping: false,
  /** Block ids shown in the editor right now, to find the steps a recording adds. */
  knownIds: [] as string[],
});

/** The window was rebuilt by the "Add a note" shortcut (page opened as #note).
 *  Read once here and cleared at the first status poll, so it can only apply
 *  to the recording that was running when the window opened. */
export const openedForNote = { pending: location.hash === "#note" };
if (openedForNote.pending) history.replaceState(null, "", location.pathname + location.search);

export function statusLabel(s: RecordingStatus): string {
  if (!s.active) return "Not recording";
  if (s.paused) return `Paused · Keystrokes ${s.keystrokes ? "on" : "off"}`;
  return `Recording · Keystrokes ${s.keystrokes ? "recording" : "off"}`;
}

export async function refreshStatus(): Promise<RecordingStatus> {
  try {
    const s = await api.recordingStatus();
    const before = statusLabel(rs.status);
    rs.status = s;
    if (!s.active) openedForNote.pending = false;
    const after = statusLabel(s);
    if (before !== after) say(after + ".");
  } catch {
    console.error("error kind: status");
  }
  return rs.status;
}

export function setStatus(s: RecordingStatus) {
  rs.status = s;
  say(statusLabel(s) + ".");
}

/** Poll every 500 ms while recording, every 1500 ms otherwise. Returns a stop function. */
export function startPolling(): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const tick = async () => {
    await refreshStatus();
    if (!stopped) timer = setTimeout(tick, rs.status.active ? 500 : 1500);
  };
  tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}

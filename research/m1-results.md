# M1 results

Measured evidence from M1 (Windows capture parity). Conditions go with every number.

## Recording engine self-test (2026-11-06)

`platform/windows`, `capture-selftest` binary, release build, run through the `capture-selftest` skill. One monitor at 100% scaling, 24 logical CPUs, the owner's daily PC in normal use. All input is synthetic and lands on a private test window; real input is ignored.

**Result: 36 of 36 checks passed.** Each check that could pass vacuously has a control.

| Area | What passed |
| --- | --- |
| Before the click | 20 of 20 screenshots show the window before the click; 10 of 10 with analysis slowed by 400 ms per event. None marked "may show the moment after". Control: the window reacted to every click. |
| Password field | Typed password in no step text and no file on disk (database, journal, images; UTF-8 and UTF-16). Field solid black in all 5 images. Controls: ordinary typed text is recorded; the user name field is not blacked out. |
| Window changes after the click | A click that minimizes the password window: the image has the field black (from the window's remembered scan). Control: the window did minimize. |
| Fail closed | UI Automation failing: every screenshot withheld, no image written, typed text dropped. A hung call (2.5 s against a 1.5 s limit): that step withheld, the next one normal. The window scan alone failing: withheld. |
| Fallback capture | GDI capture is marked "This screenshot may show the moment after the action." |
| Pause, comments, files | Clicks while paused are not recorded; comments become steps; no image without a step and no step without its image. |
| Hooks | 0 events dropped in a 1,000-key burst; callback mean 1.7 µs, max 69 µs. |

Analysis per event: mean about 94 ms, max about 100 ms, scanning 7 windows. Frames held for analysis stayed within the 200 MB budget (0 over budget).

**What changed because of testing and review (the 34/34 first pass hid these):**

- Capture waited for room in a frame cap before grabbing, so under load screenshots showed the wrong moment. It now never waits.
- The full-monitor scan took about 1.5 s per click. Skipping windows fully hidden behind opaque windows brought it to about 80–95 ms.
- The scan used the window list after the click, so a window minimized or closed by the click could be saved unredacted. The window list is now taken at grab time, with per-window memory and withholding when a changed window was never scanned.
- The frame history could label a stale frame "before the click" on a busy (60 Hz) screen. Fixed, with unit tests on a simulated busy screen.

**Not tested:**

- Multiple monitors and mixed DPI scaling.
- Real applications: browsers (Edge, Chrome, Firefox), elevated windows, UAC.
- Sleep, lock, resolution changes and long sessions. The rebuild code exists but has not run.
- A shortcut (Alt+F4) closing a password window during more than 10 s of typing, and a page replacing its password field in place. The design covers both (60 s window memory) but no test exercises them.
- Lower-end hardware. Analysis at 94 ms per event is measured on a fast desktop only.
- The app did not use the engine at the time of this run. It does now (next section), but that path has not run on the real desktop.

## Recorder connected to the app (2026-11-06)

Built, privacy-audited (PASS), not yet run in the real app:

- Commands to start, pause, resume, add a note and stop. While recording, the open recording belongs to the recorder, and save, export and the like refuse with "busy".
- Global hotkeys Ctrl+Alt+Shift+R (start or stop), P (pause), N (note), X (stop now), registered with `RegisterHotKey`. Their key presses are dropped before they can become steps. A conflict with another app is shown in the recording panel.
- A tray icon: grey idle, red recording, amber paused. Its tooltip says whether keystrokes are recorded.
- Closing the window or quitting while recording stops and saves first. If stopping fails, the document is reopened.
- UI: record dialog with keystrokes off by default; recording panel; live status line; autosave 1.5 s after the last edit. A hotkey start with unsaved edits asks the editor to save, then start.

Checked only in the browser preview with sample data, and by build and type checks.

**Known limits:** a hotkey start can miss an edit made in the last moment before it (the editor reports unsaved edits about once a second). A failed save on close is silent. Whether Windows sign-out or shutdown runs the stop-and-save path is not known.

**Still to test (needs the owner's agreement, because it records their real screen):** each hotkey, the tray menu, pause, a note, stop, closing and quitting while recording, and that no hotkey press appears as a step.

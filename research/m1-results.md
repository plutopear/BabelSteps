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

## First live test in the real app (2026-11-07)

Release build, the owner's desktop with their agreement, keystroke capture off, one monitor. Clicks went to a small test window of our own (sent only when it was under the point); the BabelSteps shortcuts were sent as synthetic key presses. The recording was deleted afterwards; its screenshots were not viewed.

| Check | Result |
| --- | --- |
| Record dialog: keystrokes off by default; start from the UI | Pass. Status line "Recording · Keystrokes off" |
| All four shortcuts register | Pass (no conflicts on this PC) |
| Ctrl+Alt+Shift+P pauses and resumes; a click while paused is not recorded | Pass |
| Ctrl+Alt+Shift+N focuses the note box; the note becomes a step | Pass |
| Ctrl+Alt+Shift+R stops; R starts again from the open recording; X stops | Pass |
| Closing the window while recording saves first; the app exits | Pass: the click just before closing is on disk |
| No shortcut press becomes a step | Pass: 0 of 11 steps |
| On disk after relaunch | 11 steps (5 test-window clicks, 1 note, 5 other clicks, most likely the owner's own); 10 images, 0 missing, 0 unreferenced; no window titles; no PNG metadata chunks; 0 withheld |

**Not tested:** the tray icon's look and its menu (Pause, Stop, Quit), because clicking it needs real mouse input on the taskbar; quitting from the tray while recording; Windows sign-out or shutdown while recording; keystroke capture on; real apps (browsers, password fields, elevated windows).

## Tray test (2026-11-07)

Release build; the owner clicked the tray icon while a test recording ran, and confirmed the colours and tooltip.

- Red dot while recording, tooltip "recording. Keystrokes off"; amber when paused: as expected (owner's observation).
- Tray menu Pause/Resume: the recorder paused and resumed (state log).
- Tray menu Quit while recording: saved first, then exited. On relaunch, 5 steps and 5 images, 0 missing, 0 unreferenced.
- **Finding:** clicks on the tray icon itself (owned by the Windows taskbar, not by BabelSteps) were recorded as steps: 3 of 5, one of them naming the BabelSteps tray button. Clicks on the tray menu were not recorded (it belongs to BabelSteps). Steps Recorder leaves its own controls out, so these clicks should be dropped.
- **Fixed the same day:** clicks inside the tray icon's rectangle (from Tauri, updated every 300 ms) and clicks on taskbar elements named "BabelSteps:..." (the icon in the hidden-icons flyout) are dropped before analysis and write nothing. Owner's retest (with an earlier "BabelSteps" prefix): pause, resume and quit from the tray; 0 tray-icon clicks recorded; 1 other taskbar click recorded, most likely the "show hidden icons" arrow (an ordinary Windows control; not confirmed). The prefix was then narrowed to "BabelSteps:" so File Explorer items can never match; that version is not retested live. Unverified: a taskbar that moves or auto-hides within 300 ms of a click.

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

**Known limits:** a hotkey start can miss an edit made in the last moment before it (the editor reports unsaved edits about once a second). A failed stop on close now keeps the window open and shows an error dialog (written 2026-11-08, not compiled or run yet: it needs a Windows build). Whether Windows sign-out or shutdown runs the stop-and-save path is not known.

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

## Browser password fields (2026-11-07)

New tool: `browser-selftest <browser.exe>` (feature `selftest`, release build). It opens our own local test page in a Chromium browser with a fresh temporary profile in app mode, clicks and types only when that window is under the point or focused, records with keystrokes on, and checks the real output files. Recordings and the temporary profiles are deleted afterwards; only a crop of the test page is kept.

Edge 157, one monitor at 100% scaling, the owner's desktop with their agreement each run. Chrome and Firefox are not installed, and the owner chose not to install them (Edge only). Chrome uses the same engine as Edge, so it is probably covered too, but that is a guess.

| Page | What it does | Result |
| --- | --- | --- |
| Plain (password manager off) | Clicks the password field first, before anything else has read the page's accessibility tree (Edge's first answer was wrong in M0); types; user name; button; password again | Pass. 14 of 14 checks on the first run, all checks again on the second |
| Login form (password manager on) | User name, password, Edge's "show password" eye button, more typing after it, Sign in (the form hides, as after a real sign-in), click on the empty page | Pass |

Second run: 27 of 27 checks. Both typed passwords are in no step and no file (searched as UTF-8 and UTF-16). The password field is solid black in every image while it is on the page. Typing is titled "Enter password". Controls passed: the typed user name is recorded, the user name field is not blacked out, and its cyan background sits exactly where the test expects it, so the rectangles line up. 0 withheld; slowest analysis 66 ms. The two kept crops show only the test page, with the field blacked out.

**Not verified:**
- Whether the eye button actually showed the password. The field is black in every image, and the page cannot report the state.
- Whether Edge offered to save the password after Sign in, and what that prompt showed in the last image. Edge draws it outside the field, so redaction does not cover it. The prompt hides the password by default.
- Password fields inside frames, fields that only look like password fields (ordinary text boxes masked by the site's own code: detection cannot see these), display scaling other than 100%, a normal browser window with tabs, Chrome, Firefox.

## 2026-11-08: screen frame (C2) kept out of screenshots

Self-test (release build, one monitor, Windows 11, Windows Graphics Capture, 0 GDI fallbacks): 38 of 38 pass. New check: the same frame without the capture exclusion shows 118,080 frame-coloured pixels at the screen edge in saved screenshots (the control, so the check can fail); with the exclusion there are 0. The next section extends this run.


## 2026-11-08: expanded self-test, 55 of 55 pass

Release build, one monitor, Windows 11, on the owner's daily PC. Every new check has a control that can fail:
- Frame vs screenshots, both capture methods: Graphics Capture and the forced GDI fallback each show 118,080 frame-coloured edge pixels without the capture exclusion and 0 with it.
- Edge clicks: the test window was moved flush to the bottom-right and top-left screen corners. A frame that is not click-through takes all 8 edge points (1 to 4 px) from the window (control); the real frame takes none, all 8 clicks per corner reached the window and were recorded with the window's own bounds, not the frame's. The Windows taskbar covers the bottom screen row when no frame is up, so a no-frame baseline there is only a yardstick.
- Remapped shortcuts (Ctrl+Alt+Shift+F11 and F12, registered for the test): both fire; with the ignore list, 0 shortcut steps and 5 click steps; without it, 2 shortcut steps (control).
- Retention through the finished recording (the same core function the app calls): 5 files before, keep-newest-2 leaves exactly the last 2 screenshots, the other 3 files are gone, the saved document matches and keeps all 5 steps.

**Not covered:** the Settings window in the real webview; the Tauri glue (re-registering shortcuts on save, the stop notice) as one flow; multiple monitors (geometry is unit tested only); Windows 10 (unsupported, Q19). The test registered its shortcuts itself, so it does not prove the app's own start-up path.

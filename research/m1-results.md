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

## 2026-11-08: BLOCKER, the app window does not load its UI on the owner's PC (cleared, see next section)

The owner reports "localhost refused to connect" in the app window, first with a release exe built without `custom-protocol` (a known cause), then again with the corrected build (`--features custom-protocol`, in `target\app-release\release`). Cause unknown; hypotheses: the build did not embed the frontend, or the window still points at devUrl. It was not launched in this session, so nothing here proves the corrected build loads. Untested because of this: the Settings window, the app's wiring (shortcut re-registration, stop notice), the no-socket check on a running app, and the multi-monitor path. The headless self-test (55 of 55) does not touch the app window. PR #2 must not be merged until the UI is confirmed to load.

## 2026-11-08: app window loads its UI (blocker cleared)

Launched once with the owner's OK. Release exe rebuilt from the branch with `--features custom-protocol`; no dev server on port 1420; WebView2 started with a local debugging port (127.0.0.1 only) so the page could be read without screenshots.

- The window loaded `http://tauri.localhost/`, the bundled frontend, not the dev server. The Library screen rendered (headings and buttons present, no "refused" text).
- The Settings screen rendered: screenshot retention, the red recording frame, and the four shortcuts (8 inputs). Cancelled without saving, so the owner's settings were not changed.
- Sockets while running (the app process, its children, and its WebView2 processes): none, except the test's own debugging listener on 127.0.0.1:9333. This is a single idle snapshot, not a recording-and-export session.
- Closing the window ended the process; nothing was left running.
- Build guard (commit 19b70f1): refuses `cargo build --release` without `custom-protocol`, accepts it with the feature.

**Not tested:** changing and saving settings, shortcut re-registration after a save, the stop notice, a recording in the release app, the owner's own click-through. Why the owner saw the error the second time is unknown.

## 2026-11-08: self-test with titles, typing and outline checks, 67 of 67 pass

Release build, one monitor at 100% scaling, our own test window, synthetic input, run with the owner's OK. New section (12 checks):

- **A11, keystrokes off:** typing into the user name field gave exactly one step, `Typed in "User name:"`. The typed text is in no step and no file (UTF-8 and UTF-16). Control: the text was read back from the field.
- **A3, titles:** the Save click reads `Left-click "Save" button in capture-selftest`. A right click on the window background is one `Right-click window at (x, y)` step. The window title is in no step and no file. Those background clicks hit the window element, whose accessible name is the window title, so this check exercised the 2026-11-08 fix (before it, the title would have been in the step). Control: the title was read back from the window.
- **A2, outline and marker:** the Save click's image has the green outline on all four sides, 1 to 3 px outside the button, and none 8 px further out (control). The red ring sits around the click point; another click's image has no ring there (control).

Timings: analysis per event mean 66 ms, max 71 ms (normal); hook callback mean 1.7 us, max 9.2 us.

**Not covered:** real apps (Chromium panes and documents, tab items, taskbar buttons; see the known gap in docs/04 section 2), middle clicks, unnamed controls other than the window, multiple monitors and other scaling.

## 2026-11-08: live test of the real app, 21 of 22 pass

Release exe built with `--features custom-protocol`, including the window-title fix. Owner's desktop with their agreement, one monitor, keystroke capture off. The harness (`.claude/skills/capture-selftest/scripts/live-app-test.ps1`) clicked the app's buttons inside the page through a local WebView2 debugging port. It sent real clicks only to its own small test window, and shortcut keys only when BabelSteps held them. Settings were backed up and restored, and the test recording was deleted without viewing its images. The first attempt stopped at a harness bug before recording anything; the second run is reported here.

| Check | Result |
| --- | --- |
| Settings: keep 2 screenshots, start on Ctrl+Alt+Shift+F9; saved to disk | Pass |
| The new start shortcut is held at once; the old one is released | Pass (checked by trying to register each) |
| Create a recording; record dialog has keystrokes off; start from the UI | Pass |
| Pause button and pause shortcut pause and resume; clicks while paused not recorded | Pass (5 steps for 4 unpaused clicks + 1 note) |
| Note from the panel becomes a step; note shortcut focuses the note box | Pass |
| Stop button stops and returns to the editor | Pass |
| Retention notice after the stop | **Fail in the harness:** checked immediately, while the screen updates its status only every 1.5 s. Retention itself worked (2 of 6 steps kept images). Since fixed: the screen now refreshes when a stop finishes. Not yet confirmed live |
| Remapped shortcut starts a recording; stop-now shortcut stops it | Pass |
| Reset shortcuts: defaults held again, F9 released | Pass |
| On disk: 6 steps, 2 images, 0 unreferenced, 0 missing; the test window's title in no step | Pass |
| Settings restored; nothing left running | Pass |

**Measurements** (fast desktop PC, one monitor, debug port on; label: single samples):
- Launch to the Library screen: 0.65 s (D3 target: under 1 s to the tray; this measures the window, not the tray).
- Idle, window open, after 10 s: private memory 147 MB across 6 processes, of which the app process is 6 MB and WebView2 the rest; CPU 1.2% of one core. **D1 (under 50 MB idle) is not met with the window open.** Not measured: idle with the window closed to the tray.
- Recording, no activity: private memory 345 MB in total, app process 151 MB (frame history on a 5120x1440 screen); CPU 0.6% of one core. D7 (under 250 MB) is met by the app process and not by the total.
- Sockets while recording: none, except the test's own debugging listener.

**Not tested:** stop from the tray menu (needs a real click on the taskbar), the size-warning notice, keystroke capture on, long sessions.

## 2026-11-08: close to the tray (Q21), live test, 24 of 24 pass

Run with the owner's OK on the release build, using `live-tray-test.ps1` in the capture-selftest skill. One monitor, Windows 11. Each timing is a single measurement.

- With the window open and idle: 147 MB private memory across 6 processes (6 MB of that is the app process).
- After closing to the tray and waiting 15 s: 6 MB private memory in 1 process, and 0.3% of one core. The WebView2 processes ended. **D1 is met in tray mode** (target: under 50 MB). The process held no network sockets.
- Passed:
  - The start shortcut began a new recording with the window closed.
  - The note shortcut does nothing when no recording is running. During a recording it rebuilt the window with the cursor in the note box.
  - Closing during a recording stopped and saved it. The clicks, the note and the images were on disk, with nothing orphaned and nothing missing.
  - Stopping and starting again with the window closed made a new recording. This bug was found in review and fixed before this run.
  - The stop-now shortcut worked in a rebuilt window.
  - Choosing "Quit" again made closing end the app.
- Hardening: right-clicking the start-up window and a rebuilt window opened no browser menu. As a control, the same check found the menu on our own test window. The window now reads its WebView2 settings back and closes itself if they did not apply.
- The first run (22 of 23) saw one new window after right-clicking the rebuilt window. Its cause is unknown, because that run did not record window classes. The second run, after the fail-closed change, saw none.
- Not tested:
  - The tray menu's Show item and double-clicking the tray icon, because both need real taskbar clicks. The menu now opens on right-click only.
  - Windows sign-out or shutdown during a recording with the window closed.
  - The fail-closed path itself (it never triggered).
- After this run, the privacy audit found that the refused window could be shown again while its error message was open, and that the window appeared before its hardening was confirmed. Both are fixed: the window is now built hidden and shown only once the hardening is confirmed, and a timeout reports a hardening step that never runs. The audit passed after the fix. **The window-showing change has not been live-tested.** The 24 of 24 run used the build before it. A later finding is noted but not fixed: a late "window destroyed" event can stop Show from bringing an already-open window to the front. It cannot show an unhardened window.
- Quick follow-up (2026-11-08, with the owner's OK, no recording made): on the hidden-until-hardened build, the window appeared 0.66 s after launch, the Library screen loaded at 0.81 s, right-clicking opened no browser menu, and closing quit the app (default setting). 4 of 4.

## 2026-11-08: Screenshot size (PSR-1)

Built in `core/src/screenshot.rs`. Measured in a Linux cloud container (4 cores), not on the owner's PC, using test pages rendered by headless Chromium at 5120x1440: text documents in five columns (docs), a 40-column spreadsheet (table), a settings page with 150 controls (form), the docs page with a noisy photo-like gradient over its left 10% (photo10), and a page whose left half is that gradient (mixed). No real screen content was used. Timings are single runs on that container.

| Screen | Photo-like share | Stored as | Before (fast PNG) | Now | Encode time now | Print-size export |
| --- | --- | --- | --- | --- | --- | --- |
| docs | 0.04 | PNG | 4,249 KB | 1,761 KB | 99 ms | 595 KB |
| table | 0.04 | PNG | 4,462 KB | 991 KB | 71 ms | 949 KB |
| form | 0.07 | PNG | 1,665 KB | 561 KB | 58 ms | 303 KB |
| photo10 | 0.13 | PNG | 5,677 KB | 3,201 KB | 161 ms | 727 KB |
| mixed | 0.49 | JPEG | 10,397 KB | 1,476 KB | 139 ms | 137 KB |

- Choosing the format reads every 4th row and took about 9 ms per frame, on the analysis thread.
- An assumed 100-step mix (30 docs, 25 table, 25 form, 10 photo10, 10 mixed; a guess, not measured usage) went from 431 MB to 135 MB on disk. Its print-size images, which the HTML and PDF exports use, total 56 MB. The PDF itself was not measured. Steps Recorder needs about 40 MB per 100 steps.
- Options measured and not used: JPEG at quality 80 to 95 was larger than the compressed PNG on every UI screen (docs at quality 85: 1,897 KB), and blurs text. Lossless WebP from the `image` crate was larger than PNG on the form page (1,129 KB). Deflate level 6 was 1-15% smaller than level 3 for about twice the time. Quality 80 JPEG text looked softer than 85 at 2x zoom.
- Exports used the fastest PNG setting with an alpha channel; they now use the same compressed RGB PNG, or JPEG for JPEG sources, still re-encoded from pixels with no metadata.
- **Not tested:** real Windows screens (ClearType text may compress worse; M0 measured 1.5-3.8 MB for real fast-PNG frames, in the same range as the test pages); the fastest-PNG fallback when frames queue up (more than 100 MB in flight); encode times on the owner's PC; a real 100-step recording; the PDF size; the capture self-test on this build.

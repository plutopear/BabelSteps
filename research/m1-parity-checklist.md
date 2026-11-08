# M1 parity checklist

The M1 exit test (`docs/05-roadmap.md`): every Steps Recorder behavior and every M1 requirement, with our status and the evidence for it. Built 2026-11-08 from `docs/02-requirements.md`, `research/baseline-and-competitors.md` section 1, the results docs and the code. Each "pass" and "partial" was re-checked against its evidence by a second reviewer, which downgraded four items.

**Rules.** Pass means a recorded test on the real desktop shows it. Code without a test is "not tested". Unit tests alone, for behavior that depends on the real desktop, are "partial". Live results from before the 2026-11-08 hotkey and close-path changes count as partial until re-run on the current build.

**Where it stands (2026-11-08, after PR #13 merged):** Pass 2, Partial 27, Not tested 1, Not built 0, Owner decision 0, Out of scope (M1) 2 (of 32; the counts equal the rows in the Summary table). Since the earlier count: PSR-10 moved from Not tested to Partial (PR #13, lock and sleep handling; simulated messages only); PSR-1 was built and merged (PR #9, Partial until a real 30-step PDF is measured); PSR-2, PSR-3, PSR-5 and PSR-6 were built and merged (PR #11), A9 moved to Partial once Q23 settled it, and PSR-7 moved out of M1 under Q25. M1 is not ready to exit. The remaining items need real clicks on the tray icon (which only the owner can make), or hardware or machine state the owner may not have (a second monitor, an elevated window).

**Close to the tray, 2026-11-08 (Q21; 24 of 24, plus a quick 4 of 4 on the final build; `research/m1-results.md`):** D1 is met in tray mode, at 6 MB and 0.3% of one core with the window closed. D3 was measured warm only, at 0.66 to 0.81 s to the window. The start, note and stop shortcuts work with no window. The window is rebuilt hardened, and it stays hidden until the hardening is confirmed.

**Live app run, 2026-11-08 (21 of 22, `research/m1-results.md`):** start from the UI, the pause button and shortcut, a note from the panel and the note shortcut, the stop button, a remapped start shortcut, stop-now, resetting shortcuts, retention on disk, and sockets while recording all passed on the current build. That evidence supersedes the 2026-11-07 run for A1, A4, A5, C3 and A8. Still open from it: the retention notice on screen (fixed, not yet confirmed live), stopping from the tray, and D1 (147 MB idle with the window open, against a 50 MB target).

## Next actions, by who must act

**Owner clicks, or an owner-approved run:**
- Tray menu Stop, Show and Quit, and a double-click on the tray icon (the menu now opens on right-click).
- See the red frame and the retention notice on screen.
- Pick a non-default library folder (C4).
- Sign out during a recording with the window closed (tray mode).

**Done 2026-11-08 (live app 21 of 22, tray 24 of 24):** start, pause, notes, stop and remapped shortcuts; settings saved and applied; D1 met in tray mode.

**Done 2026-11-08 (self-test, 67 of 67):** A11 (keystrokes off, typing into our test window gives exactly one "Typed in" step and the text is in no file), A2 (outline and click-marker pixels on our test window, with a control), A3 (named left click, right click, unnamed area).

**Owner decisions:** none left open. A9 is settled by Q23 (the PDF export counts; what remains is a PDF from a real recording) and PSR-7 by Q25 (no command line before the first release). PSR-2, PSR-3, PSR-5 and PSR-6 were decided on 2026-11-08 (Q25, Q30) and are merged into `main` (PR #11; Windows build and 211 tests passed). Not yet checked: a real PDF export with the options on, and the counts notice after a live recording. A6 is decided (Q13) and merged (PR #5); its self-test checks passed on Windows.

**Needs hardware or the owner's machine state:** A7 and the monitor half of A6 (a second monitor, Q20), PSR-8 (an elevated window and a UAC prompt, Q17), PSR-10 (a real lock, sleep and wake with the owner present, and a resolution or scaling change).

**Found while building this checklist (2026-11-08):** a click on empty window space, a title bar, a page background or a tab put the window or page title into the step title and `recording.json`, against A3. Fixed in the core the same day (names of those element types are dropped; unit tested). New self-test checks for A11, A2 and A3, including this one, passed on 2026-11-08 (67 of 67, `research/m1-results.md`): typing with keystrokes off, step titles without the window title, the outline and the click marker, on our own test window. That moves the self-test half of A2, A3 and A11 forward; real apps are still untested. Known gap: taskbar buttons and caption text in newer title bars.

**Claude can build without the owner:** nothing left. Merged on 2026-11-08: smaller stored images (PSR-1, PR #9), the opt-in full window title (A3, PR #4; its self-test check `window_title_opt_in` has not run on Windows and the owner has not clicked the Settings option) and the window-or-monitor choice (A6, Q13, PR #5; self-test checks passed on Windows).

## Summary

| ID | Item | Status |
| --- | --- | --- |
| A1 | Start and stop a recording from the app window and from a global hotkey. | Partial |
| A2 | Take a screenshot on every mouse click, outline the clicked element with a green rectangle (as Steps Recorder does) and mark the click point. | Partial |
| A3 | Write a step title per click from the element's name and type plus the app name (not the window title). Right and middle clicks use the same wording. Showing the full title is an opt-in. Fall back to coordinates when there is no name. | Partial |
| A4 | Add a comment at any point during a recording, by hotkey or from the app. | Partial |
| A5 | Pause and resume without ending the recording. | Partial |
| A6 | Record the full screen, or a chosen window or monitor. | Partial |
| A7 | Recording works correctly on several monitors and with mixed display scaling (DPI). | Partial |
| A8 | Screenshot retention is configurable. Ours defaults to unlimited with a size warning (Q12), where Steps Recorder keeps the last 25. | Partial |
| A9 | Output one self-contained file that opens without the app (format under Q2: our own format, no .mht). | Partial |
| A10 | Open and view existing Steps Recorder output (.zip containing one .mht). | Out of scope (M1) |
| A11 | With keystroke capture off, a typing burst still becomes one step ("Typed in User name field") with no text, ended by a focus change, click or shortcut. | Partial |
| B4 | Never record password fields as text. | Pass |
| B12 | Black out every password field in any visible window on every step's screenshot before it is saved, and withhold the screenshot if the check fails or times out. | Partial. A false alarm on 2026-11-08 is explained in the B12 section |
| C1 | Entirely local: no network calls, telemetry, accounts or cloud storage. | Partial |
| C2 | Always-visible recording indicator: a tray icon plus a visible cue (frame) on the capture target, kept out of the screenshots. | Partial |
| C3 | Global hotkeys for start, pause, add note and stop, remappable with conflict warnings; the panic-stop hotkey always works. Defaults are Ctrl+Alt+Shift+R/P/N/X. | Partial |
| C4 | Recordings are stored in a user-chosen folder, with an in-app library to browse, rename and delete. | Partial |
| D1 | Idle in the tray, the app uses under 50 MB of RAM and almost no CPU (M1 exit target). | Pass |
| D2 | While recording, CPU stays under 5% sustained on a typical laptop and no clicks are dropped (M1 exit target). | Partial |
| D3 | Cold start to the tray takes under 1 second (M1 exit target). | Partial |
| D6 | A screenshot is ready within 150 ms of a click and shows the screen just before the click. This is a deliberate difference from Steps Recorder, which shows the screen after the click (Q3). | Partial |
| D7 | Memory while recording stays under 250 MB on a 5120x1440 screen, including in long sessions. | Partial |
| PSR-1 | Image size: Steps Recorder stores full-monitor JPEGs of about 390 KB each, so a recording stays small enough to email. Ours must make a comparable size decision (crop, format, downscaling). | Partial |
| PSR-2 | Each step has a number and a timestamp. | Partial |
| PSR-3 | The output states that typed text and passwords were not recorded (when keystroke capture is off). | Partial |
| PSR-4 | Function keys, modifier keys and shortcuts are shown even when typed text is not. | Partial |
| PSR-5 | The document states how many steps were recorded and how many were missed or had errors. | Partial |
| PSR-6 | An additional-details section lists the operating system, program and UI elements per step. | Partial |
| PSR-7 | Command-line control: start and stop from a script, set the output path and screenshot limit, hide the UI, limit to one process. | Out of scope (M1) |
| PSR-8 | Clicks in elevated (admin) windows and on UAC prompts: what gets recorded, and whether password detection fails closed when UI Automation is denied. | Not tested |
| PSR-9 | Clicks on the recorder's own controls (window, tray icon, menus, hotkeys) are left out of the recording. | Partial |
| PSR-10 | Sleep, lock and screen-resolution or scaling changes during a recording: capture recovers with fresh pictures and no wrong 'before the click' labels. | Partial |

## Details

### A1: Partial

Start and stop a recording from the app window and from a global hotkey.

- **Steps Recorder:** Start/Stop buttons in its window and command line /start and /stop work. The baseline records no global hotkey. Its window UI was not exercised, because it was launched with /gui 0.
- **Evidence:** research/m1-results.md, 'First live test in the real app (2026-11-07)': rows 'Record dialog: keystrokes off by default; start from the UI' Pass, 'All four shortcuts register' Pass, 'Ctrl+Alt+Shift+R stops; R starts again from the open recording; X stops' Pass, 'Closing the window while recording saves first' Pass. 'Tray test (2026-11-07)': tray Quit while recording saved first. Code: app/src-tauri/src/recording.rs, platform/windows/src/hotkeys.rs.
- **Gap:** Stop from the app window (the recording panel's stop button or the tray menu's Stop) is not in any recorded result. Stops were by hotkey, by closing the window, or by tray Quit. The live evidence predates commits 984c562 (remappable hotkeys) and e40609d (close path), both of which changed app/src-tauri/src/recording.rs. m1-results 'app window loads its UI (blocker cleared)' lists 'a recording in the release app' as not tested since. Also untested: sign-out or shutdown while recording, sleep and lock. Tested on one monitor and Windows 11 only (Windows 10 is unsupported, Q19). To close: one owner-approved live run of the current release exe that starts and stops from the panel, from the tray and by hotkey.

### A2: Partial

Take a screenshot on every mouse click, outline the clicked element with a green rectangle (as Steps Recorder does) and mark the click point.

- **Steps Recorder:** One JPEG per step, full monitor at native resolution, with a bright green rectangle around the clicked element and no marker at the click point.
- **Evidence:** One screenshot per click, live on the real desktop: capture_selftest checks 'before-state ({label}): one click step per click' and 'password: no image without a step, no step without its image' (research/m1-results.md 'Recording engine self-test (2026-11-06)'; '2026-11-08: expanded self-test, 55 of 55 pass'). browser_selftest (m1-results 'Browser password fields', 27/27). Outline and marker exist as code only: platform/windows/src/frame.rs Frame::outline (green 0,200,0) and draw_click_marker (red ring), called from platform/windows/src/pipeline.rs redact_and_keep (around lines 813-852). The unit tests in frame.rs cover black_out and PNG only.
- **Found 2026-11-08 (self-test, one monitor):** when the click minimizes its window, the element is looked up after the window is gone, so the step's outline can go round a different element: in the self-test it crossed the hidden password field (66 pixels of the outline green, 0,200,0). Not a leak, because the outline is drawn after the black-out and only overwrites it with green (the `window change` check passed once it allowed the outline colour: 84 of 84). Still wrong-looking. A fix would skip the outline when the window changed between the frame and the lookup; not done.
- **Gap:** No test checks that the green outline is drawn at the element's bounds or that the marker sits at the click point. Close it with a self-test check that samples outline and marker pixels on our own test window, with a control (the usual owner OK for a self-test). By design the outline is skipped when the password signal is not a definite 'no'. Untested: multi-monitor and mixed DPI (A7), elevated windows, sleep and lock. The frame.rs comment calls the ring 'Steps Recorder's click marker', but the baseline says Steps Recorder has no click-point marker.

### A3: Partial

Write a step title per click from the element's name and type plus the app name (not the window title). Right and middle clicks use the same wording. Showing the full title is an opt-in. Fall back to coordinates when there is no name.

- **Steps Recorder:** User left click on "Save (button)" in "Window title", and the same form for right click. Always uses the full window title.
- **Evidence:** Unit test core/src/markdown.rs click_descriptions_use_app_name_never_window_title (app name and no window title; 'Right-click at (5, 6)'; coordinates fallback). Function core/src/model.rs describe_click. Hooks map WM_RBUTTONDOWN and WM_MBUTTONDOWN (platform/windows/src/hooks.rs lines 112-113). Live: m1-results 'First live test (2026-11-07)', on-disk row 'no window titles'. browser_selftest locates steps by their titles. M0: UI Automation names read in real apps (m0-spike-results 'UI Automation in real apps').
- **Opt-in built and merged 2026-11-08 (PR #4):** Settings, Step names, "Show full window titles in steps", off by default. With it on, click steps read `Left-click "Save" button in "Untitled - Notepad"`, with the app in brackets when the title does not name it. Unit tests: core/src/markdown.rs click_descriptions_name_the_window_only_with_the_opt_in, core/src/normalize.rs window_titles_are_dropped_unless_opted_in opted_in_window_titles_are_scrubbed and opted_in_window_titles_fail_closed_near_passwords_and_skip_typing (no title near password signals or on typing steps). New self-test check window_title_opt_in, not yet run on Windows. The owner has not clicked the Settings option.
- **Gap:** No live check asserts the exact title for a named element, the coordinates fallback, or right or middle clicks (a context menu opens on press, so the before-click picture matters most there). Elevated windows, where names may be unreadable, are untested. To close: add self-test checks on our test window for a named left click, a right click and an unnamed area, and decide whether the opt-in ships in M1.

### A4: Partial

Add a comment at any point during a recording, by hotkey or from the app.

- **Steps Recorder:** Add Comment is in its window UI, but the baseline did not verify it (launched with /gui 0).
- **Evidence:** capture_selftest check 'comment: a comment becomes a step' (m1-results 'Recording engine self-test', row 'Pause, comments, files'; still in the 55/55 run on 2026-11-08). Live app: m1-results 'First live test (2026-11-07)', row 'Ctrl+Alt+Shift+N focuses the note box; the note becomes a step' Pass. Unit test core/src/normalize.rs comment_text_is_scrubbed.
- **Gap:** Downgraded from pass. The live app evidence predates the hotkey rework (commit 984c562 changed recording.rs and hotkeys.rs). m1-results 2026-11-08 says the self-test 'does not prove the app's own start-up path' for shortcuts. A note typed from the panel without the hotkey is not separately recorded. Steps Recorder's comment behavior is unverified, so the detail of parity is unknown; the psr-baseline skill with its GUI could check it. To close: the owner-approved live run of the current release exe.

### A5: Partial

Pause and resume without ending the recording.

- **Steps Recorder:** Pause Record is in its window UI, but the baseline did not verify it.
- **Evidence:** capture_selftest checks 'pause: clicks while paused are not recorded' and 'pause: no image from a paused click is on disk' (m1-results 'Recording engine self-test'; 55/55 run). Live app: m1-results 'First live test (2026-11-07)', row 'Ctrl+Alt+Shift+P pauses and resumes; a click while paused is not recorded' Pass. 'Tray test (2026-11-07)': tray menu Pause/Resume worked (state log); the owner saw the amber icon. Unit tests core/src/normalize.rs nothing_typed_while_paused_reaches_a_step, pause_flushes_the_current_run_first.
- **Gap:** Downgraded from pass. The live app evidence predates the hotkey and recording.rs changes (commits 984c562, e40609d). Pausing from the panel button is not separately recorded. Typing while paused is covered only by unit tests: every live pause case used keystrokes off and only clicks. Steps Recorder's pause behavior is unverified. To close: the owner-approved live run of the current release exe.

### A6: Partial

Record the full screen, or a chosen window or monitor.

- **Steps Recorder:** Every step image was the full monitor at native resolution. Unverified whether its UI can limit capture to one window; second-monitor behavior unverified.
- **Evidence:** The full-screen default works: capture_selftest images are full monitor frames (m1-results 'Recording engine self-test'). Q13 (docs/06-open-questions.md): full screen by default, with a choice of window or monitor. (Before PR #5 no scope option existed; it is now built, see the next line.)
- **Built 2026-11-08, merged (PR #5):** the Record dialog offers "The whole screen" (default), each monitor when there is more than one, and each open window. Hotkey starts record the whole screen. Clicks and typing outside the choice are not recorded. A window's screenshots are cut to the window (plus the menu or dialog clicked, with the gap between them painted grey) before the password scan. Code: `platform/windows/src/scope.rs`, `pipeline.rs` `handle_raw`, `Frame::crop` and `keep_only`. New self-test `scopes` (8 checks, one skipped on a single monitor): the window cut holds the test window's client area and is not much bigger, the password field is black and the user name field is not in the cut images, the click marker lands on the click, a click on another window makes no step, a monitor choice gives full-monitor images, a click outside a chosen monitor makes no step (needs a second monitor), and a disconnected monitor is refused. Unit tests: `Rect::intersect`/`union` (core), `Frame::crop`, `Frame::keep_only`. A privacy review on 2026-11-08 found that a File Explorer target let desktop and taskbar clicks in with a whole-monitor cut; fixed before the first run. First Windows run (2026-11-08, one 5120x1440 monitor at 100%): all 7 scope checks passed (the window cut was exactly the window's 506x293; the second-monitor control was skipped). The run as a whole was 70 of 74: the remapped-hotkey check recorded one shortcut step that should have been ignored, and the three retention checks that expect exactly five screenshots failed with it. That code is unchanged by A6. Main then passed 67 of 67, and two more runs of the A6 branch passed 74 of 74 each, so it was most likely a one-off timing glitch; it is not ruled out as a rare bug, and the log gave only counts, not which shortcut slipped through.
- **Gap:** The out-of-scope monitor case needs a second monitor (Q20). Windows lying on top of the chosen window still show inside its cut. A window spanning two monitors is cut to the part on the clicked monitor. A second window of the same app with no title bar counts as part of the chosen window (the rule that lets its menus and drop-downs in). Real apps (Edge, Office, store apps) are untested.

### A7: Partial

Recording works correctly on several monitors and with mixed display scaling (DPI).

- **Steps Recorder:** Full monitor at native resolution on one monitor (5120x1440). Second-monitor behavior is listed as unverified in baseline section 1.
- **Evidence:** Unit tests only: platform/windows/src/frame.rs black_out_uses_screen_coordinates_and_clips (monitor at negative x); platform/windows/src/indicator.rs thickness_scales_with_dpi_and_never_drops_below_four and frame-hole geometry tests. Code: capture.rs grab picks the monitor containing the point and refresh() rebuilds when the monitor set changes; monitors.rs per-monitor DPI awareness; capture_selftest check 'setup: per-monitor DPI aware'. Q20: do not claim A7 until it is tested on hardware. m1-results 'Not tested' lists multiple monitors and mixed DPI; every live run was one monitor at 100%.
- **Gap:** Needs the owner and a second display. Run capture-selftest and a real-app recording with the second monitor to the left of or above the primary (so coordinates go negative) and different scaling (for example 100% and 150%). Check clicks on each monitor, a window dragged across the boundary, the before-click picture, the password blackout lining up on the secondary monitor, and a frame on every monitor. Scaling other than 100% on one monitor is also untested; the owner can change it in Settings. A psr-baseline run on two monitors would establish what Steps Recorder does.

### A8: Partial

Screenshot retention is configurable. Ours defaults to unlimited with a size warning (Q12), where Steps Recorder keeps the last 25.

- **Steps Recorder:** The default kept only the last 25 screenshots of a longer session; /maxsc 100 raised it.
- **Evidence:** Live on the real desktop: m1-results '2026-11-08: expanded self-test, 55 of 55 pass', 4th bullet, with capture_selftest checks 'retention control: five screenshots on disk before', 'retention: keep-newest-2 leaves exactly the last two screenshots, the other three files are gone' and 'retention: the saved document matches and keeps all five steps'. Unit tests: core/src/model.rs keeps_only_the_newest_screenshots, under_the_limit_changes_nothing_and_zero_removes_all, a_shared_image_is_not_removed; core/src/storage.rs retention_removes_old_files_from_disk_and_keeps_the_document_consistent; core/src/settings.rs defaults_keep_everything, validate_rejects_zero_and_huge. The Settings screen rendered the retention field (m1-results 'app window loads its UI'). Decision Q12.
- **Gap:** Not tested: changing and saving the limit in the real Settings window, then stopping a recording in the app (app/src-tauri/src/recording.rs apply_retention) and checking the files and the notice. The unlimited-mode size warning (recording.rs lines 228-229, dir_size, DEFAULT_SIZE_WARNING_MB 500) has no test; a unit test with a small threshold would cover it. Not compared: whether Steps Recorder keeps the text of steps older than the limit (ours keeps the step and removes only the image); the psr-baseline skill can check this without the owner. The Settings round trip needs the owner's OK for a live app run.

### A9: Partial

Output one self-contained file that opens without the app (format under Q2: our own format, no .mht).

- **Steps Recorder:** A .zip holding one Recording_YYYYMMDD_HHMM.mht: a main HTML page, 62 fragments, a stylesheet and one JPEG per step. 31 steps were 16.9 MB as MHTML and 10.3 MB zipped.
- **Evidence:** docs/05-roadmap.md M1 status: 'Not built yet: the single-file output decision (A9, Q2)'. Q2 (docs/06-open-questions.md): our own format, no .mht compatibility. The PDF export is one file that opens without the app: research/m2-results.md 'D5: PDF export speed' (headless self-test) and 'Editor UI in the real app (2026-11-06)' (2-page PDF export). The native recording is a bundle folder (core/src/storage.rs closed_bundle_is_a_single_db_file plus the images folder); Markdown export is a folder. Single-file HTML is B10 (P2).
- **Decided 2026-11-08 (Q23):** the PDF export satisfies A9 for M1; single-file HTML stays a later extra (B10).
- **Gap:** What remains is a PDF exported from a real recording: all PDF exports so far used synthetic images (m2-results 'Not tested: Real screenshots'). PDF size is part of this decision (see PSR-1).

### A10: Out of scope (M1)

Open and view existing Steps Recorder output (.zip containing one .mht).

- **Steps Recorder:** Steps Recorder writes a .zip with one .mht. Whether other tools can read it is an open research task (baseline section 3).
- **Evidence:** docs/02-requirements.md A10 is P2. docs/06-open-questions.md Q2: no .mht compatibility or import for now. docs/05-roadmap.md lists it under 'Later'. No import code in core/ or app/.
- **Gap:** Not built, by decision. It would need an MHTML parser in core and sample files made by Steps Recorder (the psr-baseline skill can make them).

### A11: Partial

With keystroke capture off, a typing burst still becomes one step ("Typed in User name field") with no text, ended by a focus change, click or shortcut.

- **Steps Recorder:** A typing burst becomes one step, User keyboard input on "User name: (edit)" in "Window" [...]. Typed strings appear nowhere in the file.
- **Evidence:** Unit tests in core/src/normalize.rs: keystrokes_off_never_buffers (title 'Typed in "Box"', empty text), keystrokes_off_emits_only_shortcuts_with_ctrl_alt_or_meta, keystrokes_off_still_names_password_fields_and_withholds, run_ends_on_focus_change_click_shortcut_and_finish, enter_and_tab_end_the_run_and_get_their_own_step. Live typing exists only with keystrokes on: capture_selftest start(dir, "password", true) (capture_selftest.rs line 224) and browser_selftest (keystrokes_enabled: true, line 422).
- **Gap:** Every keystrokes-off case in capture_selftest (lines 165, 313, 391, 415, 433, 451, 481, 516, 592) sends clicks or F24, never ordinary typing. The 2026-11-07 live app test ran with keystrokes off but typed into no field. To close: a capture_selftest case with keystrokes off that types into our test window's user name field, then checks for exactly one 'Typed in ...' step and that the string is in no file (UTF-8 and UTF-16). It needs only the usual self-test OK. Typing in real apps other than Edge is untested.

### B4: Pass

Never record password fields as text.

- **Steps Recorder:** Never records any typed text. The file states that passwords and other typed text were not recorded.
- **Evidence:** Live on the real desktop, keystrokes on: capture_selftest checks 'password: typed password is in no step text', 'password: typed password is in no file on disk' (UTF-8 and UTF-16), control 'password control: ordinary typed text is recorded', 'password: the password run is titled "Enter password"', and 'fail-closed: typed text was dropped' (m1-results 'Recording engine self-test', row 'Password field' and 'Fail closed'; in the 55/55 run on 2026-11-08). browser_selftest on Edge 157, plain and sign-in pages, 27/27: both typed passwords in no step and no file (m1-results 'Browser password fields (2026-11-07)'). Unit tests in core/src/normalize.rs: password_run_keeps_no_text, unknown_signal_mid_run_erases_everything, typing_abc_then_a_mid_run_yes_leaves_no_abc. No capture or redaction code has changed since 2026-11-07 except the tray-click fix (git log).
- **Gap:** Pass only under the tested conditions: our test window and Edge, one monitor, 100% scaling. Untested: Chrome and Firefox (the owner chose Edge only), password fields inside frames, fake masked text boxes (detection cannot see these), elevated windows, Alt+F4 closing a password window during long typing, a field replaced in place. Tested at engine level, not through the current release app.

### B12: Partial

**False alarm, 2026-11-08 (self-test fixed in PR #17):** the check 'window change: a window minimized by the click never leaves its password field visible' failed 3 of 3 runs on the owner's PC with 'IMAGE WITH THE FIELD VISIBLE'. The confirming run showed that the 66 non-black pixels over the hidden field were exactly the recorder's own element outline colour (0,200,0), drawn after the black-out; the check only allowed the red click marker. After the test change, the self-test passes 84 of 84. So B12 is not affected by this. Side finding for A2 (cosmetic, not fixed): when the click minimizes its window, the element is looked up after the window is gone, so the outline can go round a different element.

Black out every password field in any visible window on every step's screenshot before it is saved, and withhold the screenshot if the check fails or times out.

- **Steps Recorder:** Not in baseline. Steps Recorder does not redact password fields in screenshots.
- **Evidence:** capture_selftest checks: 'password: the password field is solid black in every image' with control 'password control: the user name field is not blacked out'; 'window change: a window minimized by the click never leaves its password field visible' with control; 'fail-closed: every step's screenshot is withheld' and 'fail-closed: no image file was written'; 'timeout: a hung UI Automation call withholds that step'; 'scan failure: the window scan alone failing withholds the screenshot' with control (m1-results 'Recording engine self-test', 36/36, and the 55/55 run). browser_selftest on Edge: field solid black in every image, and the user name field's cyan sits where expected (m1-results 'Browser password fields'). Unit tests in platform/windows/src/scan.rs, for example failed_scan_withholds_everything.
- **Gap:** Shown only on one monitor at 100% scaling, on our test window and an app-mode Edge window. Untested: multiple monitors and mixed DPI, elevated windows (scan behavior unknown, Q17), Chrome and Firefox, fields in frames, a normal tabbed Edge window, sleep, lock and resolution change, Edge's save-password prompt (drawn outside the field), whether 'show password' took effect, a field replaced in place. Multi-monitor needs the owner's hardware; the elevated test needs the owner to approve a UAC prompt.

### C1: Partial

Entirely local: no network calls, telemetry, accounts or cloud storage.

- **Steps Recorder:** Not in baseline. Steps Recorder is local, but its network behavior was not measured.
- **Evidence:** Static: .claude/skills/babelsteps-check/scripts/no-network.ps1 (dependency tree for all three OS targets, Tauri plugins, tokio net, capabilities, remote URLs). docs/04-privacy-and-security.md 'Guards' records it passing 2026-11-08 with 322 crates. app/src-tauri/tauri.conf.json CSP connect-src is limited to ipc. Runtime: m1-results 'app window loads its UI (blocker cleared)': one idle snapshot of the running release app showed no sockets except the test's own 127.0.0.1 debugging port.
- **Gap:** The runtime test the privacy doc promises (item 1: fail if the app opens a socket during a recording-and-export session) does not exist; there is only one idle snapshot. To close: watch sockets during a full recording plus PDF and Markdown export in the release app (needs the owner's OK, because it records the screen). Which local pipes WebView2 uses is unchecked.

### C2: Partial

Always-visible recording indicator: a tray icon plus a visible cue (frame) on the capture target, kept out of the screenshots.

- **Steps Recorder:** Not in baseline. Steps Recorder's indicator was not measured (launched with /gui 0).
- **Evidence:** Tray: m1-results 'Tray test (2026-11-07)': the owner saw red while recording and amber when paused, and the tooltip says whether keystrokes are recorded. Frame: m1-results '2026-11-08: screen frame (C2) kept out of screenshots' (38/38) and 'expanded self-test, 55 of 55 pass'. Checks 'frame cue: with the exclusion, no frame pixels in any screenshot' (0 pixels) and 'frame cue control: ...' (118,080 pixels), for both Graphics Capture and the forced GDI fallback. Edge clicks pass through the frame at all 8 points per corner. Unit tests in platform/windows/src/indicator.rs.
- **Gap:** The frame has not been shown in a real app recording, and the owner has not confirmed seeing it; the self-test drew the frame itself. The tray evidence predates the 11-08 changes to recording.rs. Multi-monitor frames are unit tested only (Q20). Windows 10 is unsupported (Q19), so the path where Windows refuses the exclusion is untested, and a refusal after monitors change mid-recording shows no warning (Q19). Needs an owner-approved recording in the release app.

### C3: Partial

Global hotkeys for start, pause, add note and stop, remappable with conflict warnings; the panic-stop hotkey always works. Defaults are Ctrl+Alt+Shift+R/P/N/X.

- **Steps Recorder:** Not in baseline. Steps Recorder's command line /start and /stop worked; its pause and settings UI is unverified.
- **Evidence:** Defaults, live in the app: m1-results 'First live test (2026-11-07)': all four register; P, N, R and X work; 0 of 11 steps are shortcut presses. Remapping, live at engine level: m1-results '2026-11-08: expanded self-test, 55 of 55 pass', 3rd bullet, with checks 'remapped hotkeys: all four shortcuts registered', 'remapped hotkeys: the remapped note and stop shortcuts fire', 'remapped hotkeys: with them ignored, no step records either shortcut' and control 'remapped hotkeys control: without the ignore list the same shortcuts ARE recorded as steps'. Unit tests: core/src/settings.rs hotkeys_parse_normalize_and_reject, bad_hotkeys_in_the_file_fall_back_to_defaults; platform/windows/src/hotkeys.rs settings_text_becomes_virtual_keys.
- **Gap:** The default-hotkey live test predates the remappable-hotkey code (commit 984c562). m1-results 2026-11-08 says the self-test 'does not prove the app's own start-up path'. Not tested: remapping through the Settings window and re-registration after save; the conflict warning (no conflicts existed on this PC; a helper process could pre-register a shortcut); panic stop under load or with the app window hung. Needs a live app run with the owner's OK.

### C4: Partial

Recordings are stored in a user-chosen folder, with an in-app library to browse, rename and delete.

- **Steps Recorder:** Steps Recorder saves one .zip wherever the user chooses (or /output on the command line). It has no library.
- **Evidence:** Unit tests in core/src/storage.rs: library_lists_and_skips_unreadable, rename_changes_title_only, delete_refuses_ordinary_folders, create_makes_layout_and_dedups, roundtrip_through_fresh_handle, open_refuses_non_bundles. Live (debug build, page-level input): research/m2-results.md 'Editor UI in the real app (2026-11-06)': the library lists and opens a recording; delete asks first, says it cannot be undone and removes the folder. m1-results 'First live test' and 'Tray test': saved recordings reopen with 0 missing and 0 unreferenced images.
- **Gap:** Not tested live: choosing a non-default folder through the native folder picker (m2-results 'Not tested: the native file pickers'), and rename in the UI (unit test only). To close: a live run in the release app that picks a folder, records, renames and deletes (the recording part needs the owner's OK).

### D1: Pass

Idle in the tray, the app uses under 50 MB of RAM and almost no CPU (M1 exit target).

- **Steps Recorder:** Not in baseline. Steps Recorder's memory and CPU were not measured.
- **Evidence:** research/m1-results.md, "close to the tray (Q21), live test, 24 of 24". The release app was set to "Keep running in the tray" with the window closed. After 15 s idle it used 6 MB of private memory in 1 process, 28 MB working set, and 0.3% of one core over 5 s. The WebView2 processes had ended. With the window open it uses 147 MB, and no target covers that state (D1 note in docs/02-requirements.md).
- **Gap:** Single measurements on one fast PC. There is no laptop figure, and no long idle period (minutes) was sampled.

### D2: Partial

While recording, CPU stays under 5% sustained on a typical laptop and no clicks are dropped (M1 exit target).

- **Steps Recorder:** Not in baseline for CPU. Its summary line reports 'Missed Steps: 0' for a 31-step recording.
- **Evidence:** Dropped input, live on the real desktop: capture_selftest checks 'hooks: no input event was dropped' and 'hooks: the burst reached the hooks' (0 of 1,000 dropped; hook callback mean 1.7 microseconds, max 69 microseconds, per m1-results 'Recording engine self-test' row 'Hooks'). 'before-state ({label}): one click step per click' passed 20/20 and 10/10 with analysis slowed by 400 ms. All 8 edge clicks per corner recorded (55/55 run). Analysis took about 94 ms mean and 100 ms max per event, off the capture path (m1-results 'Recording engine self-test'; 66 ms max in Edge). CPU: only the M0 spike, 0.4-0.8% of one core idle and 2.0-3.5% at 60 Hz repaint (m0-spike-results 'Measured results').
- **Gap:** CPU has never been measured for the M1 engine or the real app while recording, so WebView2, the tray and the analysis thread are not counted. There is no laptop figure; m1-results says lower-end hardware is untested. The click checks used slow synthetic clicks on one monitor. Untested: fast real click bursts, many open windows, multi-monitor, elevated windows, long sessions. Needed: a CPU sample of the release app during a recording (owner's OK) and a laptop run (hardware).

### D3: Partial

Cold start to the tray takes under 1 second (M1 exit target).

- **Steps Recorder:** Not in baseline.
- **Evidence:** Warm launches of the release build on 2026-11-08 (research/m1-results.md): 0.65 s to the Library screen in the live app test. In the quick check on the final build, the window was visible at 0.66 s and the Library screen at 0.81 s. These are single runs.
- **Gap:** Time from process start until the tray icon is registered and the window is ready, in the release build, over several launches, with cold (first after boot) separated from warm. Needs the owner's OK; a true cold start needs a reboot, which the owner would notice. Guess: WebView2 start-up is the main risk.

### D6: Partial

A screenshot is ready within 150 ms of a click and shows the screen just before the click. This is a deliberate difference from Steps Recorder, which shows the screen after the click (Q3).

- **Steps Recorder:** The image shows the state after the click: 21 of 21 clicks on a window that reacts on press showed the reacted state.
- **Evidence:** Before-state, live on the real desktop: capture_selftest checks 'before-state ({label}): ...' with control 'before-state control ({label}): the window reacted to every click' (capture_selftest.rs around lines 195-202). 20 of 20, and 10 of 10 with analysis slowed by 400 ms (m1-results 'Recording engine self-test', row 'Before the click'). Check 'fallback capture: a screenshot that cannot be proven to predate the click is marked'. Unit tests in platform/windows/src/capture.rs: frame_just_before_the_click_is_chosen, busy_screen_keeps_advancing_and_reaches_back, overwritten_pre_click_frame_is_not_verified. Latency: M0 spike only, p50 9.4 ms and p95 11.2 ms (m0-spike-results 'Measured results').
- **Gap:** The before-state half is demonstrated, but only on one monitor at 100% scaling, on our synthetic window. The M1 engine reports no click-to-pixels latency (no latency figure in pipeline.rs, capture.rs or capture_selftest.rs), so the 150 ms target is backed only by the old spike code. Untested: multi-monitor, mixed DPI, elevated windows, after sleep, lock or resolution change (the rebuild code has never run), real apps other than our window, lower-end hardware. GDI fallback frames are labeled 'may show the moment after', not guaranteed to be before. To close: add a latency figure to the self-test (needs the owner's OK to run).

### D7: Partial

Memory while recording stays under 250 MB on a 5120x1440 screen, including in long sessions.

- **Steps Recorder:** Not in baseline. A 31-step Steps Recorder recording completed with 'Missed Steps: 0'; no step cap beyond the screenshot limit was observed.
- **Evidence:** Only proxies: research/m0-spike-results.md, spike peak about 160 MB; research/m1-results.md 'Recording engine self-test': frames held for analysis stayed within the 200 MB budget (0 over budget). docs/03-architecture.md sets the 200 MB frame budget. m1-results lists long sessions as not tested. Live app test 2026-11-08 (research/m1-results.md): during a short recording with the window open, the app process used 151 MB of private memory, and 345 MB including the WebView2 processes. That is a single sample on the owner's screen. Long sessions are untested, and it is open whether the 250 MB target counts WebView2.
- **Gap:** Downgraded from partial. Neither the M1 engine nor the app (with WebView2) has had its whole-process memory measured while recording; staying inside the frame budget is not a memory measurement. Guess: the frame budget alone could reach 200 MB under load, so 250 MB is at risk once the UI is added. No test is longer than a few dozen steps. Needed: working-set samples during a release-app recording, plus a run of 100+ steps or 30+ minutes that checks memory, the frame budget, and the step and image counts on disk (owner's OK and an idle PC).

### PSR-1: Partial

Merged into `main` with PR #9 (2026-11-08).

Image size: Steps Recorder stores full-monitor JPEGs of about 390 KB each, so a recording stays small enough to email. Ours must make a comparable size decision (crop, format, downscaling).

- **Steps Recorder:** Full monitor at native 5120x1440, about 390 KB per JPEG. 31 steps were 16.9 MB as MHTML and 10.3 MB zipped.
- **Evidence:** Built 2026-11-08 (core/src/screenshot.rs, used by platform/windows/src/frame.rs and pipeline.rs, and by core/src/image_export.rs for exports). Full monitor, full resolution; UI screens are stored as tightly compressed lossless PNG, and screens that are largely photo-like as JPEG quality 85 with full colour resolution. Measured on synthetic 5120x1440 screenshots (research/m1-results.md 'Screenshot size (PSR-1)'): UI screens 2.4 to 4.5 times smaller than before (0.5 to 1.7 MB each), a photo-heavy screen 7 times smaller (10.2 MB to 1.4 MB); an assumed 100-step mix went from 431 MB to 135 MB on disk, and its print-size images (HTML and PDF input) total 56 MB. Unit tests cover the format choice, lossless PNG, no metadata in either format, and redaction in JPEG exports. Target set by the owner on 2026-11-08: a typical 30-step recording under 10 MB. On the owner's PC, WebView2 re-compresses every PDF image itself, so image formats do not change the PDF size (research/m1-results.md 'PDF size is set by WebView2').
- **Gap:** Not measured on a real recording: the numbers come from Chromium-rendered test pages, and real Windows text (ClearType) may compress worse. Still about 3.5 times Steps Recorder per image, because text is kept pixel-exact; going further means palette PNG or smaller exports, which trade readability. Not tested live: the fastest-PNG fallback when screenshots queue up, the format choice in the capture self-test, and the PDF size. First real recording (8 File Explorer steps at 5120x1440, 2026-11-08): 672 KB per stored screenshot and 0.37 MB of PDF per step, so about 11 MB per 30 steps, just over the 10 MB target (an estimate). The PDF self-test still passes D5 (8.7-9.4 s). The owner chose smaller PDF pictures on 2026-11-08: print images wider than 2000 px are now shrunk to 1600 px (narrower ones are left as they are), expected to give about 7 MB per 30 steps (an estimate, not yet measured on a real recording).

### PSR-2: Partial

Each step has a number and a timestamp.

- **Steps Recorder:** Each step has a number and timestamp.
- **Evidence:** Numbers: core/src/normalize.rs steps_keep_their_order_and_get_sequential_ids; core/src/html.rs numbering_matches_markdown_and_withheld_has_no_image. Time: StepBlock.captured_unix_ms is stored (core/src/model.rs), but in core/src/markdown.rs, html.rs and export.rs it appears only in test fixtures.
- **Gap:** Was: capture time stored but shown nowhere. Now built and merged (PR #11), decided in Q30. Partial because only unit tests and the Windows build (211 tests) cover it: no real PDF export with the time option on, and no look at the app's editor, has been recorded. The Evidence line above describes the code before PR #11.
- **Built 2026-11-08, merged (PR #11):** the editor shows each step's local time; the export dialog option "The time of each step" (off by default) adds `Time: HH:MM:SS` under each step and the date once at the top (core/src/export_options.rs; tests in core/tests/export_options_integration.rs). Times use the UTC offset at the first step, so a recording that crosses a daylight-saving change is off by an hour after it (guess: rare). Markdown import drops the time line.

### PSR-3: Partial

The output states that typed text and passwords were not recorded (when keystroke capture is off).

- **Steps Recorder:** The file says passwords and any other typed text were not recorded, except function and shortcut keys.
- **Evidence:** No such notice in core/src/markdown.rs, html.rs or export.rs (grep for 'not recorded' and 'keystroke' found nothing). The panel and tray tooltip show keystrokes on or off while recording (m1-results 'Tray test').
- **Gap:** Was: no such notice. Now built and merged (PR #11), decided in Q30. Partial: unit tests only; a real export of a real recording with the line in it has not been checked. The notice must state only what was not captured and never claim the export is free of sensitive data (product rule). The Evidence line above describes the code before PR #11.
- **Built 2026-11-08, merged (PR #11):** a new recording starts with one ordinary text block, "Typed text was not recorded. Keyboard shortcuts were." or "Typed text was recorded. Review it before sharing." (core/src/notice.rs). The user can edit or delete it and it stays that way; an unedited "not recorded" line becomes "recorded" if a later session records typed text. Older recordings that already have steps get no line.

### PSR-4: Partial

Function keys, modifier keys and shortcuts are shown even when typed text is not.

- **Steps Recorder:** Modifier keys are shown, for example [Ctrl-Ctrl] and [... Win]. The file says function and shortcut keys are recorded. How it words a shortcut such as Ctrl+A was not established.
- **Evidence:** Unit tests: core/src/normalize.rs named_shortcuts, window_shortcuts_and_general_format, keystrokes_off_emits_only_shortcuts_with_ctrl_alt_or_meta, bare_escape_and_named_keys_are_presses_that_end_the_run; platform/windows/src/keys.rs ctrl_s_is_a_char_with_ctrl, altgr_fake_ctrl_is_not_reported_as_ctrl, named_and_fixed_keys. Live checks cover only that our own hotkeys and F24 make no step (capture_selftest 'hooks: F24 with keystrokes off makes no step', the remapped-hotkey checks, and 0 of 11 in the 2026-11-07 live test).
- **Gap:** No live test sends an ordinary shortcut (for example Ctrl+S) through the real hooks and checks its step. By design, bare function keys and Enter or Tab make no step when keystrokes are off, unlike Steps Recorder; confirm that difference is acceptable. Dead keys are recorded as the plain letter (known). To close: a capture_selftest case on our test window (usual OK).

### PSR-5: Partial

The document states how many steps were recorded and how many were missed or had errors.

- **Steps Recorder:** Summary line: 'Recorded Steps: 31, Missed Steps: 0, Other Errors: 0'.
- **Evidence:** Steps carry screenshot_withheld, and exports leave out withheld images (core/src/markdown.rs withheld_step_never_links_its_image; core/src/html.rs numbering_matches_markdown_and_withheld_has_no_image). Pipeline status counts withheld steps and UI Automation timeouts (read by capture_selftest). Grep for a recorded or missed summary in core/src/html.rs and markdown.rs found none.
- **Gap:** Was: no total reached the user. Now built and merged (PR #11), decided in Q30. Partial: the counts notice after a live recording has not been checked, and a real PDF export with the technical-details section on has not been checked. The Evidence line above describes the code before PR #11.
- **Built 2026-11-08, merged (PR #11):** after every stop the app notice starts with the counts, for example "Recorded 12 steps. 1 screenshot was withheld ... 2 steps have no screenshot. 1 click or key press was missed." Missed inputs come from the hook queue for that session only. Exports show counts only in the technical details.

### PSR-6: Partial

An additional-details section lists the operating system, program and UI elements per step.

- **Steps Recorder:** Fields: Recording Session, Recorded Steps, Operating System, Program, UI Elements.
- **Evidence:** research/baseline-and-competitors.md section 1, 'Additional details fields'. Q2 (same workflow, own format). A3 names the app rather than the window title, for privacy. No OS or session-details section in core exporters (grep).
- **Gap:** Was: not built. Now built and merged (PR #11), decided in Q25 and Q30. Partial: unit tests only; a real PDF export with the technical-details option on has not been checked. The Evidence line above describes the code before PR #11.
- **Built 2026-11-08, merged (PR #11):** export dialog option "A technical details section" (off by default) adds, at the end: recording session start and end, recorded steps, screenshots withheld, steps without a screenshot, Windows version (RtlGetVersion, for example "Windows 11 (build 22631)"), app version, and per step the program and UI element. Window titles, user names and machine names are never included (unit tested).

### PSR-7: Out of scope (M1)

Command-line control: start and stop from a script, set the output path and screenshot limit, hide the UI, limit to one process.

- **Steps Recorder:** /start, /output, /sc, /maxsc, /gui, /recordpid and /stop all worked. It must be launched through the shell; direct process creation fails with error 740.
- **Evidence:** research/baseline-and-competitors.md section 1, 'Command line'. No matching requirement in docs/02-requirements.md (grep for 'command line' found nothing).
- **Decided 2026-11-08 (Q25):** no command-line control in M1 or before the first release.
- **Gap:** None for M1. IT users (Q5) may still expect scripted start and stop later, and /recordpid overlaps with A6's window scope.

### PSR-8: Not tested

Clicks in elevated (admin) windows and on UAC prompts: what gets recorded, and whether password detection fails closed when UI Automation is denied.

- **Steps Recorder:** The manifest sets level="asInvoker" and uiAccess="true" (measured). Actual behavior on elevated windows and UAC prompts is unverified.
- **Evidence:** docs/06-open-questions.md Q17 (open, 'decide after the M1 test'). research/m0-spike-results.md finding 8 and its untested table ('Elevated windows, UAC prompts: M1 test'). research/m1-results.md 'Not tested' lists elevated windows and UAC. docs/05-roadmap.md M1 step 2: 'Still to do: an elevated window (Q17)'. No code detects elevated targets or the secure desktop (grep for elevat, uiaccess, TokenElevation found nothing in platform/ or app/src-tauri/src).
- **Gap:** Merged the three elevated/UAC rows. Test: start our test window as admin (the owner approves the UAC prompt) and record clicks and typing in it. Check whether clicks are dropped, unnamed or recorded, and that the screenshot is withheld if UI Automation is denied. Then trigger a UAC prompt mid-recording and check that capture resumes with fresh frames and no stale picture is labeled 'before the click'. Guess: the secure desktop hides everything from hooks and capture. Compare with Steps Recorder. Needs the owner; the result feeds the owner's Q17 decision on uiAccess and code signing.

### PSR-9: Partial

Clicks on the recorder's own controls (window, tray icon, menus, hotkeys) are left out of the recording.

- **Steps Recorder:** Not measured in baseline section 1. m1-results 'Tray test' asserts that Steps Recorder leaves its own controls out.
- **Evidence:** m1-results 'Tray test (2026-11-07)': tray-icon clicks were recorded (3 of 5), fixed (commits ebb3e40, dac12d1), and the owner's retest recorded 0 tray-icon clicks (with the earlier 'BabelSteps' prefix). Tray menu clicks were not recorded. Hotkeys: 0 of 11 shortcut steps (m1-results 'First live test') and the remapped-hotkey checks with control (55/55 run). Code: platform/windows/src/pipeline.rs ignore_own_windows (by process id) and the tray-rectangle and 'BabelSteps:' name drop.
- **Gap:** Merged two duplicate rows. The narrowed 'BabelSteps:' prefix has not been retested live. No recorded test checks that clicks on the main window or Settings window during a recording are absent (capture_selftest runs with ignore_own_windows: false). A taskbar that moves or auto-hides within 300 ms is unverified. Needs the owner's live retest.

### PSR-10: Partial

Sleep, lock and screen-resolution or scaling changes during a recording: capture recovers with fresh pictures and no wrong 'before the click' labels.

- **Steps Recorder:** Not in baseline.
- **Evidence:** Code: platform/windows/src/capture.rs refresh() rebuilds when the monitor list or sizes change, the GPU device is lost, or capture failed; indicator.rs rebuilds on WM_DISPLAYCHANGE; hooks.rs counts re-installs (REHOOKS). There is no session-lock or power handling: grep for WTSRegister, WM_WTSSESSION and WM_POWERBROADCAST found nothing. m1-results 'Recording engine self-test', Not tested: 'Sleep, lock, resolution changes and long sessions. The rebuild code exists but has not run.' docs/05-roadmap.md M1 step 3 is open.
- **Built 2026-11-08 (branch claude/project-thread-md1ty1):** `platform/windows/src/session.rs` listens (hidden window, `WTSRegisterSessionNotification` and `WM_POWERBROADCAST`) for lock, unlock, sleep and wake, including a session leaving the console. While locked or asleep nothing is recorded (clicks, keys and focus changes are dropped) and a typing run ends. On return capture is rebuilt, so the frame history starts empty and a click that beats the first fresh frame is flagged as not verified instead of getting a pre-lock picture. Waking onto a lock screen stays away until the unlock. The panel warns "Not recording: the screen is locked or the computer is asleep", and the notice after a stop says how many times it happened. Unit tests cover the lock and sleep state rules and the message decoding (`session.rs`). Self-test `lock_and_sleep` (8 checks) sends the same messages through the real hidden window, so it proves the wiring and the rebuild but not that Windows sends them.
- **Live test with the owner, 2026-11-08 (real Win+L and real sleep and wake; `research/m1-results.md` 'Real lock and sleep'):** Windows sent the lock, unlock, sleep and wake messages to the recorder, which went away and came back each time. No click step was made in either away period (4 s locked, 46 s asleep), capture was rebuilt once per return, the hooks were reinstalled both times, no step was marked not verified and none missed. Not checked: that the first picture after each return was current (the log has no picture content; the self-test checks that on the test window), clicks on the lock screen itself (the hooks never see that desktop), and a lock lasting longer than a few seconds.
- **Gap (still open):** Resolution and scaling changes were not touched. If the unlock message is ever missed, recording stays off until the next start, and the panel warning is the only sign (a guess: rare). It is unknown whether sign-out or shutdown runs stop-and-save. Earlier gap text: kept as an M1-exit item (roadmap step 3). Live tests, each with the owner's OK: change resolution or scaling mid-recording, lock and unlock (Win+L), sleep and wake. Check that capture resumes (the rebuild counter rises), the next click's picture is fresh, no stale frame is labeled 'before the click', and no step is made while locked. It is unknown whether sign-out or shutdown runs stop-and-save (m1-results 'Known limits').

# Roadmap

Milestones are sequenced by dependency, not calendar dates. Sizing and dates get set once the Q-decisions are made and the team and time budget are known.

## M0 — Decisions and spikes

**Goal:** settle the blocking questions and prove the risky parts work.

**Status (2026-11-02):** spikes 1 to 4 and the Steps Recorder baseline are done. See `research/m0-spike-results.md`. Remaining gaps are listed there and moved into M1.

- Answer the blocking questions in `06-open-questions.md`.
- Hands-on baseline of Steps Recorder: record sessions, inspect the output `.zip` and `.mht`, note settings and defaults. Update `research/baseline-and-competitors.md`.
- Spike 1: low-level mouse and keyboard hooks with a non-blocking queue. Measure callback latency and confirm no dropped events under load.
- Spike 2: screenshot on click with Windows.Graphics.Capture, including multi-monitor and mixed DPI.
- Spike 3: UI Automation element lookup at a click point, with timing and `IsPassword` checks against Notepad, a browser, and a Win32 app.
- Spike 4: one-recording end-to-end path: click → screenshot → step → Markdown file.

**Exit:** the spikes meet the D-targets or the targets are revised with reasons. Stack confirmed.

## M1 — Windows capture parity

**Goal:** match the Steps Recorder workflow.

**Status (2026-11-06):** the current focus (Q18). The owner wants core capture to be bulletproof before Scribe-style work continues. No capture code ships yet: hooks, screenshots, UI Automation and redaction exist only in the M0 spike. Platform-free pieces already in `core` (fail-closed password check, PII scrubbing, event normalizer) are reused.

**Status (2026-11-06, later):** the recording engine is built in `platform/windows` (branch `m1-capture`) and passes its self-test 36/36 on one monitor (`research/m1-results.md`). Next: wire it into the app (start, pause, stop, comments, hotkeys, tray indicator), then test real apps, multiple monitors, admin windows, sleep and lock, and build the parity checklist.

**Status (2026-11-06, end of session):** the recorder is wired into the app: record dialog (keystrokes off by default), recording panel, Ctrl+Alt+Shift+R/P/N/X hotkeys (excluded from recordings), tray indicator, and stop-and-save on close and quit. Privacy audit passed. First live test passed on 2026-11-07 (`research/m1-results.md`): UI start, all hotkeys, pause, note, stop, close while recording. Not yet: the tray menu, quitting from the tray. Next, in order:
1. Done 2026-11-07: tray menu and quit while recording pass. Clicks on the BabelSteps tray icon are no longer recorded (fixed and retested).
2. Real apps: Edge password fields pass (2026-11-07, 27/27, `browser-selftest`; plain and sign-in forms, "show password" button). Chrome and Firefox not installed; the owner chose Edge only. Still to do: an elevated window (Q17).
3. Sleep, lock, resolution change; multiple monitors if the owner has a second one.
4. The Steps Recorder parity checklist, item by item (the M1 exit). Drafted 2026-11-08 in `research/m1-parity-checklist.md`: 1 of 32 items passes, 18 partial; it lists what closes each gap and who must act.
Built 2026-11-08 on branch `m1-retention-cue-hotkeys`, NOT live-tested: retention settings (A8), the screen frame (C2), remappable hotkeys (C3). Self-tested live 2026-11-08 (`research/m1-results.md`): frame out of screenshots (both capture methods), edge clicks, remapped shortcuts left out of steps, retention on disk. Still needs the owner's click-through of the Settings window; multi-monitor is untested on hardware (Q20); Windows 10 is unsupported (Q19). Not built yet: the single-file output decision (A9, Q2).
Built 2026-11-08: the setting for what closing the window does (Q21). Live test 24 of 24 (`research/m1-results.md`): D1 is met in tray mode (6 MB), the note shortcut rebuilds the window, and the rebuilt window is hardened. Not tested yet: Show and double-click from the tray icon (they need real taskbar clicks), and sign-out during a recording with the window closed.

**Status (2026-11-07, end of session, paused by the owner):** all work is merged into `main` and pushed to GitHub. Done this session: the tray test and the tray-click fix; Edge password fields (27/27, `browser-selftest`). Resume with step 2's elevated window (Q17), then steps 3 and 4. Open, unverified: Edge's save-password prompt and whether the "show password" button took effect (`research/m1-results.md`); the narrowed "BabelSteps:" tray-name check has not been retested live. Both self-tests need the owner's OK for each run.

- Start, pause, stop, comments, and hotkeys (A1–A6, C2–C3).
- Multi-monitor and DPI correctness (A7).
- Recording storage in the folder bundle format (C4).
- Basic library and a simple viewer.
- Retention settings (A8).
- First task: a Tauri 2 shell, to prove the UI half of the stack and measure D1, D3, and D7.
- Carried over from M0: multiple monitors and mixed DPI, elevated windows and UAC, Chrome and Firefox, Steps Recorder's comment, pause, and settings UI, and sleep, lock, and resolution changes.
- Image size decision: crop to the active window, choice of format, and downscaling for export.
- Privacy fixes from the M0 audit, none of which may carry over from the spike: withhold the screenshot when the password check fails or times out, scan all visible windows, use all three password signals, and name the app rather than the window title (`describe_click` in `core`).

**Exit:** a parity checklist, derived from the baseline work, passes item by item. Performance targets D1–D3 met.

## M2 — Document and export

**Goal:** the Scribe-style value.

**Status (2026-11-02):** started at the owner's request, before M1 is finished. M2 needs two M1 items, which are pulled forward: the Tauri 2 shell (for the editor and PDF printing) and recording storage (C4). Keystroke capture on a real machine (B1–B4) still waits on M1's capture pipeline, so only its platform-free logic is built first. The rest of M1 remains open.

**Status (2026-11-06):** paused at the owner's request until M1 is done (Q18). Done so far: the Tauri shell and PDF export are merged. D5 is met on one fast PC (about 9 s for 100 ultrawide steps; see `research/m2-results.md`), not yet on lower-end hardware. Next: the editor UI.

- Keystroke capture with text grouping, shortcut naming, and password suppression (B1–B4).
- Document generation after stop (B5).
- Block editor with formatting (B6).
- Manual redaction (B7), automatic redaction of passwords and PII (B12–B14), and manual steps (B11).
- Markdown export (B9) and PDF export (B8).

**Exit:** golden-file export tests pass. A 100-step recording exports to PDF within D5.

## M3 — Windows release

- Installer, code signing, and the trust-signal pages (Privacy doc §5). Decision: no paid certificate. Apply to SignPath Foundation (free; needs an OSI license, a public repo, MFA, and a published signing policy, and the Foundation becomes the publisher). Options and caveats are in `research/baseline-and-competitors.md` §4.
- If SignPath does not accept the project, ship unsigned with mitigations: published SHA-256 hashes, reproducible builds where possible, a clear "why Windows warns you" page, submission of every release to Microsoft Defender's false-positive portal and major antivirus vendors, and distribution through winget.
- Accessibility pass on the app (C6) and themes (C5).
- Antivirus false-positive submissions.
- Documentation and user guide.

**Exit:** signed installer, privacy guarantees verified in CI, first public or internal release.

## M4 — macOS

- Backends for ScreenCaptureKit, CGEventTap, and AXUIElement.
- Permission onboarding flow (Screen Recording, Accessibility, Input Monitoring).
- Re-run the parity checklist and privacy tests on macOS.

## M5 — Linux (backlog, not scheduled)

Kept in the backlog. Until it is scheduled, we only keep the platform interfaces clean so a port stays possible.

- Start with X11 (XRecord or XInput2 for input, XGetImage for screenshots, AT-SPI for names).
- Wayland: plan based on the Wayland decision in `03-architecture.md` §6. Likely limited feature set, using portals where allowed.
- Packaging (AppImage and/or Flatpak).

## Later (not scheduled)

- Single-file HTML export (B10) and bundle export.
- Opening Steps Recorder output (A10).
- Local-only AI assistance for step descriptions.
- Browser extension that stays local.

**Cleared (2026-11-08, later): the app window loads its UI.** With the owner's OK, the rebuilt release exe (built with `--features custom-protocol`, no dev server running) opened its bundled page (`http://tauri.localhost/`), showed the Library screen, and showed the Settings screen with all three sections (cancelled without saving). While it ran, no BabelSteps or WebView2 process held a network socket other than the test's own local debugging port. The cause of the owner's second report is unknown (likely the old exe or a browser tab). The build guard now asks Tauri itself whether it is in dev mode; it refuses a release build without the bundled UI and accepts `cargo build --features custom-protocol` (checked 2026-11-08). Still not tested: changing and saving settings, shortcut re-registration after a change, the stop notice, the owner's own click-through. The history of the blocker follows.

**BLOCKER (2026-11-08, cleared later the same day; see above): the app window shows "localhost refused to connect" for the owner.** Seen with the first release exe (`target\release`, built without `custom-protocol`, a known cause) and reported again with the corrected exe (`target\app-release\release\babelsteps-app.exe`, built with `--features custom-protocol`). Not diagnosed: the owner's second report said "the edge window", and it is unclear whether it was the app window or a browser tab opened on localhost. The app was never launched in this session (it would take the owner's focus), so the corrected build is unverified in a running window. Start here: launch the corrected exe with no dev server running, confirm the UI loads, check for a leftover `npm run dev` or an old process, and check the WebView2 loader. Static check (2026-11-08, nothing launched): the corrected exe contains the built frontend files (`index-*.js`, `index-*.css`); the old `target\release` exe does not, so it was deleted to prevent launching it by mistake. No dev server was listening on port 1420. Still unverified in a running window. Until then the Settings click-through and the socket check (docs/04 section 7) have not happened. The branch `m1-retention-cue-hotkeys` is committed locally and was pushed earlier at `2b6cace`; later commits are not pushed.
Settings click-through, the app's own wiring (shortcut re-registration, stop notice) and the multi-monitor path remain untested. PR #2 must not be merged until the UI is confirmed to load. Hypotheses: the build did not embed the frontend, or the window still points at devUrl. Suggested first step: run `npm run tauri build` through the Tauri CLI and compare with the cargo build.

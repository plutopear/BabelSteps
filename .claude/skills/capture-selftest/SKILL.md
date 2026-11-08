---
name: capture-selftest
description: Run the BabelSteps Windows capture self-test (hooks, before-the-click screenshots, UI Automation, password redaction) with safety rules for a live desktop and a compact summary. Use after changing hooks, screen capture, UI Automation, redaction, or the step pipeline, and whenever capture behavior needs checking on the real machine.
---

# Capture self-test

This runs on the owner's daily PC while they may be using it. Tell them first, in one line: it takes about a minute, shows a small green test window on top, and takes keyboard focus for a few seconds while it types into that window. It ignores their real input.

It tests the real recorder in `platform/windows` (`src/bin/capture_selftest.rs`): before-the-click screenshots (also with analysis slowed down), password text and pixels on disk, fail-closed on UI Automation failure, timeout and scan failure, the fallback-capture note, pause, comments, orphan images and hook speed. `-Spike` runs the old M0 spike test instead. If typing stops with "lost focus", the safety guard worked; rerun once.

```powershell
powershell -NoProfile -File .claude/skills/capture-selftest/scripts/selftest.ps1
```

Options: `-BuildOnly` compiles without running. Always the release build: the debug build encodes images too slowly and runs out of frame budget. `-LogFile <path>` re-summarizes an earlier log without running anything.

It prints every failed check, a pass count, and the key numbers. The full log and `selftest-report.json` paths are printed too; search them with Grep or a small script rather than reading them, because the report holds a trace for every click.

## Reading the results

- A pass only covers what the test exercises: one monitor at 100% scaling, a private test window, and synthetic clicks. Do not claim more. `research/m0-spike-results.md` lists the gaps.
- If **every** check passes after a capture change, ask whether the test could have caught the bug. M0's first version passed everything while missing real problems.
- Compare timings with `docs/02-requirements.md` section D, and report CPU as a percentage of one core.
- `newer frame skipped 0 times` in the normal phases is expected. The forced-race phase is what exercises that path.

## If you change the test

Any synthetic input must stay safe while a person uses the PC. These protections exist because early versions moved the real cursor and recorded the owner's real clicks:

- Send each click as one `SendInput` call that also returns the cursor (`testwin::click_with`).
- Check that our window is under the point first (`safe_click`), and that it has focus before typing (`safe_type`).
- Keep `only_injected: true`, so the owner's real input is ignored.
- Delete full-screen images after checking them. View only `test-window-crop.png`, which is cropped to the client area so nothing behind the window shows.

Use the `record` mode only with the owner's agreement. It records their real clicks and full screen, and it has no PII redaction and fails open on password detection, so it is not safe for sensitive work.

## Browser password fields

`target/release/browser-selftest.exe "<path to msedge.exe or chrome.exe>"` (build with `cargo build --release -p babelsteps-windows --features selftest --bins`). It opens our own test page in the browser with a throwaway profile, takes focus for about 45 seconds, and records the whole screen. The same rules apply: warn the owner and get their OK for each run, and don't view anything except the page crops it keeps in `target/browser-selftest/`.

## Live test of the real app

`scripts/live-app-test.ps1` runs the release app (`target\app-release\release`, built with `--features custom-protocol`) for about two minutes. It drives the app's own buttons through a local WebView2 debugging port, not with the mouse. It sends real clicks only to its own small test window, and sends shortcut keys only when BabelSteps holds them. It covers: start from the UI, pause and note (button and shortcut), stop button and retention notice, a remapped start shortcut, stop-now, resetting shortcuts, memory and CPU, sockets while recording, and images on disk. It saves your settings first and restores them afterwards, and it deletes its test recording. The recording includes the owner's whole screen and any real clicks they make, so get their OK for each run and do not view its images.

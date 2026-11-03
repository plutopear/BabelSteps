---
name: capture-selftest
description: Run the BabelSteps Windows capture self-test (hooks, before-the-click screenshots, UI Automation, password redaction) with safety rules for a live desktop and a compact summary. Use after changing hooks, screen capture, UI Automation, redaction, or the step pipeline, and whenever capture behavior needs checking on the real machine.
---

# Capture self-test

This runs on the owner's daily PC while they may be using it. Tell them first, in one line: it takes about a minute, loads all CPU cores for about 10 seconds, shows a small green test window, and takes keyboard focus for about a second. It then ignores their real input.

```powershell
powershell -NoProfile -File .claude/skills/capture-selftest/scripts/selftest.ps1
```

Options: `-BuildOnly` compiles without running. `-LogFile <path>` re-summarizes an earlier log without running anything.

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
- Delete full-screen images after checking them. View only `test-window-crop.png`.

Use the `record` mode only with the owner's agreement, because it records their real clicks and screen.

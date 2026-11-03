# Baseline and Competitors

Working notes. Items marked *(verify)* have not been confirmed on a real machine yet. They become confirmed facts during M0.

## 1. Steps Recorder (parity baseline)

**Installed binary on the planning machine**
- Path: `C:\Windows\System32\psr.exe`
- File version: 10.0.26100.8521 (Windows 11, build 26100 line)
- Product name: Microsoft Windows Operating System. File description: Steps Recorder.

**Behavior we believe is true (verify)**
- Records a screenshot on each mouse click and marks the click location.
- Lets the user add comments during a recording, and pause and resume.
- Output is a `.zip` containing an MHTML (`.mht`) document and image files, which opens in a browser.
- Keeps a limited number of recent screenshots, configurable *(verify default and range)*.
- Records the full desktop. Window-only capture *(verify)*.
- Keystroke or text-entry capture: *unknown*. Must be checked by recording a session that types text and inspecting the output.

**Hands-on verification checklist (M0)**
1. Record a session with: a click on a button, a text entry, a keyboard shortcut, a comment, a pause, and a second monitor.
2. Save the output. Note the file name, size, and contents of the zip. Note the `.mht` structure.
3. Open the output in a browser. Note how each step is described, and whether text entry appears.
4. Check any settings UI for screenshot count and other options. Note all defaults.
5. Test with an elevated window and a UAC prompt. Record what is captured.
6. Record each observation in this file with a date.

## 2. Scribe (competitor)

**From general knowledge (verify current features before relying on them)**
- Cloud-first product with a browser extension and desktop app.
- Auto-captures a screenshot per click and keystroke-driven step text, then generates a step-by-step guide.
- Exports to several formats (including PDF and Markdown *(verify)*), and shares through links and integrations with other cloud services.
- Requires an account and sends content to its servers.

**Where BabelSteps differs (planned)**
- Fully local: no account, no cloud, no network from the app.
- Keystroke capture is explicit, opt-in, and suppressed for password fields.
- Output is editable locally and exports to PDF and Markdown offline.
- Native, lightweight app, and a shared core across Windows, macOS, and Linux.

## 3. Open research tasks

- Confirm Scribe's current export formats and privacy claims from its own documentation.
- Survey other local-first options (e.g., ShareX-style tools, Snagit, Greenshot, and open-source step recorders) for prior art on keystroke grouping and export.
- Check whether Steps Recorder's output format can be read by other tools (useful for Q2).

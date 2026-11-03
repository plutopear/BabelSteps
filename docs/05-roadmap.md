# Roadmap

Milestones are sequenced by dependency, not calendar dates. Sizing and dates get set once the Q-decisions are made and the team and time budget are known.

## M0 — Decisions and spikes

**Goal:** settle the blocking questions and prove the risky parts work.

- Answer the blocking questions in `06-open-questions.md`.
- Hands-on baseline of Steps Recorder: record sessions, inspect the output `.zip` and `.mht`, note settings and defaults. Update `research/baseline-and-competitors.md`.
- Spike 1: low-level mouse and keyboard hooks with a non-blocking queue. Measure callback latency and confirm no dropped events under load.
- Spike 2: screenshot on click with Windows.Graphics.Capture, including multi-monitor and mixed DPI.
- Spike 3: UI Automation element lookup at a click point, with timing and `IsPassword` checks against Notepad, a browser, and a Win32 app.
- Spike 4: one-recording end-to-end path: click → screenshot → step → Markdown file.

**Exit:** the spikes meet the D-targets or the targets are revised with reasons. Stack confirmed.

## M1 — Windows capture parity

**Goal:** match the Steps Recorder workflow.

- Start, pause, stop, comments, and hotkeys (A1–A6, C2–C3).
- Multi-monitor and DPI correctness (A7).
- Recording storage in the folder bundle format (C4).
- Basic library and a simple viewer.
- Retention settings (A8).

**Exit:** a parity checklist, derived from the baseline work, passes item by item. Performance targets D1–D3 met.

## M2 — Document and export

**Goal:** the Scribe-style value.

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

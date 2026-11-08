# Decisions and Open Questions

Other docs refer to these by number. Do not re-ask anything under **Decided**.

## Open

**Open: Q17. Admin windows and code signing (decide after the M1 test).** Steps Recorder uses a Windows permission called `uiAccess`, which is designed to let accessibility tools work with admin windows without running as admin. It needs a signed app installed in a protected folder such as Program Files. Without it, BabelSteps is expected to miss clicks in admin tools unless it is itself run as admin. This raises the value of a signature for IT users.

## Decided (2026-11-02)

| # | Topic | Decision |
| --- | --- | --- |
| Q1 | Stack | Rust core + Tauri 2 + web UI. The owner is not a developer and wants cross-platform from the start to avoid a rewrite, so Claude owns technical choices. |
| Q2 | "1:1 parity" | Same workflow and features, with our own file format. No `.mht` compatibility or import for now. |
| Q3 | Screenshot timing | The state just before the click. Steps Recorder shows the state after the click (measured in M0), so this is a deliberate difference. |
| Q4 | Keystroke capture | Off by default, enabled per recording with a visible indicator. Password fields are never recorded. |
| Q5 | Audience | IT/helpdesk first, end-user training second. See requirements section E. |
| Q6 | Editing | WYSIWYG block editor: edit titles and descriptions, reorder, delete, add notes, redact. Documents round-trip with Markdown. |
| Q7 | PDF layout | US Letter only; A4 is not offered. A header and footer (title and page number) are an option in the export dialog, off by default. |
| Q7a | Markdown export | `document.md` plus an `images/` folder. |
| Q8 | Licensing and name | Open-source core under a permissive license, final choice at first public release. The name stays BabelSteps. |
| Q9 | Platform order | Windows, then macOS. Linux is in the backlog; platform interfaces stay clean so a port remains possible. |
| Q10 | Encryption at rest | Dropped. The owner wants automatic redaction of passwords and PII instead (B12–B14, Privacy doc §3). |
| Q11 | Git | Yes, a local repository. No remote yet. |
| Q12 | Screenshot retention | Unlimited, with a warning at a size threshold. |
| Q13 | Recording scope | Full screen by default, with a choice of window or monitor. |
| Q14 | Hotkeys | `Ctrl+Alt+Shift+R` start/stop, `P` pause, `N` add note, `X` panic stop. All remappable, with conflict warnings. Two-modifier defaults were rejected because they clash with browsers and Task Manager. |
| Q15 | Browser extension | Not in v1. |
| Q16 | Code signing | The owner is in the US and does **not** want to pay for a certificate. Apply to SignPath Foundation (free for open source) before the first public release, with unsigned distribution as the fallback (roadmap M3). Microsoft Artifact Signing (about $120/yr) stays available if they change their mind. |
| Q18 | Order of work (2026-11-06) | Steps Recorder parity first, Scribe-style features later. M2 is paused where it stands; M1 capture must be solid and tested before M2 resumes. Overrides the 2026-11-02 request to start M2 early. |
| Q19 | Supported Windows versions (2026-11-08) | Windows 11 only for now; Windows 10 is not supported, so the pre-2004 behaviour of the screen frame is not tested. If Windows refuses to keep the frame out of screenshots, no frame is shown (the tray icon stays). At the start of a recording the app also shows a warning; if the refusal happens later, when monitors change mid-recording, the frame just disappears with no warning yet. |
| Q20 | Multiple monitors (2026-11-08) | Built and reviewed without multi-monitor hardware: a frame on every monitor (negative coordinates, per-monitor scale, rebuilt when monitors change), hotkeys unaffected. Geometry has unit tests. Not tested on real multi-monitor hardware; do not claim A7 is met until it is. |
| Q21 | Closing the window (2026-11-08) | The user chooses in Settings. The two options are "Quit BabelSteps" and "Keep running in the tray". The default is quit, the same as Steps Recorder and the behaviour before this setting existed. In both modes, closing during a recording stops and saves it first. In tray mode the window is destroyed rather than hidden, which frees the WebView2 engine. It comes back through Show BabelSteps, by double-clicking the tray icon, or with the note shortcut. D1 (idle in the tray) applies to tray mode, and it was met on 2026-11-08 at 6 MB. The tray menu opens on right-click. |
| Q30 | Step times, typed-text notice, step counts (2026-11-08) | PSR-2: the time of each step is always shown in the app; in exports it is an option, off by default (local clock time, with the date once at the top). PSR-3: every export carries one line, by default, saying what was not captured: with keystrokes off "Typed text was not recorded. Keyboard shortcuts were.", with keystrokes on "Typed text was recorded. Review it before sharing." It is an ordinary paragraph the user can edit or delete, and it never claims the file is free of sensitive data. PSR-5: the app shows how many steps were recorded and how many screenshots were withheld or missed after every recording; exports include the counts only inside the optional technical-details section (Q25). Numbered Q30 because another branch adds Q22 to Q29. |
| Q22 | Milestone exit with hardware gaps (2026-11-08) | A milestone may exit with items untested only because we lack the hardware (a second monitor, A7) or a machine state (an elevated window, PSR-8). Each such item is listed as "not tested" in the release notes and retested when the hardware exists. It never counts as passed. Exit criteria per milestone are in `07-definition-of-done.md`. |
| Q23 | Single-file output for M1 (2026-11-08) | The PDF export satisfies A9 for M1. Single-file HTML stays a later extra (B10, P2). What remains for A9 is a PDF exported from a real recording. |
| Q24 | Recording size (2026-11-08) | Target about Steps Recorder's: a typical 30-step recording under 10 MB, so it can be emailed (PSR-1). Steps Recorder measured 10.3 MB zipped for 31 steps. |
| Q25 | Technical details and command line (2026-11-08) | PSR-6: a technical-details section is an export option, off by default, with no user or machine names. PSR-7: no command-line control in M1 or before the first release. |
| Q26 | "Typical laptop" for speed targets (2026-11-08) | D2 and D5 are measured on one named real machine, ideally 4 cores and 8 GB of memory, in addition to the owner's desktop. The machine is still to be found. |
| Q27 | Sensitive data in screenshots (2026-11-08) | M2 covers PII in typed text plus manual review before export. Detecting PII in screenshots with local text recognition (part of B13) comes after the first release. |
| Q28 | Licence and first release (2026-11-08) | A permissive licence (MIT or Apache-2.0; the exact one is picked at release) and a public first release, because free signing through SignPath requires a public open-source repo. Refines Q8. |
| Q29 | macOS spending (2026-11-08) | The Mac port needs Mac hardware and an Apple developer account (about $99 a year). Decide when M3 ships. |

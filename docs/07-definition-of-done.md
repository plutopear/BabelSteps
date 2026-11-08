# Definition of Done

Pass/fail exit criteria for each milestone in `05-roadmap.md`. Agreed with the owner on 2026-11-08 (decisions Q22 to Q29 in `06-open-questions.md`). Update the Status column as evidence lands.

## Rules

- A milestone is done when every row is **Met** and the owner has signed it off. Otherwise the row stays open, or the owner moves it to a later milestone and the reason is written down.
- **Met** means shown on the real desktop, with the evidence in a `research/*-results.md` file. Unit tests alone count as Partial for behaviour that depends on the operating system.
- Items untested only because the hardware or machine state is missing (a second monitor, an elevated window) may be left "not tested" at exit. They are listed in the release notes, retested when possible, and never counted as passed (Q22).
- Each milestone ends with the owner doing one real task with the build.
- True at every exit, so not repeated below: the no-network check passes; no networking crates are added; the `privacy-auditor` reports no FAIL; `core/` holds no platform code; test recordings are deleted; the roadmap and decisions doc are current.

Status values: Met, Partial, Not started, Needs decision, Not checked.

## M0: Decisions and spikes (closed 2026-11-08)

| # | Done when | Checked by | Status |
| --- | --- | --- | --- |
| 0.1 | Spikes 1 to 4 ran and their numbers are recorded | `research/m0-spike-results.md` | Met |
| 0.2 | Steps Recorder baseline recorded from a real machine | `research/baseline-and-competitors.md` | Met |
| 0.3 | Each D-target is met or revised with a reason | D6 measured at about 11 ms p95; D7 set from the measured peak | Met |
| 0.4 | Stack confirmed | Q1; the Tauri shell loads its window | Met |
| 0.5 | Blocking questions answered | Q1 to Q16, Q18 onward; Q17 left for the M1 admin-window test | Met |
| 0.6 | Roadmap marks M0 closed | `05-roadmap.md` | Met |

## M1: Windows capture parity

Done when a Steps Recorder user can switch and lose nothing they rely on, and BabelSteps never saves a password. The detailed test is `research/m1-parity-checklist.md`.

| # | Done when | Checked by | Status |
| --- | --- | --- | --- |
| 1.1 | Every parity checklist item is Pass, or moved out of M1 with a written reason (Q22 for hardware gaps) | Parity checklist (2 of 32 Pass on 2026-11-08) | Partial |
| 1.2 | Start, stop, pause, notes and all four shortcuts work from the window, the shortcuts and the tray menu on the current release build (A1, A4, A5, C3) | Live app and tray tests; owner's tray clicks | Partial |
| 1.3 | Each click gives one step with the before-click picture, green outline, click mark and a name with no window title (A2, A3, A11, D6) | Self-test plus a recording in Notepad, File Explorer and Edge | Partial |
| 1.4 | Passwords are never in a step, always blacked out, and the picture is withheld when the check fails, including in an elevated window (B4, B12, PSR-8) | `capture-selftest`, `browser-selftest`, one elevated-window run | Partial |
| 1.5 | Record one window or one monitor, with redaction still aligned (A6) | New self-test | Not started |
| 1.6 | Two monitors at different scaling (A7) | Self-test on a second monitor; else "not tested" under Q22 | Partial |
| 1.7 | Sleep, lock and resolution change mid-recording recover with fresh pictures (PSR-10) | Live run | Not started |
| 1.8 | Save to a user-picked folder; library opens, renames, deletes; retention works (C4, A8) | Live run with a non-default folder | Partial |
| 1.9 | A real recording exports to one PDF that opens without the app (Q23), and a 30-step recording is under 10 MB (Q24, D8, PSR-1) | Real recording exported and opened on another account | Partial |
| 1.10 | Owner sees the frame and tray colours; the frame never appears in a picture (C2) | Self-test plus the owner's eyes | Partial |
| 1.11 | D1 idle under 50 MB, D2 recording under 5% CPU on the Q26 laptop, D3 start under 1 s, D7 recording memory under 250 MB | Measured on the release build, conditions written down | Partial |
| 1.12 | No socket opens during a full record, edit and export session (C1) | Socket watch during a live session | Partial |
| 1.13 | The owner records a real 15-minute task and would send the result to a colleague | Owner | Not started |

## M2: Document and export

Done when a recording becomes a guide the owner would hand to a user: edited, reviewed for sensitive data, and exported to PDF and Markdown. Paused until M1 exits (Q18).

| # | Done when | Checked by | Status |
| --- | --- | --- | --- |
| 2.1 | With keystrokes on, typing groups per field, shortcuts are named, passwords never appear (B1 to B4) | Self-test with keystrokes on, Notepad and Edge | Partial |
| 2.2 | Stopping opens a numbered guide with a picture and caption per step (B5) | Live app run | Partial |
| 2.3 | Editor edits, reorders, deletes, merges and inserts steps, text, headings, lists and images, with basic formatting; a shortcut adds a step while recording (B6, B11) | Live editor run against a written list of actions | Not checked |
| 2.4 | Markdown export, import, export gives the same file; it pastes cleanly into one ticketing tool the owner names (B6, B9, E2) | Golden-file test plus a paste test | Partial |
| 2.5 | PDF is US Letter, header and footer optional, print-friendly, matches its golden file (B8, E6) | Golden-file test (only HTML has one today) | Partial |
| 2.6 | Manual black-out boxes; exports never contain the hidden pixels (B7) | Pixel check in exported files | Partial |
| 2.7 | Pre-export review of every step, typed-text PII pre-marked, with a reminder that detection can miss things (B13 typed text, B14, E5; screenshot detection is after release, Q27) | Live run with planted test data | Not checked |
| 2.8 | 100 real-screenshot steps export to PDF in under 10 s on the Q26 laptop (D5) | Timed run | Partial |
| 2.9 | Note, Tip and Warning callouts and the two templates exist (E1, E3) | Live editor run | Not checked |
| 2.10 | The owner turns a real recording into a guide and exports both formats | Owner | Not started |

## M3: Windows release

Done when a stranger can install BabelSteps without being scared off and the privacy promise is checked automatically.

| # | Done when | Checked by | Status |
| --- | --- | --- | --- |
| 3.1 | Permissive licence chosen and the repository public (Q28) | Owner | Not started |
| 3.2 | Installer under 20 MB installs and uninstalls cleanly on a fresh Windows 11 machine (D4) | Clean virtual machine | Not started |
| 3.3 | Signed through SignPath, or the unsigned fallback is complete: SHA-256 hashes, winget, "why Windows warns you" page, Defender submission (Q16) | Release page | Not started |
| 3.4 | CI runs unit tests, golden files, the no-network dependency check and a runtime socket test on every change; a failure blocks release | GitHub checks | Partial: CI runs all four (runtime test covers core only, not Windows capture); blocking merges needs the repository public (`04-privacy-and-security.md` section 8) |
| 3.5 | Admin-window approach decided and shipped (Q17) | Decision and test | Needs decision |
| 3.6 | Record, edit and export work keyboard-only and with Narrator; themes follow Windows (C5, C6) | Accessibility run | Not started |
| 3.7 | User guide and privacy page published | Website or repo | Not started |
| 3.8 | The owner installs the release on a PC that never had BabelSteps and records and exports a guide | Owner | Not started |

## M4: macOS

Needs Mac hardware and an Apple developer account, decided when M3 ships (Q29).

| # | Done when | Checked by | Status |
| --- | --- | --- | --- |
| 4.1 | Mac capture, input and element lookup sit behind the same interfaces; `core/` unchanged | Code review | Not started |
| 4.2 | First run explains and requests Screen Recording, Accessibility and Input Monitoring; a refusal is explained and never leads to unsafe recording | Live run, granting and refusing each | Not started |
| 4.3 | Password detection fails closed on Mac | Mac self-test | Not started |
| 4.4 | Parity checklist and privacy tests rerun; each item Pass or not applicable with a reason | Mac copy of the checklist | Not started |
| 4.5 | Signed and notarised | Install on a clean Mac | Needs decision |
| 4.6 | D1 to D8 measured on a named Mac | Results doc | Not started |

## M5: Linux (backlog)

Until scheduled, done means the door stays open: no Windows or Mac types in `core/`, checked at each milestone exit. When scheduled, it gets a table modelled on M4, plus the Wayland decision and packaging.

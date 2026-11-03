# Requirements

**Priority key:** P0 = required for v1 · P1 = required for v1 if time allows, otherwise next · P2 = later.
Items marked *(verify)* are based on public knowledge of Steps Recorder and must be confirmed on a real machine during milestone M0.

## A. Steps Recorder parity (Windows)

| ID | Requirement | Pri |
| --- | --- | --- |
| A1 | Start and stop a recording from the UI and from a global hotkey. | P0 |
| A2 | Capture a screenshot on each mouse click. Outline the clicked element, as Steps Recorder does with a green rectangle, and mark the click point. | P0 |
| A3 | Generate a text description per click from the target element's name and type (e.g., "Left-click *Save* button in Notepad"). Fall back to coordinates when no name is available. | P0 |
| A4 | Add user comments at any point, by hotkey or from the UI. | P0 |
| A5 | Pause and resume without ending the recording. | P0 |
| A6 | Record the full screen or a chosen window/monitor. | P0 |
| A7 | Correct behavior on multi-monitor setups and mixed DPI scaling. | P0 |
| A8 | Retention limit on recent screenshots, configurable. Steps Recorder keeps only the last 25 by default; ours defaults to unlimited with a size warning (Q12). | P1 |
| A9 | Output a single self-contained file that opens without the app (format decision: see Q2). | P0 |
| A10 | Open and view existing Steps Recorder output (`.zip` containing one `.mht`). | P2 |
| A11 | With keystroke capture off, still record a step when the user types ("Typed in *User name* field"), with no text. This matches Steps Recorder. | P0 |

## B. Scribe-style additions

| ID | Requirement | Pri |
| --- | --- | --- |
| B1 | Capture clicks **and** keystrokes into one ordered step list. | P1 |
| B2 | Group typed text into a single step ("Type *quarterly report* into *File name* field"). A group ends on focus change, Enter, Tab, or a click. | P1 |
| B3 | Name keyboard shortcuts as actions (Ctrl+S → "Save"; Enter → "Press Enter"). | P1 |
| B4 | Password fields are never recorded as text (see Privacy doc). | P0 |
| B5 | Auto-generated document after stop: numbered steps, a screenshot per step, and caption text. | P1 |
| B6 | WYSIWYG editable document: edit titles and descriptions, reorder, delete, merge, and insert text, headings, lists, and images. Basic formatting (bold, italic, underline, lists, headings, code). Content round-trips with Markdown (import and export) without losing structure. | P1 |
| B7 | Blur or black-out regions on any screenshot before export. | P1 |
| B8 | Export to **PDF**. | P0 |
| B9 | Export to **Markdown** (text file plus an `images/` folder). | P0 |
| B10 | Export to single-file HTML. | P2 |
| B11 | Keyboard shortcut to add a manual step or note while recording. | P1 |
| B12 | Automatic redaction of passwords: every password field in the window is blacked out at capture on every step, and the original pixels are never stored. | P0 |
| B13 | Automatic detection and redaction of PII and secrets in typed text and on screenshots (emails, phones, card numbers, ID numbers, IPs, API keys and tokens), using local-only detection (UI Automation text plus local OCR). | P1 |
| B14 | Review step before export: accept, remove, or add redaction boxes. A strict mode burns in redactions at capture. A user-editable list of custom words and patterns. | P1 |

## C. Cross-cutting

| ID | Requirement | Pri |
| --- | --- | --- |
| C1 | Entirely local: no network calls, telemetry, accounts, or cloud storage (see Privacy doc). | P0 |
| C2 | Always-visible recording indicator (tray icon plus a visual cue on the capture target). | P0 |
| C3 | Global hotkeys for start, pause, add note, and stop. A panic-stop hotkey always works. All hotkeys are remappable in settings, with conflict warnings. Defaults: `Ctrl+Alt+Shift+R/P/N/X`. | P0 |
| C4 | Recordings stored in a user-chosen folder, with an in-app library to browse, rename, and delete. | P0 |
| C5 | Light and dark themes that follow the OS setting. | P1 |
| C6 | The app itself is keyboard-navigable and screen-reader friendly. | P1 |
| C7 | Settings are stored locally and can be exported and imported. | P2 |

## D. Non-functional targets (proposed; validate in M1)

| ID | Target |
| --- | --- |
| D1 | Idle in tray: under 50 MB RAM, near-zero CPU. |
| D2 | While recording: under 5% CPU sustained on a typical laptop, no dropped clicks. |
| D3 | Cold start to tray: under 1 second. |
| D4 | Installer: under 20 MB. |
| D5 | PDF export of a 100-step recording: under 10 seconds. |
| D6 | Click-to-screenshot latency: under 150 ms, with the "before" state captured (see Q3). Measured in M0: p95 about 11 ms. |
| D7 | Memory while recording: under 250 MB on a 5120×1440 screen (proposed from M0's peak of about 160 MB, before the UI is added). |

## E. IT/helpdesk and end-user training needs (decided 2026-11-02)

| ID | Requirement | Pri |
| --- | --- | --- |
| E1 | Callout blocks inside a document: Note, Tip, Warning. Useful for training guides. | P1 |
| E2 | Markdown output pastes cleanly into common ticketing and knowledge-base tools (plain headings, numbered lists, relative image links). | P1 |
| E3 | Built-in document templates: "Helpdesk how-to" and "Training guide" (title, purpose, prerequisites, steps, result). | P1 |
| E4 | Optional environment line in a document (OS version, app name and version), off by default, never auto-filled with user or machine names. | P2 |
| E5 | Redaction is prominent in the export flow, since helpdesk recordings often show customer or admin data. A pre-export checklist reminds the user to review screenshots. | P1 |
| E6 | Print-friendly PDF layout for handouts. | P1 |

## Non-goals for v1

- Cloud sharing, links, team workspaces, or accounts.
- AI-generated step text (revisit later, and only with fully local models).
- Video or audio recording.
- Browser extension (a possible later addition, but it must stay local).
- Mobile.
- Integrations with Confluence, Notion, Google Docs, or similar services.

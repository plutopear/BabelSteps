# Architecture

## 1. Stack options

| Option | Pros | Cons |
| --- | --- | --- |
| **A. Rust core + Tauri 2 shell + web UI (recommended)** | Small installer and low RAM. One UI codebase for Windows, macOS, and Linux. Rust gives direct, safe access to Win32, Cocoa, and X11/Wayland APIs through crates. Strong performance for input hooks. | Rust has a learning curve. Linux uses WebKitGTK, whose rendering can lag WebView2 and WKWebView. |
| **B. C# / .NET 8 + WinUI 3** | Best native Windows integration. Fastest path to a polished Windows-only v1. | Windows-only. macOS and Linux would mean rewriting the UI and capture layers. |
| **C. Electron** | Easiest web-style UI and largest ecosystem. | Large install and RAM footprint, which conflicts with the "highly optimized" goal. |
| **D. Qt (C++ or Python bindings)** | Mature, truly cross-platform, native look. | Licensing choice (LGPL or commercial). C++ effort is high. |

**Decision (2026-11-02):** Option A. It best fits the "fast, efficient, cross-platform later" goals and avoids a UI rewrite when macOS arrives. Frontend framework and editor library are chosen in M0; the leading candidate for the WYSIWYG editor is ProseMirror/TipTap, which has good Markdown round-trip support.

## 2. Components

```
┌──────────────────────────── UI (web: TypeScript + framework TBD) ───────────────────────────┐
│  Recorder controls · Library · Document editor (blocks) · Export dialog · Settings          │
└───────────────────────────────────────────▲─────────────────────────────────────────────────┘
                                            │ Tauri commands / events (IPC)
┌───────────────────────────────────────────┴─────────────────────────────────────────────────┐
│  Core (Rust)                                                                                │
│  ┌──────────────┐  ┌────────────────┐  ┌───────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │ Event        │→ │ Normalizer     │→ │ Step builder  │→ │ Document     │→ │ Exporters  │  │
│  │ sources      │  │ (text grouping,│  │ (joins shot + │  │ store        │  │ PDF / MD / │  │
│  │ (input hooks)│  │  shortcuts)    │  │  UIA target)  │  │ (SQLite +    │  │ HTML       │  │
│  └──────┬───────┘  └────────────────┘  └───────┬───────┘  │  images)     │  └────────────┘  │
│         │                                       │          └──────────────┘                 │
│  ┌──────┴──────────────── Platform backends (trait per capability) ──────────────────────┐  │
│  │ CaptureBackend · InputBackend · AccessibilityBackend · PermissionCheck               │  │
│  │   windows (v1)  |  macos (M4)  |  linux-x11 / linux-wayland (M5)                     │  │
│  └──────────────────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

## 3. Windows capture pipeline (v1)

- **Input:** low-level hooks (`WH_MOUSE_LL`, `WH_KEYBOARD_LL`) on a dedicated thread with its own message loop. Hook callbacks must return almost immediately or Windows drops the hook, so callbacks only timestamp the event and push it to a lock-free queue.
- **Screenshots:** Windows.Graphics.Capture runs continuously during a recording. Each monitor keeps 3 recent frames in GPU memory, at least 25 ms apart. A click reads back the newest frame captured before it. Capturing on demand showed the wrong state in every trial (M0: 0 of 30 correct). GDI `BitBlt` remains a last-resort fallback that cannot show the before state, so it needs a visible warning.
- **Threads:** separate threads for hooks, frame readback, analysis (UI Automation and password scan), and image encoding. A password scan took about 270 ms on a large web page, so analysis must never delay the readback of the next click.
- **Element identity:** UI Automation `ElementFromPoint` gives the control name, control type, automation ID, and `IsPassword`. Lookups usually take about 2 ms but can be much slower, so they never run on the hook or capture threads. Query the window when it comes to the foreground, so browsers build their accessibility data before the first click.
- **Session state (PSR-10):** a hidden window (`platform/windows/src/session.rs`) listens for lock, unlock, sleep, wake and a session leaving the console. While away nothing is recorded; on return capture is rebuilt so no frame from before the lock can be labelled 'before the click'.
- **Normalizer:** groups keystrokes into text runs, maps shortcuts to named actions, and drops suppressed input (password fields, paused state).
- **Step builder:** joins one screenshot, one click or text group, and one UIA target into a step block.
- **Redactor:** password-field regions are blacked out in the frame before it is written. Other detections (pattern matching on typed text and on-screen text from UI Automation, with the local Windows OCR engine as a fallback) produce redaction boxes attached to the step.

**As built (M1, 2026-11-06, `platform/windows`):** four threads, `input-hooks → capture → analysis → writer`.
- Capture never waits on anything slower. Frames waiting for analysis or the disk are held to a 200 MB budget; over it, the step is recorded without an image and says so, because a late grab would show the wrong moment.
- UI Automation calls run on a worker with time limits (1.5 s lookups, 3 s scans). A worker that hangs is abandoned and replaced; its answer is unknown.
- The capture thread records the visible windows on the frame's monitor right after each grab, skipping windows fully hidden behind an opaque window. Layered, click-through, DirectComposition and shaped windows never hide anything, and neither do the foreground and clicked windows or menus, because the frame predates the click. Menus, tooltips and shadows cannot hold password fields and are not scanned.
- Analysis scans that list, plus windows that appeared since. A listed window that has since changed (minimized, hidden, moved, closed) uses its own scan from the same position within 60 s; with none, the screenshot is withheld. Every frame also blacks out every field remembered from the last 10 s, and every field of any window scanned on that monitor in the last 60 s (for windows closed by a shortcut or pages that navigated away). Every visible window is rescanned at start and every 5 s. Too much black is the accepted cost.
- `window_scan` is `Yes` only if every element reported as a password lies inside a found field. A frame is redacted before it leaves analysis, and written only once its step is final; withheld and discarded frames are dropped in memory.
- A frame not proven to predate its event (GDI fallback, history flushed) is kept but the step says "This screenshot may show the moment after the action."
- The hook watchdog compares the system's last-input time with the last hook callback and reinstalls the hooks if Windows removed them.
- Recording refuses to start unless the thread is per-monitor DPI aware.
- Scope (A6, Q13): the whole screen, one monitor (by device name, which survives a resolution change), or one window. The capture thread drops clicks and keys outside it. A window counts its owned dialogs and its own title-less pop-ups (menus, drop-downs). For one window, the frame is cut to the bounding box of the window and the clicked pop-up or dialog, and the rest of that box is painted grey, before the window snapshot, so the scan and the writer only see that area. The desktop and taskbar never count as part of a File Explorer window, although they share its process. Windows on top of the chosen one still show, and the cut uses the window's position at handling time (a few ms after the frame). If the window closes or the monitor disconnects, nothing more is recorded and the panel says so.

**Known Windows limits to document:**
- UIPI: a recorder running as a normal user is not given input sent to elevated windows, so clicks there make no step (measured 2026-11-08, `research/m1-results.md` 'Admin windows and UAC'). Decided in Q31: skip them and warn (the panel warns while an admin window is in front, `platform/windows/src/elevation.rs`), and support starting BabelSteps as administrator to record them. `uiAccess` with a signed binary in a protected folder, as Steps Recorder uses, is not chosen for now.
- The UAC prompt and the Ctrl+Alt+Del secure desktop cannot be captured by design.
- Some protected content (DRM video, some games) may render black in screenshots.

## 4. Document model

The document is the source of truth. Capture produces it, editing changes it, and exports render it.

```
Recording
  id, title, created_at, updated_at, settings snapshot
  blocks: [ Block ]              // ordered

Block (union)
  - StepBlock     { id, action: click | text | shortcut | comment | manual,
                    title, description (rich text), screenshot_ref,
                    redactions: [rect], target: { name, control_type, bounds }?,
                    captured_at }
  - TextBlock     { id, rich_text }           // notes, intro, summary
  - HeadingBlock  { id, level, text }
  - ImageBlock    { id, image_ref, caption }
  - ListBlock     { id, items: [rich_text] }
```

**Storage:** each recording is a folder bundle, for example `MyRecording.bsrec/`:
- `recording.db` (SQLite): blocks, settings, and metadata.
- `images/`: screenshots, one per capture: lossless PNG for UI screens, JPEG (quality 85, no chroma subsampling) when much of the screen is photo-like, both at full resolution with no metadata (`core/src/screenshot.rs`, PSR-1). The writer falls back to the fastest PNG setting when frames queue up, so a burst of clicks is not dropped. Print exports (HTML, PDF) shrink images wider than 2000 px to 1600 px (narrower ones keep their size: a mild shrink made PDFs larger) and use the fastest PNG, because WebView2 re-compresses PDF images itself: pixel count sets the PDF size. Password-field regions are burned in at capture. Other redactions are stored as metadata and applied at export, unless strict mode burns them in at capture (see the Privacy doc §3).

A single-file export (`.bsrec` as a zip) can be added later.

## 5. Exporters

- **Markdown:** our own serializer from the block model. Output is `document.md` plus `images/`. Optional single-folder zip.
- **PDF:** render the document to HTML with a print stylesheet, then print to PDF through the platform webview (WebView2 `PrintToPdf` on Windows, `WKWebView` on macOS). This keeps dependencies low and styling in CSS. Pages are US Letter, with an optional header and footer holding only the title and page number (Q7), so a local file path can never appear in the output. Alternative to evaluate later: Typst or a pure-Rust PDF writer if the webview route is limiting.
- **HTML:** the same HTML pipeline, output as a self-contained file.

## 6. Cross-platform seams

| Capability | Windows (v1) | macOS (M4) | Linux (M5) |
| --- | --- | --- | --- |
| Screenshots | Windows.Graphics.Capture | ScreenCaptureKit | X11 XGetImage; Wayland via `xdg-desktop-portal` (user prompt) |
| Global input | Low-level hooks | CGEventTap | X11 XRecord or XInput2. Wayland: generally not allowed, so this is limited |
| Element names | UI Automation | Accessibility (AXUIElement) | AT-SPI |
| Permissions | None extra | Screen Recording, Accessibility, Input Monitoring | Varies by session type |

**Biggest cross-platform risk:** Wayland. Global input capture is intentionally restricted by the compositor, so Linux may need an X11 session or a compositor-specific approach. Linux is in the backlog (decision 2026-11-02): we keep the backend interfaces clean so a port stays possible, but do no Linux work until after macOS.

## 7. Repository layout (proposed, after approval)

```
BabelSteps/
  core/            Rust crate: model, normalizer, storage, exporters (no platform code)
  platform/
    windows/       Rust crate: Windows backends
    macos/         later
    linux/         later
  app/             Tauri shell + web UI
  tests/           fixtures (sample recordings), golden-file export tests
  docs/
  research/
```

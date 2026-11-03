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
- **Screenshots:** Windows.Graphics.Capture for the chosen monitor or window, with GDI `BitBlt` as a fallback. Capture work runs off the hook thread.
- **Element identity:** UI Automation `ElementFromPoint` gives the control name, control type, automation ID, and `IsPassword`. These calls can be slow, so they run asynchronously and never on the hook thread.
- **Normalizer:** groups keystrokes into text runs, maps shortcuts to named actions, and drops suppressed input (password fields, paused state).
- **Step builder:** joins one screenshot, one click or text group, and one UIA target into a step block.
- **Redactor:** password-field regions are blurred in the frame before it is written. Other detections (pattern matching on typed text and on-screen text from UI Automation, with the local Windows OCR engine as a fallback) produce redaction boxes attached to the step.

**Known Windows limits to document:**
- UIPI: a non-elevated recorder cannot see input sent to elevated windows. Options are to run elevated (not recommended), or to show a warning when an elevated window is active.
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
- `images/`: PNG screenshots (one per capture, named by block ID). Password-field regions are burned in at capture. Other redactions are stored as metadata and applied at export, unless strict mode burns them in at capture (see the Privacy doc §3).

A single-file export (`.bsrec` as a zip) can be added later.

## 5. Exporters

- **Markdown:** our own serializer from the block model. Output is `document.md` plus `images/`. Optional single-folder zip.
- **PDF:** render the document to HTML with a print stylesheet, then print to PDF through the platform webview (WebView2 `PrintToPdf` on Windows, `WKWebView` on macOS). This keeps dependencies low and styling in CSS. Alternative to evaluate later: Typst or a pure-Rust PDF writer if the webview route is limiting.
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

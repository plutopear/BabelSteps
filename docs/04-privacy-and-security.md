# Privacy and Security

Privacy is a core product promise, not a feature. These guarantees are checked automatically, and any violation blocks a release. Section 8 lists what the automatic checks cover and what they cannot.

## 1. Guarantees

1. **No outbound network from the app.** The core build depends on no networking crates. CI checks the dependency tree and runs a test that fails if the app opens a socket during a recording and export session.
2. **No telemetry or crash uploads.** Crash logs are written locally, and the user can view or delete them.
3. **No accounts, sign-in, or license server.** The app works fully offline from install.
4. **Updates are opt-in and manual** if they exist at all. A later auto-update feature must be off by default and must not send recording data.
5. **Data stays where the user puts it.** Recordings live in a user-chosen folder (default: `%LocalAppData%\BabelSteps\`). Uninstalling does not silently delete recordings, and the library shows exactly where they are.
6. **Exports contain only what is visible.** Strip image metadata (EXIF, PNG text chunks). Do not embed machine names, user names, or window titles unless the user chose to show them.

## 2. Sensitive input handling

| Concern | Design |
| --- | --- |
| **Keystrokes** | Keystroke capture is **off by default** and turned on per recording by an explicit toggle. While it is on, the indicator says so ("Keystrokes recording"). See Q4. |
| **Password fields** | Password field text is never stored, and the step reads "Enter password in *field name*" with no value. No single signal is trusted: in M0, Edge's first lookup at a password field wrongly said it was not one. A field counts as a password if the element reports `IsPassword`, the point is inside a field found by the window scan, or the focused element reports it at keystroke time. If the check fails or cannot finish, typed text is dropped and the step's screenshot is withheld, with the step marked "Screenshot withheld: could not check for passwords." A missing image is recoverable; a leaked password is not. Fields that report none of these (some custom controls) are a known gap; we document it. |
| **Secrets typed elsewhere** | Typed text is shown in the document. Users can delete or redact steps before export. Recording with a visible warning reduces accidents. |
| **Screenshots of sensitive content** | Manual blur or black-out tools (B7) and automatic redaction (B12–B14, section 3). Turning keystroke capture off does not hide typed text that is visible on screen, so the UI must not imply that it does. |
| **Window titles in element names** | A click on empty window space, a title bar, a web page background or a tab hits an element named after the window, page or file title. Since 2026-11-08 the core drops the names of window, title bar, document, pane and tab item elements before anything is saved (unit tested, and checked live on our own test window on 2026-11-08). Known gap: taskbar and task-switcher buttons and the caption text in newer title bars are also named after the window title and are not caught. |
| **Full window titles in steps (A3 opt-in)** | Off by default: steps name the app, and no window title is read. The setting "Show full window titles in steps" (Settings, Step names) names the window in click steps, as Steps Recorder does, and saves the title with the step in `recording.json`. Titles are scrubbed by the same PII rules as element names. A click step drops its title when any password signal says yes or the check could not finish (fail closed), and typing steps never read or keep one. It takes effect at the next recording start. When the setting is off, the core also drops any title it is handed, so the off state does not depend on the platform code alone (unit tested). The Settings text warns that titles often show file names, page titles and customer names. |
| **Clipboard** | Not captured in v1 (non-goal). |
| **Pause and panic stop** | A global pause hotkey and a panic-stop hotkey work even when the app window is hidden. |
| **Recording indicator** | Always visible in the tray and on the capture target. Can be hidden only in a clearly labeled setting. |

## 3. Automatic redaction (decided 2026-11-02)

Encryption at rest is **dropped** (not planned). Instead, the app limits how much sensitive data is captured in the first place. All detection runs locally.

**What is detected**
- Password fields (UI Automation `IsPassword`).
- Typed text matching sensitive patterns: email addresses, phone numbers, credit card numbers (Luhn-checked), national ID formats such as US SSN, IP addresses, API keys and tokens (known prefixes and high-entropy strings), and private-key blocks.
- On screenshots: the same patterns found in on-screen text. Text comes first from UI Automation element text and bounds, and second from the built-in local Windows OCR engine for anything UI Automation cannot read. No cloud OCR.
- A user-editable list of custom words and regexes (for example, internal hostnames or customer names).

**What happens**
- **Password fields:** every password field in any visible window on the captured screen is blacked out at capture, on every step, and the original pixels are discarded. (The M0 spike covers only the clicked window. Scan time across all windows is not measured yet.) The original is never written to disk. Solid black is used because blur can leave the length readable. Known limits: a field revealed with a "show password" button may stop reporting as a password, and a field that disappears right after the click may be missed by the scan (not yet tested).
- **Other detections:** suggested redaction boxes are shown on the screenshot and in the text, applied by default, and reviewed in the pre-export checklist (E5). The user can accept, remove, or add boxes.
- **Strict mode (setting):** burn in all redactions at capture time, so unredacted pixels and text never reach disk.
- Redacted text in typed steps is replaced by a placeholder, for example `[email]`, `[card number]`. The raw value is not stored in strict mode.
- Exports are always generated from redacted data. Burned-in redactions in exported images cannot be undone.

**Limits to be honest about:** pattern and OCR detection will miss some data and sometimes flag harmless text. The pre-export review step exists for that reason. The app should never claim it guarantees that exports are free of sensitive data.

## 4. Threat model (summary)

| Threat | In scope? | Response |
| --- | --- | --- |
| Accidental capture of passwords or secrets | Yes | Password-field suppression, opt-in keystrokes, redaction, visible indicator |
| Recordings leaked via cloud sync or telemetry | Yes | No network code, no sync in v1, CI network test |
| Stolen laptop or copied recording folder | No (decision 2026-11-02) | Out of scope. Recommend BitLocker. Auto-redaction reduces how much sensitive data is stored |
| Malicious software on the same user account reading keystrokes or screen | No (cannot be prevented by this app) | Documented. Not claimed as a protection |
| Supply-chain compromise of dependencies | Partly | Lock files, minimal dependency set, license and advisory checks in CI |

## 5. Trust signals to build

- Signed installers (code signing) on Windows, so the hook-based capture is not mistaken for malware.
- A plain-language "What this app records and where it goes" page, shipped in the app.
- Open source for the core, if Q8 chooses it. This lets users verify the no-network claim.

## 6. Antivirus and keyboard hooks

Low-level keyboard hooks are the same mechanism some keyloggers use, so antivirus heuristics may flag the app. Mitigations: code signing, documentation of the hook use, submitting builds to vendor false-positive programs, and keeping the hook callback minimal and transparent.

## 7. Native app, no server (decided 2026-11-08)

BabelSteps is one native program. It runs no web server and no client/server pair, and it opens no network connection. The window shows the built-in frontend through Tauri's own `custom-protocol`, which is an in-process handler, not a TCP port.

Development vs release: during development (`npm run tauri dev`) the window may load the Vite dev server on localhost:1420; that is a development tool on the developer's machine and never ships. Every release build must be self-contained, and the guards below enforce that.

Guards:
- `build.rs` in `app/src-tauri` refuses a release build that does not embed the frontend. A plain `cargo build --release` would point the window at the development server (`localhost:1420`) and show "localhost refused to connect". Build the app with `cargo build --release -p babelsteps-app --features custom-protocol` (the executable is then under that target folder's `release`).
- `.claude/skills/babelsteps-check/scripts/no-network.ps1`, run by the `babelsteps-check` skill, fails if the dependency tree for Windows, macOS or Linux contains hyper, reqwest, mio, socket2, h2, a TLS or HTTP client crate, or a Tauri http, shell, opener, updater or websocket plugin; if tokio has its `net` feature on; if a capability grants more than the folder and save pickers; or if the config opens a remote URL. Checked 2026-11-08: passes (322 crates). Tauri itself pulls in reqwest only for Android and iOS, which we do not build, so those targets are not checked.
- Checked 2026-11-08 on a running, idle release app (one snapshot, `research/m1-results.md`): no BabelSteps or WebView2 process held a TCP or UDP socket, other than the test's own local debugging port. Checked again 2026-11-08 during a recording: none. Not yet checked: during an export, and which local pipes WebView2 uses.
- Every WebView2 window, main and print, is hardened before use (`harden` in `app/src-tauri/src/pdf/windows.rs`). The SmartScreen reputation lookup is off, browser context menus are off, devtools are off and the status bar is off. The settings are read back, and the window fails closed if any of the first three did not apply:
  - The main window is built hidden, and it is shown only once the hardening is confirmed.
  - A refused window stays hidden. The user is told, and the window is destroyed.
  - The print window refuses to print.

  A WebView2 runtime too old to offer these settings therefore cannot open the window. Checked live 2026-11-08: right-clicking the start-up window and a window rebuilt from the tray opened no browser menu, and the same check on our own test window did find one (`research/m1-results.md`). The first page load still starts before the hardening is applied, but on a hidden window that shows only our bundled page.
- Closing to the tray (Q21) destroys the window, so no WebView2 process runs while BabelSteps sits in the tray. Checked live 2026-11-08: one process, no sockets.

## 8. How the privacy checks run (added 2026-11-08)

GitHub Actions (`.github/workflows/ci.yml`) runs these on every pull request and every push to `main`. The `babelsteps-check` skill runs the first three on the owner's PC.

| Check | What it catches | Where |
| --- | --- | --- |
| Unit and golden-file tests | Exports that differ from the reviewed output, metadata left in images, withheld screenshots exported | `cargo test --workspace` (Windows) |
| Source and config scans | Socket or HTTP APIs in Rust code (`TcpStream`, `UdpSocket`, WinSock, WinHTTP, `Windows.Web.Http`); `windows` crate networking features; `fetch`, `WebSocket` or any `http(s)://` address in the frontend; a content security policy that names any outside source; Tauri permissions beyond the folder and save pickers | `checks/tests/privacy.rs`, part of `cargo test` |
| No-network dependency check | Networking crates, Tauri plugins that can reach the network, tokio's `net` feature, for Windows, macOS and Linux builds | `no-network.ps1` (Windows) |
| Runtime socket test | Any network socket opened while the core tests build, save, reload, redact and export recordings | `scripts/ci/no-socket-at-runtime.sh` (Linux, under `strace`) |

Every scan and the runtime test first prove they can fail: each scanner is tested on a planted leak, and the runtime test checks that it catches a program that opens a socket before it trusts a pass.

Security and dependency checks (`.github/workflows/security.yml`, `codeql.yml`, `.github/dependabot.yml`, `deny.toml`):

| Check | What it catches | When |
| --- | --- | --- |
| cargo-deny | Rust crates with published vulnerabilities, licences outside the allow list, banned networking crates (a second, independent no-network check), crates from anywhere but crates.io | Every pull request, push to `main`, and weekly |
| npm audit | High or critical vulnerabilities in the frontend's packages | Same |
| gitleaks | Passwords, tokens and keys committed anywhere in the history | Same |
| Dependabot | Opens pull requests for Rust (Mondays) and npm (Tuesdays) updates, at most 3 open at a time per ecosystem, and monthly for Actions. TypeScript major updates are ignored until svelte-check supports TypeScript 7. CI must pass before merging, so an update that adds network code is caught | Weekly |
| CodeQL, dependency review | Code-level security bugs in Rust, TypeScript and the workflows; vulnerable or badly licensed dependencies added by a pull request | Only while the repository is public (free there, paid on private). CodeQL skips pull requests that change only lock files or notes, and runs on `main` are never cancelled |

Ignored advisories are listed in `deny.toml` with a reason and must be removed when the fix lands.

Not covered yet:

- **Windows capture at runtime.** Hooks, screen capture and UI Automation need a real desktop, so no automatic runtime socket test covers them. The source scan and dependency check still apply to that code.
- **WebView2's own processes.** The window is drawn by Microsoft's WebView2, which runs in separate processes that the app does not control. The content security policy stops our page from reaching the network; it does not govern what WebView2 itself does.
- **Blocking a release.** Checks run on every pull request, but GitHub only lets a failed check block merging on a private repository with a paid plan. The repository went public on 2026-11-08. Another thread reports that the `main` ruleset now requires the Windows, Linux and Rust dependencies checks and that the repository rejects Actions not pinned to a commit SHA; the SHA pinning is visible in the workflow files, the ruleset itself was not checked from here.

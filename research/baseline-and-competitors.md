# Baseline and Competitors

Working notes. Items marked *(verify)* have not been confirmed on a real machine yet. They become confirmed facts during M0.

## 1. Steps Recorder (parity baseline)

**Installed binary on the planning machine**
- Path: `C:\Windows\System32\psr.exe`
- File version: 10.0.26100.8521 (Windows 11, build 26100 line)
- Product name: Microsoft Windows Operating System. File description: Steps Recorder.

**Verified by recording and inspecting output (2026-11-02)**
- **Output:** a `.zip` holding one MHTML file (`Recording_YYYYMMDD_HHMM.mht`). The MHTML holds one main HTML page, 62 small HTML fragments, a stylesheet, and one JPEG per step.
- **Images:** full monitor at native resolution (5120×1440), about 390 KB each as JPEG. A 31-step recording was 16.9 MB as MHTML and 10.3 MB zipped.
- **Screenshot limit:** the default kept only the last 25 screenshots of a longer session. `/maxsc 100` raised it.
- **Click marker:** a bright green rectangle around the clicked element, not a marker at the click point.
- **Timing:** the image shows the state **after** the click. 21 of 21 clicks on a window that reacts on press showed the reacted state.
- **Step text:** `User left click on "Save (button)" in "Window title"`, and the same for right click. Each step has a number and timestamp.
- **Typed text:** never recorded. The file says so itself ("Passwords or any other text you typed were not recorded, except for function and shortcut keys"). A typing burst becomes one step, `User keyboard input on "User name: (edit)" in "Window" [...]`. The typed strings appear nowhere in the file. Modifier keys are shown, for example `[Ctrl-Ctrl]` and `[... Win]`. How it words a shortcut such as Ctrl+A was not established: one sent during a typing burst did not get its own step.
- **Document layout:** an intro with a privacy reminder, links to review steps, a slide show, and additional details. Each step has Previous and Next links.
- **Summary line:** `Recorded Steps: 31, Missed Steps: 0, Other Errors: 0`.
- **Additional details fields:** Recording Session, Recorded Steps, Operating System, Program, UI Elements.
- **Command line:** `/start`, `/output`, `/sc`, `/maxsc`, `/gui`, `/recordpid`, and `/stop` all worked. It must be launched through the shell. Direct process creation fails with error 740.
- **Permissions:** the manifest sets `level="asInvoker"` and `uiAccess="true"`.

**Still unverified**
- Comments, pause, and the settings dialog, which are in its window UI. Launched here with `/gui 0`.
- Second monitor behavior.
- Elevated windows and UAC prompts.
- Whether its window can limit capture to one window. Everything recorded here was full monitor.

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

## 4. Windows code signing (researched 2026-11-02)

Source: research by a delegated agent from Microsoft and vendor pages. Microsoft eligibility and SmartScreen facts are well sourced (pages dated 2026-05 to 2026-10). Non-Microsoft prices come from cached or reseller pages and are **unverified**. Re-check before buying.

| Option | Cost | Who can use it | Notes |
| --- | --- | --- | --- |
| Microsoft Artifact Signing (Azure) | Basic $9.99/month (about $120/yr), 5,000 signatures, $0.005 per extra | Individuals located in the **US or Canada** only; identity validation takes 1 to 20 business days | No hardware token, works in CI. Not Store-eligible. SmartScreen trust builds over time |
| SignPath Foundation | Free | Open-source projects with an OSI license, MFA for the signing team, manual approval per release, and a published signing policy | The Foundation becomes the publisher. Whether individuals can apply is unconfirmed (terms page was a draft) |
| SSL.com individual (IV) certificate | About $129/yr, plus a cloud-HSM fee | Individuals, any country | Price unverified |
| Certum open-source certificate | About €69 | Individuals | Requires a hardware card and reader. Price unverified |
| DigiCert OV via resellers | About $380 to $580/yr | Needs more validation | Unverified. EV (about $400+) no longer bypasses SmartScreen |
| Unsigned | Free | Anyone | Users see "Windows protected your PC" and must click through. Smart App Control may block it. Hook-based utilities are often flagged by antivirus |

**Facts that apply to every paid option**
- Since June 2023, OV/EV keys must be on a hardware token or cloud HSM.
- Certificate validity has been shortened (about 460 days from early 2026 per third-party sources; exact date unconfirmed).
- Changing publisher identity resets SmartScreen reputation, so keep one identity across releases.
- Expect SmartScreen warnings for several weeks after each new release even when signed.
- Sign both the app executable and the installer.

**Tauri 2 integration:** local certificates use `bundle.windows.certificateThumbprint`, `digestAlgorithm`, and `timestampUrl`. Custom tools (such as the Artifact Signing CLI) use `signCommand` with `%1` as the file path. The Tauri page's OV guide only covers pre-June-2023 certificates.

**Recommendation:** US or Canada resident: Artifact Signing Basic. Anywhere else: apply to SignPath Foundation first, then SSL.com IV as the fallback.

**Decision (2026-11-02):** The owner is in the US and does not want to pay. Apply to SignPath Foundation (free) and fall back to unsigned with mitigations. Artifact Signing stays as an option if the owner reconsiders.

**Not yet researched:** whether publishing through the Microsoft Store (which re-signs submitted packages) is a cheaper or free route, and whether a Store package can use low-level input hooks. Worth a short check before M3.

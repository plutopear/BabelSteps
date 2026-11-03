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

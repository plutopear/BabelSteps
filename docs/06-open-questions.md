# Open Questions

These decisions block development. Each has a recommended default, so "go with the defaults" is a valid answer for any question.

## Blocking (answer before M0)

**Q1. Tech stack.** Which option from `03-architecture.md` §1?
- *Recommended:* **A. Rust core + Tauri 2 + web UI.** Smallest footprint, one codebase for all three OSes.
- Alternative: **B. C# / .NET 8 + WinUI 3** if you are more comfortable in .NET and macOS and Linux are not firm plans.
- Also tell me: how comfortable are you with Rust, and with TypeScript for the UI?

**Q2. Meaning of "1:1 parity."** Pick one or more:
- (a) Same workflow and features, with our own output format. *Recommended.*
- (b) Output format compatible with Steps Recorder: we can open and create its `.zip` / `.mht` files.
- (c) Read-only import of existing Steps Recorder recordings.
- This affects A9 and A10 and the file format design.

**Q3. Capture timing.** When a user clicks, should the screenshot show:
- (a) The state just before the click (what the user was looking at). *Recommended for documentation.*
- (b) The state after the click (what the click produced). Steps Recorder style, as far as we know *(verify)*.
- (c) Both, with the "before" image as the step and the "after" as an optional thumbnail. Costs more storage.

**Q4. Keystroke capture default.** Scribe-style keystroke capture is powerful and also the most sensitive feature.
- (a) Off by default, enabled per recording with a visible indicator. *Recommended.*
- (b) On by default.
- (c) Off by default, but enabled once per install with a privacy explanation.

**Q5. Target users and default output.** Who is the main user? Options:
- IT and helpdesk documentation.
- Training and onboarding.
- QA and bug reports.
- General personal use.
This changes default templates, default titles, and whether comments or redaction are prominent.

**Q6. Scope of v1 editing.** How much editing in the document?
- (a) Edit step titles and descriptions, reorder, delete, add notes, and redact. *Recommended for v1.*
- (b) Full rich-text editor with tables, images, and page layout.
- (c) Markdown source as the main editor.

## Important (answer before M2)

**Q7. PDF and Markdown output.**
- PDF page size and layout: Letter and A4 both? Header and footer with title and date? Default margins?
- Markdown: `document.md` plus an `images/` folder *(recommended)*, or inline base64 images in a single file?

**Q8. Licensing and distribution.**
- Open source for the core (e.g., MIT or Apache-2.0) *(recommended if no commercial plan yet, since it supports the no-network verification story)*, or closed source?
- Is "BabelSteps" the final name?
- Is a code-signing certificate budgeted? Windows hook-based capture is sensitive to antivirus flags without one.

**Q9. Platform order after Windows.** macOS next (recommended, since its APIs are more mature for this than Wayland), or Linux first?

**Q10. Encryption at rest.** Include in v1 (P1), or defer?

**Q11. Repository setup.** Initialize a local git repository now? *Recommended: yes.* I will not commit or push without your go-ahead.

## Lower priority (defaults are fine)

**Q12. Retention default.** How many recent screenshots to keep by default for continuous recording. Proposed: unlimited, with a warning at a size threshold. Steps Recorder's default needs verifying (see research notes).

**Q13. Recording scope default.** Full screen, or the active window? Proposed: full screen, with window and monitor choice.

**Q14. Hotkeys.** **Correction (2026-11-02):** the original proposal conflicted with common apps. `Ctrl+Shift+Esc` opens Task Manager, and `Ctrl+Shift+R/P/N` are browser refresh, private window, and incognito. Because these are global hotkeys, they would hijack those shortcuts. New proposed defaults use three modifiers: Start/Stop `Ctrl+Alt+Shift+R`, Pause `Ctrl+Alt+Shift+P`, Add note `Ctrl+Alt+Shift+N`, Panic stop `Ctrl+Alt+Shift+X`. All are user-configurable, and the app warns if a hotkey is already taken.

**Q15. Browser extension.** Keep out of v1 (recommended), or include a local-only extension for better web element names?

## Answered

Decisions from 2026-11-02:

- **Q1 Stack:** Rust core + Tauri 2 + web UI. The owner is not a developer and wants cross-platform from day one to avoid a later rewrite, so Claude owns the technical choices.
- **Q2 Parity:** Same workflow and features with our own file format. No `.mht` compatibility or import for now.
- **Q3 Capture timing:** Screenshot of the state just before the click is the step image.
- **Q4 Keystrokes:** Off by default, enabled per recording with a visible indicator. Password fields are never recorded.
- **Q5 Audience:** IT/helpdesk first, end-user training second. See requirements section E.
- **Q6 Editing:** Block-based WYSIWYG editor with edit titles, descriptions, reorder, delete, notes, and redaction. Markdown compatibility is important: documents round-trip to and from Markdown.
- **Q7 Licensing/name:** Open-source core (permissive license, final choice at first public release). Name stays "BabelSteps". Code signing: deferred to M3, see notes in the roadmap.
- **Q8 Platforms:** Windows, then macOS. Linux is in the backlog; platform interfaces stay clean so a port remains possible.
- **Q12, Q13, Q15:** Proposed defaults accepted (unlimited retention with a size warning, full-screen default, no browser extension in v1).

- **Q10 Encryption at rest:** Dropped. Replaced by automatic redaction of passwords and PII (requirements B12–B14, Privacy doc §3).
- **Q11 Git:** Yes, local repository.
- **Q14 Hotkeys:** Use the three-modifier defaults, and make every hotkey remappable.
- **Code signing:** Research requested; results to be summarized in the roadmap before M3.

- **Q16 Code signing:** Owner is in the United States and does **not** want to pay for a certificate. Plan: apply to SignPath Foundation (free for open source) before the first public release, and make unsigned distribution workable as the fallback (see roadmap M3). Paid options such as Microsoft Artifact Signing (about $120/yr, available to US individuals) stay on the shelf in case the owner changes their mind.

Nothing is blocking M0 now.

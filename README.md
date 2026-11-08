# BabelSteps

A modern, fast, local-first successor to Windows Steps Recorder. It starts with 1:1 feature parity, then adds Scribe-style capture of clicks and keystrokes into an editable, readable document that exports to PDF and Markdown.

**Status:** in development, with no release yet. The Windows app records, edits and exports, and M1 (Steps Recorder parity) is in progress; see `docs/05-roadmap.md`. Stack decided (Rust + Tauri 2); audience is IT/helpdesk first, then end-user training. Decisions are recorded in `docs/06-open-questions.md`, and spike results in `research/m0-spike-results.md`.

## Principles

- **Local only.** No accounts, no cloud, no telemetry, and no network calls from the app.
- **Fast and light.** Native capture, small installer, low idle memory and CPU.
- **Easy.** One hotkey to start, one to stop, and the output is a document you can read and edit.
- **Windows first, then macOS and Linux.** Platform-specific code sits behind small interfaces so the core stays shared.

## Layout

| Path | Purpose |
| --- | --- |
| `docs/02-requirements.md` | Feature requirements: Steps Recorder parity, Scribe-style additions, targets |
| `docs/03-architecture.md` | Stack options, recommended stack, capture pipeline, data model |
| `docs/04-privacy-and-security.md` | Privacy guarantees, keystroke and password handling, threat notes |
| `docs/05-roadmap.md` | Milestones and exit criteria |
| `docs/06-open-questions.md` | Decisions needed before development (start here) |
| `research/baseline-and-competitors.md` | What we know about Steps Recorder and Scribe, and what still needs hands-on verification |
| `research/m0-spike-results.md` | Measured results from the Windows capture spikes |
| `core/` | Document model and Markdown export, shared across platforms |
| `spikes/win-capture/` | Throwaway Windows capture experiments |

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

To report a security or privacy problem, see [SECURITY.md](SECURITY.md). To contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

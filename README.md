# BabelSteps

A modern, fast, local-first successor to Windows Steps Recorder. It starts with 1:1 feature parity, then adds Scribe-style capture of clicks and keystrokes into an editable, readable document that exports to PDF and Markdown.

**Status:** Planning (Phase 0). No application code yet. Stack decided (Rust + Tauri 2); audience is IT/helpdesk first, then end-user training. Decisions are recorded in `docs/06-open-questions.md`. M0 (spikes) is next.

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

Code will go in `src/` (or per-platform folders) after the plan is approved.

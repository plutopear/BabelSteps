# BabelSteps

A local-only successor to Windows Steps Recorder. It turns clicks and keystrokes into an editable document that exports to PDF and Markdown. Rust core + Tauri 2. Windows first, then macOS; Linux is in the backlog.

## Working with the owner

The owner is not a developer. They decide product direction, and Claude makes the technical decisions. Report in plain language: what works, what doesn't, and what they need to decide. Say what was not tested. Ask only about product choices, spending, or actions on their machine that they would notice.

## Where things live

Read the section you need, not the whole file. Start a session with the `babelsteps-status` skill.

| Need | Look in |
| --- | --- |
| Current milestone, next tasks | `docs/05-roadmap.md` |
| Decisions already made, open questions | `docs/06-open-questions.md` (do not re-ask what is answered) |
| What to build, with IDs like A2, B12, D6 | `docs/02-requirements.md` |
| Design, threads, data model | `docs/03-architecture.md` |
| Privacy promises, redaction rules | `docs/04-privacy-and-security.md` |
| Measured evidence, known gaps | `research/m0-spike-results.md` |
| Steps Recorder facts, code signing options | `research/baseline-and-competitors.md` |
| Document model, Markdown export | `core/` (shipping code) |
| Windows experiments | `spikes/win-capture/` (throwaway: reference it, do not ship it) |

## Rules from what M0 proved

Each of these exists because the simpler approach failed in testing. The evidence is in `research/m0-spike-results.md`.

- **Screenshots show the state before the click.** Capture runs continuously, with a spaced frame history. Capturing when the click arrives is wrong every time.
- **Hook callbacks only timestamp and queue.** All input on the machine waits on them.
- **Analysis never delays capture.** UI Automation and password scans get their own thread; a scan took about 270 ms on a large page.
- **Password detection uses three signals and fails closed:** the element, the window scan, and the focused element. Edge's first lookup is wrong. If the check fails, drop the typed text.
- **Redact before writing to disk.** Password pixels never reach the disk.

## Product rules

- No network code: no networking crates, telemetry, or accounts. This is the product's main promise.
- `core/` holds no platform code. Platform code goes behind the backend traits in `docs/03-architecture.md`.
- Never claim an export is free of sensitive data. Detection misses things, so the user reviews before exporting.

## Testing on this PC

This PC is also the owner's daily machine, and they may be using it during a test.

- Warn before anything that injects input or takes focus. Use the `capture-selftest` skill, which has the safety rules.
- Do not view full-screen screenshots, or copy window titles or screen content into docs or chat. They can show private things. Crop to our own test window.
- Delete test recordings when done. `recordings/` is ignored by git.

## Verifying work

M0's first test passed 12 of 12 checks while hiding real bugs. When everything passes, ask what the test could not have caught. Check results in the real output file, not in memory. Make sure a check can fail. Give timings with their conditions, and label guesses as guesses.

## Saving context

- Use the project skills instead of raw commands: `babelsteps-status`, `babelsteps-check`, `capture-selftest`, `psr-baseline`, `windows-api-lookup`, `record-decision`.
- Never read `windows` crate source files whole. They are huge. See `windows-api-lookup`.
- Delegate work that produces a lot of noise, and keep only the conclusion: `test-runner` for long builds, `diagnostics-triage` for logs, `doc-researcher` for web research, `repo-scout` for finding code, `code-reviewer` after a change. Handle small tasks directly, because each subagent starts cold.

## Environment

Windows 11, PowerShell 5.1 (no `&&`). If `cargo` or `node` is not found, the shell started before they were installed:

```powershell
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
```

Install tools with winget when a package exists; installs the work needs are already approved. `python -I` ignores `PYTHON*` environment variables, so set stdout encoding inside the script.

## Git

Work on a branch, not `main`. Commit finished work. There is no remote yet. Merge to `main` only when the owner asks.

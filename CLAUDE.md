# BabelSteps

A local-only successor to Windows Steps Recorder. It turns clicks and keystrokes into an editable document that exports to PDF and Markdown. Rust core + Tauri 2. Windows first, then macOS; Linux is in the backlog.

## Working with the owner

The owner is not a developer. They decide product direction, and Claude makes the technical decisions. Report in plain language: what works, what doesn't, and what they need to decide. Say what was not tested. Ask only about product choices, spending, or actions on their machine that they would notice.

## Where things live

Read the section you need, not the whole file. Start a session with the `babelsteps-status` skill.

| Need | Look in |
| --- | --- |
| Current milestone, next tasks | `docs/05-roadmap.md` |
| Exit criteria for each milestone | `docs/07-definition-of-done.md` |
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
- **Password detection uses three signals and fails closed:** the element, the window scan, and the focused element. Edge's first lookup is wrong. If the check fails or times out, drop the typed text and withhold the screenshot. The spike does not do this yet, so do not copy its redaction code.
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

## When to use what

Use these without being asked. They keep long output out of the conversation and apply the rules above.

| When | Use |
| --- | --- |
| Starting a session, after a context reset, or asked what's next | `babelsteps-status` skill |
| After any Rust change, and before every commit | `babelsteps-check` skill |
| After changing hooks, capture, UI Automation, redaction, or the step pipeline | `capture-selftest` skill (warn the owner first) |
| After changing the app's recording wiring, tray, window, or settings | `capture-selftest` skill's live app tests: `live-app-test.ps1`, `live-tray-test.ps1` (owner's OK each run) |
| A self-test failed, its numbers need explaining, or two runs need comparing | `capture-results-analyst` agent |
| Before committing changes to capture, redaction, storage, logging, export, dependencies, or Tauri permissions | `privacy-auditor` agent. Fix any FAIL before committing |
| Calling a Windows API, or a `windows` crate compile error | `windows-api-lookup` skill. Never read that crate's source files whole |
| A question about how Steps Recorder behaves | `psr-baseline` skill (check the research doc first) |
| The owner decides something, or a test changes what we believed | `record-decision` skill, in the same turn |
| Scoped UI work under `app/` | `tauri-ui-implementer` agent |
| After changing exporters, the document model, or image handling | `export-verifier` agent |
| A scoped, clearly specified change outside capture and redaction | `standard-implementer` agent, or `cheap-implementer` for mechanical edits |
| Long or noisy output | `test-runner` for builds, `diagnostics-triage` for logs |
| Web or documentation research | `doc-researcher` agent |
| Finding code across many files | `repo-scout` or `Explore` agent |
| After a large change, to catch general bugs | `code-reviewer` agent; `security-reviewer` for anything beyond the privacy checks |

Do capture, hook, and redaction design yourself, because they depend on the evidence above. Do small tasks directly too: each subagent starts cold and has to re-read what you already know. Agents report back to you, so relay what matters to the owner.

## Environment

Windows 11, PowerShell 5.1 (no `&&`). If `cargo` or `node` is not found, the shell started before they were installed:

```powershell
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
```

Build the release app with `cargo build --release -p babelsteps-app --features custom-protocol --target-dir target/app-release`. Without the feature, the window shows "localhost refused to connect".

Install tools with winget when a package exists; installs the work needs are already approved. `python -I` ignores `PYTHON*` environment variables, so set stdout encoding inside the script.

## Git

Work on a branch, not `main`. Commit finished work. The remote is `plutopear/BabelSteps` on GitHub; changes reach `main` through pull requests. Merge to `main` only when the owner asks.

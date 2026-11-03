# M0 Spike Results (2026-11-02)

Code: `spikes/win-capture` (throwaway) and `core` (kept). To re-run: `cargo run -p win-capture --release -- selftest`. It also has `uia-probe`, `psr-baseline`, and `record` modes.

**Test machine:** Windows 11, one 5120×1440 monitor at 100% scaling, 24 logical CPUs. Results are from a release build.

## Summary

| Question | Answer |
| --- | --- |
| Can we watch clicks and keys system-wide without slowing input? | **Yes.** Mean 1.6 µs per event, max 96 µs, no drops. |
| Can we capture the screen as it was before the click? | **Yes, with a frame history.** 100 of 100 trials, including under full CPU load and a forced race. On-demand capture got 0 of 30. |
| Can we tell what was clicked? | **Yes.** About 2 ms, with good names in classic, modern, and browser apps. |
| Can we keep password fields out of screenshots? | **Yes for the tested cases**, verified on the PNG files on disk. It needs more than one signal; see Findings 2 and 3. |
| Does click → screenshot → step → Markdown work end to end? | **Yes.** 134 clicks, 134 steps, 134 images, 0 errors. |

## Measured results

Final self-test run, 134 synthetic clicks on a private test window. Real input from the person at the machine was ignored.

| Measure | Result | Target |
| --- | --- | --- |
| Hook callback | mean 1.6 µs, max 96 µs across clean runs | No target; Windows' timeout is far higher |
| Key burst | 5000 of 5000 presses, 0 dropped | No drops |
| Click to pixels in memory | p50 9.4 ms, p95 11.2 ms | D6: under 150 ms ✅ |
| Same, all CPU cores busy | p50 10.6 ms, p95 11.3 ms | D6 ✅ |
| UI Automation lookup | p50 2.2 ms, p95 24 ms, max 55 ms | None |
| PNG encode and write (off the capture path) | p50 26 ms, p95 55 ms | None |
| CPU, static screen | 0.4 to 0.8% of **one** core | D2: under 5% ✅ |
| CPU, window repainting ~60 times a second | 2.0 to 3.5% of one core | D2 ✅ |
| Memory while recording | 70 MB at start, about 106 MB at end, peak about 160 MB | None; see D7 |

These CPU figures cover this process only. Any extra work the Windows compositor does for capture is not counted.

### Before-the-click accuracy

The test window turns from green to red on mouse press and repaints immediately, the hardest realistic case. The image is sampled at a fixed background point.

| Method | Shows state before click |
| --- | --- |
| Frame history, normal | 50 of 50 |
| Frame history, all CPU cores busy | 30 of 30 |
| Frame history, worker forced to wait 40 ms | 20 of 20 (newer frame correctly skipped all 20 times) |
| Capture on demand when the click arrives (GDI) | **0 of 30** |
| Steps Recorder (for comparison) | **0 of 21** |

### UI Automation in real apps

No input was sent to these apps during the scans. In a separate step, the Edge test pages received one click on their password field. 192 points per window.

| App | Points with a name | Lookup p50 | Password scan |
| --- | --- | --- | --- |
| Character Map (classic Win32) | 192 | 1.5 ms | 14 ms |
| Calculator (modern XAML) | 166 | 2.1 ms | 9 ms |
| Edge, small page | 191 to 192 | 1.4 ms | 8 ms |
| Edge, 5000-row page | 73 to 83 first pass, then 192 | 1.4 to 1.9 ms | **about 270 ms** |

### Image size

Size depends heavily on what is on the screen. Each row is one full 5120×1440 frame, from two different runs. One earlier run reached 5.6 MB with PNG fast.

| Encoder | Time | Size, busier screen | Size, quieter screen |
| --- | --- | --- | --- |
| PNG fast | 24 to 32 ms | 3.8 MB | 1.9 MB |
| PNG balanced | 175 to 329 ms | 3.4 MB | 1.5 MB |
| PNG high | 453 to 953 ms | 3.4 MB | 1.5 MB |
| WebP lossless | 37 to 44 ms | 3.4 MB | 1.5 MB |
| Steps Recorder JPEG (for comparison) | not measured | about 0.39 MB | |

## Findings that change the plan

1. **Capture must run continuously during a recording.** On-demand capture always shows the result of the click. Each monitor keeps 3 recent frames in GPU memory, at least 25 ms apart, and a click uses the newest frame captured before it. Spacing was added after a busy screen flushed an unspaced history in a few milliseconds (1 wrong image in 20). This also means the GDI fallback cannot show the before state.
2. **Do not rely on the element under the click to detect a password field.** In Edge, the first `ElementFromPoint` read at a password field returned `IsPassword = false` in every run. All 8 repeats returned true, and keyboard focus reported true. The first click into a login field is exactly when this matters. The cause is not known. Use three signals: the element, whether the point is inside a field found by a window scan, and the focused element at keystroke time.
3. **Scan every screenshot for password fields, not just clicks on them.** A filled field shows its length in later screenshots. The first version of the test missed this.
4. **Separate capture from analysis.** A password scan took about 270 ms on a large page. If it runs on the capture thread, the next click waits, so analysis needs its own thread. The scan also runs after the click, so a field that disappears when a page navigates may be missed. This is **not tested**.
5. **Typed text reaches screenshots with keystroke capture off.** The test image plainly shows "hello" in the user name field. The off switch only protects the text log, so on-screen PII detection (B13) matters even for default recordings.
6. **Chromium builds its accessibility data on demand.** On the large page, only 73 to 83 of 192 points had names on the first pass, and all did 1.5 seconds later. Querying the window when it comes to the foreground should avoid poor names on the first click. **Not tested yet.**
7. **Images will be large.** Lossless PNG at this resolution means roughly 190 to 560 MB per 100 steps. Steps Recorder's JPEGs would be about 40 MB. Cropping to the active window is likely the biggest saving but is **not measured**. Exports, especially PDF, will need downscaling. WebP matches PNG balanced on size in a fraction of the time, but some ticketing tools may not display it, so Markdown exports should stay PNG or JPEG.
8. **Steps Recorder uses `uiAccess="true"`.** Its manifest has this permission, which Windows designed to let accessibility tools work with admin windows without running as admin. Per Microsoft's documentation (not tested here), it requires a signed binary installed in a protected folder such as Program Files. This matters for the IT audience, whose admin consoles run elevated. Our unsigned, non-elevated recorder is expected to miss those clicks, which is also **not tested**. See Q17.

## Steps Recorder baseline

Recorded on `psr.exe` 10.0.26100.8521 from its command line, limited to the test process with `/recordpid`. See `research/baseline-and-competitors.md` §1.

## Not tested

| Gap | Why | What it needs |
| --- | --- | --- |
| Multiple monitors and mixed DPI scaling (A7) | Only one monitor, at 100% | A second display or a change to scaling |
| Elevated windows, UAC prompts | Needs an elevated target and an admin prompt | M1 test, with the owner approving the prompt |
| Real people using real apps | Synthetic clicks only, apart from a 2-second run that produced 2 steps from real clicks, whose output was deleted unread | `record` mode, tried by the owner |
| Tauri and WebView2 | Not part of these spikes | First M1 task. D1 (idle memory) depends on it |
| D3, D4, D5 (startup, installer size, PDF export) | Nothing to measure yet | M1 to M3 |
| Lower-end hardware | One fast desktop only | A laptop test before M3 |
| Long sessions, screen resolution changes, sleep and lock | Out of scope for spikes | M1 |
| Other browsers (Chrome, Firefox) | Only Edge was tested | M1. Firefox's accessibility engine is separate from Chromium's |

## Other notes

- **Capture border:** the owner saw no yellow border around the screen during the test runs (reported 2026-11-02). The API accepted `IsBorderRequired(false)` from this unpackaged app. This is a person's observation on one Windows 11 build with one monitor. Other Windows versions are not tested, so re-check once the minimum supported version is chosen.
- **Calculator read 0 points in one earlier run**, and 192 in the final one. The cause was not found, so window selection for modern apps should be re-checked in M1.
- `SystemRelativeTime` averaged 0.3 to 0.9 ms *ahead* of frame arrival, so it is not a pure capture timestamp. That errs on the safe side here: a borderline frame is treated as too new.
- Raising hook and worker thread priority came with better load results (27 of 30 before, 30 of 30 after), but the earlier run was also contaminated by real clicks. The effect is not separately proven.
- Keystrokes are only counted in this spike. Turning them into text, handling dead keys and keyboard layouts, is M2 work and remains unproven.

## Testing on a live desktop

Early test versions moved the real cursor and recorded the owner's real clicks. The test now sends each click as one uninterruptible `SendInput` call that restores the cursor, checks that its own window is under the point, stops typing if focus moves, ignores non-synthetic input, and deletes full-screen images after checking them. The Steps Recorder run still held keyboard focus for about 20 seconds. Keep this in mind for any future test that needs focus: warn first, or use a separate Windows session.

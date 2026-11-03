---
name: psr-baseline
description: Find out how Windows Steps Recorder (psr.exe) really behaves, by driving it against our test window and summarizing its output file. Use whenever a parity question comes up about Steps Recorder's output, step wording, screenshots, or timing, or when comparing BabelSteps with it. Check research/baseline-and-competitors.md first, since the answer may already be recorded.
---

# Steps Recorder baseline

First check section 1 of `research/baseline-and-competitors.md`. Many answers are already there, so a new run may not be needed.

A run takes about 30 seconds. **It holds keyboard focus the whole time, so anything the owner types goes to the test window. Warn them first and wait for a good moment.**

```powershell
.\target\release\win-capture.exe psr-baseline --out recordings\psr
python -I .claude\skills\psr-baseline\scripts\psr_summary.py recordings\psr\psr-output.zip "$env:TEMP\psr-images"
.\target\release\win-capture.exe psr-images "$env:TEMP\psr-images"
```

- `psr_summary.py` prints the file structure, one line per step, the additional-details field names, and whether typed strings appear anywhere.
- `psr-images` prints the color at the test window's background point for each image. Green is the state before a click, red is after. It also writes `crop-first.png`, which shows only the test window.
- The scenario is in `spikes/win-capture/src/psr.rs`. Change it there to test something new.

## Things to know

- `psr.exe` has `uiAccess="true"` in its manifest, so direct process creation fails with error 740 even though it needs no admin rights. Launch it through the shell (`cmd /c start`), as `psr.rs` does.
- `/recordpid` limits recording to our test process, but keystrokes the owner types into the test window still appear as masked `[...]` steps. Do not mistake those for test steps.
- Without `/maxsc`, only the last 25 screenshots are kept.
- Its images are full-screen and can show private things. Look only at the crop, and delete `recordings\psr` and the image folder when done.
- Comments, pause, and settings need its window UI (`/gui 1`), so they cannot be driven by this script. Ask the owner to try those by hand.

Record new facts in `research/baseline-and-competitors.md` with the date.

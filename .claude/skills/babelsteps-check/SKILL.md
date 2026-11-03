---
name: babelsteps-check
description: Build and test the whole BabelSteps Rust workspace and report only errors, failed tests, and a warning count, keeping the long compiler output out of the conversation. Use after any Rust change, before every commit, and whenever asked to build, compile, or run the tests. Does not touch the mouse or keyboard.
---

# BabelSteps check

```powershell
powershell -NoProfile -File .claude/skills/babelsteps-check/scripts/check.ps1
```

It prints errors, failed tests, a warning count, and a pass or fail line. The full output goes to a log file, and the script prints its path. If a failure needs more detail, search that log with Grep rather than reading all of it.

A pass means the code compiles and the unit tests pass. It says nothing about capture behavior on a real desktop; that needs the `capture-selftest` skill.

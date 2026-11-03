---
name: babelsteps-status
description: Get a compact view of where BabelSteps stands (branch, uncommitted work, recent commits, milestone status, open questions, known test gaps) without reading the plan docs. Use at the start of every session, after a context reset, and whenever asked what's next, what's left, or where things stand.
---

# BabelSteps status

Run this instead of reading the docs to get oriented. It prints about 40 lines.

```powershell
powershell -NoProfile -File .claude/skills/babelsteps-status/scripts/status.ps1
```

Then read only the doc section that the next task needs. `CLAUDE.md` says which doc covers what.

If it shows uncommitted work or an unmerged branch, say so before starting anything new, because the owner may not know it is there.

---
name: record-decision
description: Record a BabelSteps decision or new evidence in the right plan documents, changing only the affected lines so the docs stay consistent without re-reading them all. Use whenever the owner answers a question or decides something about the product, a test changes what we believed, or a requirement, target, or milestone changes.
---

# Record a decision

Decisions that live only in a conversation are lost when it ends, so write them down in the same turn. Search for the affected lines and edit those; there is no need to read every doc.

## Where each thing goes

| What changed | Update |
| --- | --- |
| Owner's decision or answer | `docs/06-open-questions.md`: remove the question if it is listed, and add one line under **Answered** with the date |
| What the product must do, or a target | `docs/02-requirements.md`: the row with that ID, or a new row with the next free ID |
| How it is built | `docs/03-architecture.md` |
| A privacy promise or redaction rule | `docs/04-privacy-and-security.md` |
| Scope or order of work | `docs/05-roadmap.md`, including its **Status** line |
| Measured results or test gaps | `research/m0-spike-results.md`, or a new `research/<milestone>-results.md` |
| Steps Recorder or competitor facts | `research/baseline-and-competitors.md` |
| A rule every future session must follow | `CLAUDE.md`. Keep it short: one line, with the detail in a doc |

One decision usually touches two or three of these. To find every mention, Grep for the ID or keyword across `docs` and `research`. A leftover old statement is worse than a missing one.

## How to write it

- Use absolute dates (2026-11-02), because "today" means nothing later.
- Separate what was measured from what is assumed. Mark untested claims "not tested".
- Note why when the reason is not obvious, especially when overruling an earlier decision, so that nobody reverses it by accident.
- Do not copy window titles, file paths from the owner's machine, or screen content into docs. These files are headed for an open-source repository.

Finish by committing the doc changes, then tell the owner in one or two plain sentences what was recorded.

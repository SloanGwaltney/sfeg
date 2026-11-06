---
name: adr
description: Record an Architecture Decision Record (ADR) in docs/adr/.
---

# Description

Creates an ADR from a decision and the reasons behind it that the user provides. Once an ADR is Accepted and committed, it is locked: only dated notes may be appended, and decision changes need a new ADR.

# Steps

1. Get from the user:
   - **Decision**: what is being decided.
   - **Reasons**: why it is being made.
2. If either is missing or unclear, ask the user for it. Do not guess.
3. If this changes a locked ADR (Accepted and committed), write a new ADR that supersedes it. Never edit the old ADR's content.
4. Find the next number in `docs/adr/` (create the folder if needed). Start at `0001`.
5. Write `docs/adr/NNNN-short-title.md` with these sections:
   - `# NNNN. Title`
   - **Date**: today's date.
   - **Status**: Accepted (unless the user says otherwise).
   - **Supersedes**: the ADR(s) it replaces, if any.
   - **Context**: the reasons and background.
   - **Decision**: the decision.
   - **Consequences**: the effects of the decision. Ask the user if they aren't clear.
   - **Notes**: empty to start.
6. If it supersedes an ADR, update the old ADR:
   - Set its **Status** to `Superseded by NNNN`.
   - Append a dated note pointing to the new ADR.
7. Show the user the file path.

## Notes on locked ADRs

- The only allowed edits are appending notes and setting **Status** to `Superseded by NNNN`.
- Append notes under **Notes**, as `- YYYY-MM-DD: note`.
- Notes may only add context, a retrospective, or a pointer to a superseding ADR.
- Notes must never change a decision.

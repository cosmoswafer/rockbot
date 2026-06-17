# Scenarios — System Black Box Boundaries

These documents define RockBot's black box scenarios: what the system must do and how well it must do it, without prescribing internal implementation. Anything observable from outside the box — user-facing behavior, quality attributes, external protocol contracts — belongs here. Internal mechanics (data flow, module wiring, tool dispatch) belong in `_dfd/`.

## Files

| File | Boundary it defines |
|------|---------------------|
| [top-10-user-stories.md](top-10-user-stories.md) | Functional scenario — the 10 behaviors a user can trigger and observe (P0–P2). |
| [non-functional-requirements.md](non-functional-requirements.md) | Quality scenario — measurable limits the system must satisfy at its edges: latency caps, retry budgets, size limits, security invariants, platform constraints. |
| [image-generation-user-stories.md](image-generation-user-stories.md) | Image I/O scenario — how images enter, leave, and round-trip through the system from the user's and LLM's perspective. |

## Relationship to DFDs

Scenarios specify the **contract** at the system boundary. DFDs in `_dfd/` specify the **internal plumbing** that fulfills that contract. When a scenario changes, DFDs must be re-checked; when a DFD reveals new externally-visible behavior, it should be promoted into a scenario here.

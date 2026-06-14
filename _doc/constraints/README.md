# Constraints

Design-time constraints that shape the system's architecture and behavior.

## Files

| File | Purpose |
|------|---------|
| [top-10-user-stories.md](top-10-user-stories.md) | Top 10 user-facing features with priorities (P0-P2). Source of truth for what RockBot does. |
| [non-functional-requirements.md](non-functional-requirements.md) | Quality attributes: performance, reliability, security, maintainability, compatibility, scalability, portability, observability, testability. |
| [image-generation-user-stories.md](image-generation-user-stories.md) | Detailed image generation workflows covering text-only LLMs, vision LLMs, image editing, and the interception pipeline. |

## Relationship to DFDs

Constraints define _what_ the system must do and _how well_ it must do it. DFDs in `_dfd/` define _how_ data moves through the system to satisfy those constraints. When constraints change, DFDs must be updated; when DFDs reveal new necessary behavior, constraints should be updated.

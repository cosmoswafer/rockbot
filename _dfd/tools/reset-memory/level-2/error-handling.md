# Error Handling

## 1. Purpose

Reset cannot fail — it is a pure in-memory operation. The only error case is
a missing `room_id` (programming error, not user-facing).

- References: [Flag-Driven Reset](../flag-driven.md), [Shortcut Fast Path](../shortcut-fast-path.md)

## 2. Diagram

```mermaid
flowchart TD
    TOOL[reset_memory Tool]
    NO_ROOM{room_id present?}
    ERR_PARSE["Error: room_id required"]
    SET_FLAG["Set explicit_reset flag"]

    TOOL --> NO_ROOM
    NO_ROOM -->|"no"| ERR_PARSE
    NO_ROOM -->|"yes"| SET_FLAG
```

# Reset Memory Tool — Shared Structures

## 1. Overview

User-explicit memory reset tool. Two paths converge on the same reset pipeline:

1. **Shortcut path** — literal `!reset` or `!clearmemory` (exact match after
   trimming) is detected in `process_message()` **before the LLM call**.
   Returns a canned reply instantly — no LLM round-trip, no token cost.
2. **LLM tool-call path** — natural-language reset requests ("clear my memory",
   "start fresh") are recognized by the LLM, which invokes `reset_memory`.

Both paths set the `explicit_reset` flag and defer clearing to
`reset_room_if_needed()` (post-reply). **Instantly clears all Layer 1
messages** — no LLM call, no WebDAV write, no summary generation. Zero overhead.

- Upstream: [Agent Loop](../../agent/agent-harness/agent-loop.md) dispatches the tool
  call with room context (`room_id`) auto-injected; also handles the shortcut
  path as an early return in `process_message()`
- Upstream: [Memory Structures](../../memory/memory/structures.md) provides Layer 1
  messages for clearing
- Downstream: [Post-Reply Decision](../../memory/memory-reset/post-reply-decision.md) — shares the same
  `reset_room_if_needed` pipeline

## 2. Tool Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `room_id` | `string` | No (auto-injected) | Room UUID |

No user-supplied parameters needed — the tool operates on the current room's
memory. Room context is injected by the harness before tool execution.

## 3. Data Structures

### Tool Arguments (JSON)

```json
{
    "room_id": "abc123-room-uuid"
}
```

### Tool Result (to LLM)

The tool returns a **lightweight acknowledgment** — reset is deferred until
after the reply is sent (silent — no user-facing notification).

```
Memory reset scheduled. Reply to the user first — memory will be cleared
after your reply is sent.
```

## 4. Integration

### Two paths, one pipeline

Both the shortcut and the LLM tool-call path set the same `explicit_reset`
flag. Actual reset is handled by `reset_room_if_needed()` which is called
**after** the reply is sent (in `main.rs`).

| Phase | Subsystem | Method | Purpose |
|-------|-----------|--------|---------|
| Shortcut | `process_message` | `memory.set_explicit_reset(room_id)` | Detect literal `!reset`/`!clearmemory`, set flag, return canned reply |
| Tool call | `process_message` | `memory.set_explicit_reset(room_id)` | Intercept `reset_memory` tool call, set flag, return ack |
| Post-reply | `main.rs` | `reset_room_if_needed(room_id)` | Checks flag, clears L1 |
| Post-reply | `MemoryManager` | `needs_reset(room_id)` | Includes `explicit_reset` |
| Post-reply | `MemoryManager` | `clear_all_messages(room_id)` | Clear Layer 1 |
| Post-reply | `MemoryManager` | `clear_pressure_flags(room_id)` | Clears all flags |

## 5. Registration

```rust
// main.rs — stub tool, no harness ref needed (intercepted in process_message)
let mut h = harness.lock().await;
h.register_tool(Box::new(ResetMemoryTool::new()));
```

Room context (`room_id`) is auto-injected by the harness before tool execution
via `inject_room_context()`. The tool name is added to the stateful-tools list
alongside `webdav`, `edit_soul`, `save_knowledge`, etc.

### Execution path

When the LLM returns a `reset_memory` tool call, `process_message()` does
**not** call `execute_by_name()` for this tool. Instead it sets the
`explicit_reset` flag on the room and returns a lightweight acknowledgment as
the tool result. The LLM then generates a natural reply using the full
context. After the reply is delivered (in `main.rs`), `reset_room_if_needed()`
detects the flag and clears all Layer 1 messages.

The tool's own `execute()` is never reached in production — it exists solely
for LLM registration. Calling it directly returns an error.

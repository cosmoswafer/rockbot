# Reset Memory Tool — Shared Structures

## 1. Overview

User-explicit memory reset tool. Two paths converge on the same reset pipeline:

1. **Shortcut path** — literal `!reset` or `!clearmemory` (exact match after
   trimming) is detected in `process_message()` **before the LLM call**.
   Clears Layer 1, writes the empty snapshot synchronously, then returns the
   canned reply — no LLM round-trip, no token cost, no restart window.
2. **LLM tool-call path** — natural-language reset requests ("clear my memory",
   "start fresh") are recognized by the LLM, which invokes `reset_memory`.

The tool-call path sets the `explicit_reset` flag and defers clearing to
`reset_room_if_needed()` (post-reply). Both paths **instantly clear all Layer 1
messages** — no LLM call, no summary generation — and persist the empty history
(`write_snapshot(force = true)`) so a restart restores nothing.

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

The shortcut applies the reset (and persists it) immediately, before replying.
The LLM tool-call path sets the same `explicit_reset` flag; its reset is handled
by `reset_room_if_needed()` which is called **after** the reply is sent (in
`main.rs`). Both go through `write_snapshot(force = true)`.

| Phase | Subsystem | Method | Purpose |
|-------|-----------|--------|---------|
| Shortcut | `process_message` | `reset_room_if_needed(room_id)` | Detect literal `!reset`/`!clearmemory`, clear L1 + write empty snapshot **before** the canned reply |
| Tool call | `process_message` | `memory.set_explicit_reset(room_id)` | Intercept `reset_memory` tool call, set flag, return ack |
| Post-reply (tool path) | `main.rs` | `reset_room_if_needed(room_id)` | Checks flag, clears L1, writes empty snapshot |
| Shared | `AgentHarness` | `write_snapshot(room_id, dir, force = true)` | PUTs the empty snapshot with `archive_seq >= 1` (durable reset marker) |
| Post-reply | `MemoryManager` | `needs_reset(room_id)` | Includes `explicit_reset` |
| Post-reply | `MemoryManager` | `check_and_archive` + `prune_archived` | Clear Layer 1 |
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
detects the flag, clears all Layer 1 messages, and writes the empty snapshot
synchronously (`write_snapshot(force = true)`).

The tool's own `execute()` is never reached in production — it exists solely
for LLM registration. Calling it directly returns an error.

# Memory Partitioning

## 1. Purpose

Each room gets isolated two-layer memory. Shared room data (`soul.md`) lives
under the room's WebDAV directory. Bot-internal snapshot data lives under a
separate prefix, namespaced by `bot_id`, so two bot instances sharing the
same room never clobber each other's snapshot.

## 2. Diagram

```mermaid
flowchart TD
    BOT_A["Bot A (bot_id=bot-a)"]
    BOT_B["Bot B (bot_id=bot-b)"]
    ROOM["Shared room d-XXXX"]
    DAV_ROOM[(WebDAV d-XXXX/memory/)]
    DAV_SNAP_A[(WebDAV .snapshots/bot-a/d-XXXX/)]
    DAV_SNAP_B[(WebDAV .snapshots/bot-b/d-XXXX/)]
    L1_A[(Layer 1<br/>In-Memory Bot A)]
    L1_B[(Layer 1<br/>In-Memory Bot B)]
    SNAP_A[(snapshot.json<br/>Bot A)]
    SNAP_B[(snapshot.json<br/>Bot B)]
    L2[(Layer 2<br/>soul.md<br/>shared)]

    BOT_A --> L1_A
    BOT_B --> L1_B
    ROOM --> DAV_ROOM
    L1_A -->|"timer → persist"| SNAP_A
    L1_B -->|"timer → persist"| SNAP_B
    SNAP_A --> DAV_SNAP_A
    SNAP_B --> DAV_SNAP_B
    L2 --> DAV_ROOM
    DAV_ROOM -->|"GET soul.md"| BOT_A
    DAV_ROOM -->|"GET soul.md"| BOT_B
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.

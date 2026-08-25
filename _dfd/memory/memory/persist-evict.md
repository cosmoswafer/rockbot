# Persist & Evict

## 1. Purpose

A single periodic timer handles both crash-recovery snapshot persistence and
TTL-based eviction. The snapshot stores only Layer 1 (conversation history) —
it is bot-internal data written to a separate prefix
(`{snapshot_prefix}/{bot_id}/{wd}/snapshot.json`), isolated per bot instance.
After persisting, rooms idle longer than `memory_ttl_secs` are saved and
removed from the in-memory map.

When Layer 1 changes (new message, reset), the snapshot is marked dirty
and rebuilt on the next timer tick — writes are coalesced to avoid thrashing
WebDAV. Soul changes do NOT mark the snapshot dirty — soul is shared room
data stored in its own file.

## 2. Diagram

```mermaid
flowchart TD
    TIMER[Evict Timer]
    L1[(Layer 1<br/>Chat History)]
    WEBDAV[(NextCloud WebDAV<br/>snapshot_prefix)]
    LOAD_ROOM{More Rooms?}
    EMPTY{Room Empty?}
    DIRTY{Snapshot Dirty?}
    BUILD[Build Snapshot<br/>L1 only]
    PERSIST(Persist Snapshot)
    STALE{"now - last_activity<br/>> memory_ttl_secs"}
    EVICT(Remove Room<br/>from Memory)
    ROOMS[(RoomStateMap)]
    DONE[Done]

    TIMER -->|"every persist_interval_secs"| ROOMS
    ROOMS -->|"iterate rooms"| LOAD_ROOM
    LOAD_ROOM -->|"next room"| L1
    LOAD_ROOM -->|"no more"| DONE
    L1 -->|"room_id + messages + char_count"| EMPTY
    EMPTY -->|"no"| DIRTY
    EMPTY -->|"yes: skip"| LOAD_ROOM
    DIRTY -->|"yes: collect L1"| BUILD
    DIRTY -->|"no"| STALE
    BUILD --> PERSIST
    PERSIST -->|"PUT {snapshot_prefix}/{bot_id}/{wd}/snapshot.json"| WEBDAV
    PERSIST --> STALE
    STALE -->|"yes: evict"| EVICT
    STALE -->|"no: keep in memory"| LOAD_ROOM
    EVICT -->|"remove HashMap entry"| ROOMS
    EVICT --> LOAD_ROOM
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.

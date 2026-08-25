# Error Handling

## 1. Purpose

Failure paths for snapshot persistence, soul writes, and room-init loads:
failed snapshot writes keep the dirty flag and retry on the next timer tick;
failed soul writes and missing/partial snapshots warn and continue operating.

References: ../persist-evict.md, ../restore-room.md, ../soul-editing.md

## 2. Diagram

```mermaid
flowchart TD
    SOUL_WRITE[Write soul.md]
    SNAP_WRITE[Write snapshot.json]
    DAV[(NextCloud WebDAV)]
    LOAD[Load on Room Init]
    WARN[Warn + Continue]
    RETRY[Retry Next Tick]

    SNAP_WRITE -.->|"PUT failed"| RETRY
    SOUL_WRITE -.->|"PUT failed"| WARN
    LOAD -.->|"snapshot missing / partial"| WARN
    WARN -->|"fallback: read individual files"| LOAD
    RETRY -->|"keep dirty flag, retry on next timer"| SNAP_WRITE
```

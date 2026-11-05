# Error Handling

## 1. Purpose

Failure paths for snapshot persistence, soul reads/writes, legacy soul
migration, and room-init loads: failed snapshot writes keep the dirty flag and
retry on the next timer tick; failed soul writes warn and continue; a failed
legacy migration falls back to the legacy in-memory view; broken front matter
is rejected on write but preserved verbatim on read.

References: ../restore-room.md, ../soul-editing.md

## 2. Diagram

```mermaid
flowchart TD
    SOUL_WRITE[Write soul.md]
    SNAP_WRITE[Write snapshot.json]
    MIGRATE[Migrate legacy soul]
    DAV[(NextCloud WebDAV)]
    LOAD[Load on Room Init]
    WARN[Warn + Continue]
    RETRY[Retry Next Tick]
    LEGACY[Legacy View]
    REJECT[Reject SoulFormat]

    SNAP_WRITE -.->|"PUT failed"| RETRY
    SOUL_WRITE -.->|"PUT failed"| WARN
    SOUL_WRITE -.->|"broken front matter"| REJECT
    MIGRATE -.->|"PUT failed / already attempted"| LEGACY
    LOAD -.->|"snapshot missing / partial"| WARN
    LOAD -.->|"broken front matter<br/>(raw kept as body)"| LEGACY
    WARN -->|"fallback: read individual files"| LOAD
    RETRY -->|"keep dirty flag, retry on next timer"| SNAP_WRITE
    DAV -->|"soul.md"| LOAD
```

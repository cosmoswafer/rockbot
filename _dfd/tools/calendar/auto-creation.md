# WebDAV Calendar — Auto Creation

## 1. Purpose

The per-room calendar is auto-created on first use via CalDAV `MKCALENDAR`,
keyed by `room_id → webdav_dir` in the in-memory `room_calendars` map. The
ensure step is fast-pathed when the calendar is already cached; otherwise it
checks existence on NextCloud and creates it if missing.

- Downstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) injects `room_id` + `webdav_dir`
  into calendar tool arguments

## 2. Diagram

```mermaid
flowchart TD
    CALLER[Caller provides room_id + webdav_dir]
    MAP[(room_calendars HashMap)]
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]
    CHECK{Calendar in map?}
    EXISTS{Calendar exists on NC?}
    CREATE(MKCALENDAR)
    CAL_READY[Use per-room calendar URL]
    CAL_ERR[Warn — proceed with operation]

    CALLER --> CHECK
    CHECK -->|yes, cached| CAL_READY
    CHECK -->|no| EXISTS
    EXISTS -->|yes via PROPFIND| MAP
    EXISTS -->|no| CREATE
    EXISTS -->|error| CAL_ERR
    CREATE -->|201 created| MAP
    CREATE -->|error| CAL_ERR
    MAP --> CAL_READY
```

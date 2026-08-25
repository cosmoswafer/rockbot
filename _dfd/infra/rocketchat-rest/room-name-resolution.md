# Room Name Resolution — REST Fallback

## 1. Purpose

When the DDP `stream-room-messages` subscription delivers a `"changed"` event
without `fname` in `args[1]`, the message handler in `main.rs` falls back to
REST API room name resolution via `resolve_room_fname()`. The flow is gated by
a non-empty `auth_token` check to avoid the `assert!` panic that broke the
earlier attempt.

- Upstream: [RocketChat Connection](../rocketchat/main-path.md) — DDP
  `changed` events and `auth_token`
- Upstream: [Shared Structures](structures.md) — `rooms.get` / `rooms.info`
  endpoints, `RoomInfo`

## 2. Diagram

```mermaid
flowchart TD
    DDP[RocketChat DDP]
    PARSE(ParseIncomingMessage)
    CHECK{room_fname<br/>is empty?}
    DOWNCAST{Is RcPlatformSender?}
    AUTH{Has auth_token?}
    REST(RestApiClient<br/>resolve_room_fname)
    RC_API[RocketChat REST API]
    CACHE[(room_name_cache)]
    USE_FNAME["Use resolved fname"]
    DM_CHECK{is_dm?}
    USE_ROOM_NAME["Use room_name (username)<br/>as display name"]
    ERROR["Send error reply<br/>+ skip message"]

    DDP -->|"changed event"| PARSE
    PARSE -->|"IncomingMessage"| CHECK
    CHECK -->|"no"| USE_FNAME
    CHECK -->|"yes"| DOWNCAST
    DOWNCAST -->|"yes (RocketChat)"| AUTH
    DOWNCAST -->|"no (Matrix)"| DM_CHECK
    AUTH -->|"non-empty"| REST
    AUTH -->|"empty"| DM_CHECK
    REST -->|"check cache"| CACHE
    CACHE -->|"miss"| REST
    REST -->|"GET rooms.info?roomId="| RC_API
    RC_API -->|"RoomInfo {fname}"| REST
    REST -->|"fallback: GET rooms.get"| RC_API
    REST -->|"fname resolved"| USE_FNAME
    REST -->|"not found"| DM_CHECK
    DM_CHECK -->|"yes"| USE_ROOM_NAME
    DM_CHECK -->|"no"| ERROR
```

See [`_doc/rocketchat/room-name-fields.md`](../../../_doc/rocketchat/room-name-fields.md)
for the full rationale. If REST resolution fails and the room is **not** a DM
(`is_dm == false`), the bot sends an error reply and skips message processing.
For **DM rooms** (`is_dm == true`), the bot falls back to `room_name` (the
sender's username) as the display name instead of erroring. This is because
DMs in RocketChat never have an `fname` — they only have a `username` on the
other participant. The bot never panics from missing room names.

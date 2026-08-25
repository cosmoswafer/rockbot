# RocketChat REST API — Shared Structures

## 1. Overview

Extends the RockBot connection layer with **RocketChat REST API v1** calls for
two capabilities the legacy DDP `changed` events cannot reliably provide:
**(1)** lookup of Unicode-friendly room names (`fname`) that may be missing from
DDP events, and **(2)** sending messages with a per-message `alias` field to
override the sender's display name. The alias is sourced from the bot's
per-room soul memory (Layer 3) via `self_display_name()`.

Messages are sent **REST-first with alias**: the agent loop produces a reply
text, the bot's self-display name is extracted from soul memory, and the REST
`chat.sendMessage` endpoint is called with `alias`. If the REST call fails for
any reason, the system falls back to DDP `sendMessage` **without** alias.

- Upstream: [Configuration Management](../config/main-path.md) provides server
  hostname and TLS settings
- Upstream: [RocketChat Connection](../rocketchat/main-path.md) provides
  `user_id` and `auth_token` from the DDP `login` response
- Upstream: [Memory Management](../../memory/memory/soul-editing.md) Layer 3
  (soul) stores the bot's per-room self-display name, extracted via
  `self_display_name()`
- Downstream: Agent Loop (`main.rs`) orchestrates the REST-then-DDP send flow

## 3. Data Structures

### REST API Endpoints

#### `GET /api/v1/rooms.get`

Returns all rooms the authenticated user has joined.

**Request headers**: `X-Auth-Token`, `X-User-Id`

**Response** (`application/json`):
```json
{
    "update": [{
        "_id": "8g4gQkEAhewkGPkPL",
        "name": "shit",
        "fname": "💩💩💩SHIT屎",
        "t": "p",
        "msgs": 146779,
        "usersCount": 6
    }],
    "success": true
}
```

#### `GET /api/v1/rooms.info`

**Query params**: `roomId` (UUID) or `roomName` (ASCII slug only — Unicode
`fname` cannot be used as a query parameter).

**Response**:
```json
{
    "room": {
        "_id": "8g4gQkEAhewkGPkPL",
        "name": "shit",
        "fname": "💩💩💩SHIT屎",
        "t": "p",
        "msgs": 146779,
        "usersCount": 6
    },
    "success": true
}
```

#### `POST /api/v1/chat.sendMessage`

Sends a message. Supports `alias` (including Chinese/emoji like `"零夢✨"`).

**Request body**:
```json
{
    "message": {
        "rid": "GENERAL",
        "msg": "Hello world",
        "alias": "零夢✨"
    }
}
```

**Response**:
```json
{
    "message": {
        "_id": "Bf8dNR3WWJXaxdMyT",
        "rid": "GENERAL",
        "msg": "Hello world",
        "alias": "零夢✨",
        "u": { "_id": "wEv8J45KntNhDdkeY", "username": "rockai", "name": "香菜" },
        "ts": { "$date": 1781112548565 }
    },
    "success": true
}
```

#### `GET /api/v1/chat.getMessage`

Retrieves a single message by `_id`. Useful for verifying alias propagation.

**Response**: message object with `alias` field preserved.

#### `POST /api/v1/users.setAvatar`

Sets the bot's avatar from a URL. Local file paths are never used.

**Request body**:
```json
{
    "avatarUrl": "https://example.com/avatar.png"
}
```

#### `POST /api/v1/rooms.upload`

Uploads a file to a RocketChat room. Used for sending attachments (e.g. generated images via DDP fallback with `data:` URIs).

**Request**: multipart form with `file`, `room_id`, and optional `msg`, `description`.

### Rust Types

#### `RestApiClient`

Wraps `reqwest::Client` and holds auth headers. Created once per send from the
`MessageSender` which captures `user_id` and `auth_token` during DDP login.

| Field        | Type              | Purpose                           |
| ------------ | ----------------- | --------------------------------- |
| `host`       | `String`          | Server hostname (from config)     |
| `use_tls`    | `bool`            | HTTPS if true                     |
| `user_id`    | `String`          | `X-User-Id` header value          |
| `auth_token` | `String`          | `X-Auth-Token` header value       |
| `http`       | `reqwest::Client` | Reusable HTTP client              |
| `room_name_cache` | `HashMap<String, String>` | Caches resolved `fname` values from `rooms.info`/`rooms.get` — currently per-client-instance (created fresh per message handler call), shared across the `resolve_room_fname` → `get_room_info` → `get_rooms` cascade |

#### `RoomInfo`

| Field   | Type     | Source                         |
| ------- | -------- | ------------------------------ |
| `id`    | `String` | `rooms.get.update[]._id`       |
| `name`  | `String` | URL slug (ASCII)               |
| `fname` | `String` | Friendly name (Unicode)        |
| `t`     | `String` | Room type: `d`, `p`, `c`       |

### Implementation Map

| Component          | Source File                        |
| ------------------ | ---------------------------------- |
| `RestApiClient`    | `crate-rocketchat/src/rest.rs`     |
| REST endpoints     | `crate-rocketchat/src/rest.rs`     |
| `rest_client()`    | `crate-rocketchat/src/client.rs`   |
| Token capture      | `crate-rocketchat/src/client.rs`   |
| Room name cache    | `crate-rocketchat/src/rest.rs`     |
| Alias send         | `crate-rockbot/src/main.rs`        |
| `self_display_name`| `crate-rockbot/src/memory.rs`      |

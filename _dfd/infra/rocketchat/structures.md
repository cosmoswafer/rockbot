# RocketChat — Shared Structures

## 1. Overview

Rust crate (`crate-rocketchat`) that manages the full lifecycle of a
RocketChat connection over **DDP (Distributed Data Protocol)** via WebSocket:
authentication, subscription to message stream, event dispatch, message
parsing/filtering, and reply delivery. DMs, messages that start with or contain `@botname` or the bot's
self-display name (emoji stripped), and room-specific registered callbacks
are forwarded to the agent.

This crate also implements the `MessagingClient` trait (defined in
`crate-rockbot/src/platform/mod.rs`) via `RocketChatPlatform`, which wraps
`RocketChatClient` and `RestApiClient` together. The trait provides the
agent loop with a platform-agnostic interface — the same `IncomingMessage`
type is shared with the [Matrix platform](../matrix/main-path.md).

> **Deprecation note**: Rocket.Chat's official documentation marks the raw
> DDP/bots approach as **deprecated** (2025). The recommended replacement is
> [`@rocket.chat/ddp-client`](https://www.npmjs.com/package/@rocket.chat/ddp-client)
> or the [Apps-Engine](https://developer.rocket.chat/docs/rocketchat-apps-engine).

- Upstream: [Configuration Management](../config/main-path.md) provides configuration
  (typed `RocketChatConfig` deserialized from TOML via `serde`)
- Downstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) receives filtered `IncomingMessage`
  structs via async callback; sends replies through `MessagingClient::send_reply()`
- See also: [Matrix Connection](../matrix/main-path.md) for the alternative platform

## 3. Data Structures

The Rust crate defines formal typed structs with `serde` (Serialize/Deserialize)
in `crate-rocketchat/src/types.rs`. Tables below map each field to its struct
definition and how it is populated.

#### `IncomingMessage` (fields defined in `types.rs`)

| Field         | Type              | Source / Notes                                      |
| ------------- | ----------------- | --------------------------------------------------- |
| `msg_id`      | `Option<String>`  | `raw["id"]` — DDP message ID                        |
| `room_id`     | `String`          | `args[0]["rid"]` — RocketChat room ID               |
| `room_name`   | `String`          | `args[1]["roomName"]` — URL slug (ASCII, e.g. `sen1-lin2-sheng1-tai4`). For DMs: `""` or `"DIRECT_MESSAGES"` on legacy servers, or the other user's username on servers that send the `t` field |
| `room_fname`  | `String`          | Per-event `args[1]["fname"]`. Empty when absent from the DDP event or for rooms without a custom fname |
| `sender_name` | `String`          | `args[0]["u"]["username"]` — sender's RocketChat username |
| `text`        | `String`          | `args[0]["msg"]` — message body text                |
| `is_dm`       | `bool`            | `true` when `args[1]["t"] == "d"` (room type), falling back to `room_name` empty or `"DIRECT_MESSAGES"` for legacy servers without the `t` field |
| `timestamp`   | `Option<i64>`     | `args[0]["ts"]` — message timestamp (`$date`)       |
| `sender_id`   | `String`          | `args[0]["u"]["_id"]` — sender's RocketChat user ID |
| `alias`       | `Option<String>`  | `args[0]["alias"]` — sender alias                   |
| `file`        | `Option<FileInfo>` | `args[0]["file"]` — primary file metadata (present when message has an attachment) |
| `files`       | `Vec<FileInfo>`   | `args[0]["files"]` — all file variants (original + thumbnails) |
| `attachments` | `Vec<AttachmentInfo>` | `args[0]["attachments"]` — attachment objects with download URLs |
| `urls`        | `Vec<MessageUrl>`    | `args[0]["urls"]` — URL preview metadata (content type, content length). Used by harness to detect image URLs for auto-injection into image_gen. |

Room name precedence:
- **Matching/registration**: use `room_name` (slug) — always ASCII, deterministic
- **Display/log messages**: use `room_fname` — when absent from DDP, resolved via REST API; if REST also fails and the room is a DM, fall back to `room_name` (username); otherwise send an error reply and skip processing
- **WebDAV directory naming**: `compute_webdav_dir` uses `room_fname` exclusively — **panics** if empty (safety net; resolved upstream before reaching harness)

The agent harness computes `webdav_dir` using `room_fname`:
- **Channel with fname** (e.g. `#森林生態`): DDP supplies `roomName: "sen1-lin2-sheng1-tai4"` + `fname: "🐵🌴🐷森林生態"` → `webdav_dir: "r-🐵🌴🐷森林生態"`
- **Channel without fname in DDP** (e.g. `#general`, or any channel where `args[1]` omits `fname`): `room_fname` is resolved via REST API fallback (`resolve_room_fname` in `crate-rockbot/src/main.rs:417-451`) — calls `GET /api/v1/rooms.info?roomId=` to fetch the display name. If REST fails and the room is a DM, the bot uses the username as the display name (see [`rocketchat-rest.md §2d`](../rocketchat-rest/room-name-resolution.md)). For non-DM rooms, the bot sends an error reply and skips processing.

The flat `r-`/`d-` prefixes prevent collisions. Room name resolution prefers
`args[1].fname` from DDP, with a REST fallback when `fname` is absent (see
[`room-name-fields.md`](../../../_doc/rocketchat/room-name-fields.md) for details).

> **Important distinction**: `room_id` (the RocketChat UUID from DDP `args[0].rid`)
> and `webdav_dir` (the `r-`/`d-`-prefixed path key) are **separate values**.
> `room_id` is used as a stable in-memory lookup key. `webdav_dir` is used for
> WebDAV path construction. Tool calls receive both via `inject_room_context`.

#### `BotReply`

| Field       | Type              | Constructor                          |
| ----------- | ----------------- | ------------------------------------ |
| `room_id`   | `String`          | `MessageSender::room_id()`           |
| `text`      | `String`          | `MessageSender::reply(text)`         |
| `alias`     | `Option<String>`  | `BotReply::new()` defaults to `None` |
| `thread_id` | `Option<String>`  | Reserved for threaded replies (`tmid`) |

`MessageSender` also provides `reply_code(text)` (code-block format),
`reply_with_alias(text, alias)` (DDP aliased reply — used in tests, not
production), and `typing(state, username)` (typing indicator).

**Production alias flow**: the alias is not part of `BotReply`. Instead,
`main.rs` extracts the bot's self-display name from soul memory via
`MemoryManager::self_display_name(room_id)`, then sends via REST
`chat.sendMessage` with alias. On REST failure, falls back to DDP
`sendMessage` without alias. See [RocketChat REST API](../rocketchat-rest/rest-alias-send.md).

No `DdpEvent` struct exists. Raw DDP frames are handled as `serde_json::Value`
with the `"msg"` field extracted via helper functions: `msg_field()`, `is_ping()`,
`is_changed()`, etc. (`ddp.rs:68-101`).

#### `FileInfo`

| Field      | Type     | Source                                   |
| ---------- | -------- | ---------------------------------------- |
| `_id`      | `String` | `args[0]["file"]["_id"]`                 |
| `name`     | `String` | `args[0]["file"]["name"]`                |
| `type`     | `String` | MIME type (e.g. `image/png`)             |
| `size`     | `u64`    | File size in bytes                       |
| `format`     | `Option<String>` | File extension (e.g. `png`)              |
| `type_group` | `Option<String>` | `"image"`, `"video"`, `"thumb"`, etc.  |

#### `AttachmentInfo`

| Field             | Type                | Source                                       |
| ----------------- | ------------------- | -------------------------------------------- |
| `title`           | `Option<String>`    | Display title (filename)                     |
| `title_link`      | `Option<String>`    | Relative path to **original file** download  |
| `title_link_download` | `Option<bool>`  | True for file uploads                        |
| `image_url`       | `Option<String>`    | Relative path to **thumbnail** image         |
| `image_type`      | `Option<String>`    | MIME type                                    |
| `image_size`      | `Option<u64>`       | Original file size in bytes                  |
| `image_dimensions`| `Option<ImageDim>`  | `{width, height}` pixel dimensions           |
| `image_preview`   | `Option<String>`    | Base64-encoded inline preview                |
| `type`            | `Option<String>`    | `"file"` for uploads                         |
| `file_id`         | `Option<String>`    | Back-reference to original `file._id`        |

To construct the full download URL: join `{server_config.host()}{attachment.title_link}`. The `image_url` field points to a thumbnail variant — use `title_link` for the original, full-quality image.

#### `MessageUrl`

| Field     | Type                | Source                              |
|-----------|---------------------|-------------------------------------|
| `url`     | `String`            | The URL string                      |
| `meta`    | `Option<Value>`     | RocketChat server metadata (JSON)   |
| `headers` | `Option<UrlHeaders>`| HTTP response headers for the URL   |

#### `UrlHeaders`

| Field             | Type            | Source                         |
|-------------------|-----------------|--------------------------------|
| `content_length`  | `Option<String>`| `contentLength` header value   |
| `content_type`    | `Option<String>`| `contentType` header value     |

Image URLs are detected by `headers.contentType` matching `image/*` — the harness populates `current_image_urls` from these and auto-injects them into `image_gen` calls, bypassing vision for text-only models.

#### `MessageFilter`

| Field         | Type      | Purpose                            |
| ------------- | --------- | ---------------------------------- |
| `bot_user_id` | `&str`    | User ID to filter out self-messages|

Method `filter(&self, raw: &Value) -> Option<IncomingMessage>` parses and
filters a raw DDP event, returning `None` for self-messages and `Some` for
valid incoming messages. The dispatch decision (DM, mention, display name,
registered rooms) is implemented inline in `client.rs:226-235` at the
`connect_and_run` event loop level. `room_fname` is parsed directly from the
per-event `args[1].fname` field; when absent, the `main.rs` message handler
performs a secondary REST API lookup via `resolve_room_fname()` (see
[rocketchat-rest.md §2d](../rocketchat-rest/room-name-resolution.md)).

#### `RocketChatClient`

| Field          | Type              | Purpose                            |
| -------------- | ----------------- | ---------------------------------- |
| `bot_name`       | `String`              | `@username` for mention matching (not stripping — stripping is via `RcPlatformSender::strip_mention_prefix`) |
| `config`         | `RocketChatConfig`   | Server connection configuration             |
| `username`       | `String`             | Bot login username                          |
| `user_id`        | `Option<String>`     | User ID received after authentication       |
| `auth_token`     | `Option<String>`     | Auth token received after login             |
| `registered_rooms` | `HashMap<String, bool>` | Rooms the bot should listen in regardless of mentions |

*(Room name cache replaced with REST-based resolution — see [rocketchat-rest.md §2d](../rocketchat-rest/room-name-resolution.md) and `room-name-fields.md` for details.)*

#### `RocketChatPlatform` (crate-rockbot/src/platform/rocketchat.rs)

Wrapper that implements the `MessagingClient` trait for RocketChat. Composes
`RocketChatClient` (DDP) and `RestApiClient` (REST). Room name resolution is
handled by the `main.rs` message handler via `RcPlatformSender` downcast —
see [rocketchat-rest.md §2d](../rocketchat-rest/room-name-resolution.md).

| `MessagingClient` method | RocketChat implementation                                    |
| ------------------------ | ------------------------------------------------------------ |
| `connect_and_run()`      | Delegates to `RocketChatClient::connect_and_run()` with the handler callback |
| `bot_id()`               | Returns `&self.bot_name` (the `@username` value passed to `RocketChatPlatform::new()`). Used by `main.rs` as the per-bot namespace for WebDAV snapshot paths and as the canonical bot identifier passed to `AgentHarness::new()`. Non-emptiness is guaranteed by `ServerConfig.username` validation (`#[validate(min_length = 1)]`). |

The platform wrapper is constructed in `main.rs::run_bot()` after config loading
and provider wiring. It is passed to the reconnect loop which calls
`connect_and_run()` with the message dispatch closure. `bot_name` (`@username`)
is passed to `RocketChatPlatform::new()` and used for mention **checking** in
the DDP client (`client.rs` — `msg.text.starts_with(&bot_name)`). Mention
**stripping** is handled by `RcPlatformSender::strip_mention_prefix()`.

`bot_name` is also exposed via the `MessagingClient::bot_id()` trait method,
which `main.rs` calls to obtain the `bot_id` value passed to
`AgentHarness::new()` (issue #58 — the harness no longer derives `bot_id`
from `config.platform.name`).

#### `RcPlatformSender` (implements `PlatformSender`)

Per-message platform handle created inside the DDP event handler. Stores the
DDP `MessageSender`, bot username, and RocketChat config for reply sending
and mention stripping.

| `PlatformSender` method   | RocketChat implementation                                    |
| ------------------------- | ------------------------------------------------------------ |
| `send_reply()`            | DDP `MessageSender::reply()` — REST-first alias send orchestrated by `main.rs` |
| `send_reply_with_attachments()` | DDP `MessageSender::reply_with_attachments()`          |
| `send_typing()`           | DDP `stream-notify-room/typing` via `MessageSender::typing()` |
| `strip_mention_prefix()`  | Strips `@username ` or `@username` from text start (non-DM)  |
| `room_id()`               | DDP `MessageSender.room_id`                                  |
| `as_any()`                | Enables `main.rs` downcast for REST alias sends              |
